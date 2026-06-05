use crate::platform;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const CLAUDE_DIR_NAME: &str = "Claude";
const MCP_CONFIG_FILE: &str = "claude_desktop_config.json";
const BACKUPS_DIR_NAME: &str = ".backups";
const RUN_MODE_FILE: &str = ".run-mode";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Switch,
    MultiInstance,
}

impl RunMode {
    fn from_str(s: &str) -> Self {
        if s.trim() == "multi" {
            Self::MultiInstance
        } else {
            Self::Switch
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Switch => "switch",
            Self::MultiInstance => "multi",
        }
    }
}

pub fn run_mode() -> RunMode {
    fs::read_to_string(profiles_dir().join(RUN_MODE_FILE))
        .map(|s| RunMode::from_str(&s))
        .unwrap_or(RunMode::Switch)
}

pub fn set_run_mode(mode: RunMode) {
    let dir = profiles_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join(RUN_MODE_FILE), mode.as_str());
}

pub fn profile_desktop_path(name: &str) -> PathBuf {
    platform::profile_desktop_dir(name)
}

pub fn is_profile_running(name: &str) -> bool {
    if platform::is_profile_desktop_running(name) {
        return true;
    }
    current_profile().as_deref() == Some(name) && platform::is_default_desktop_running()
}

pub fn running_profiles() -> Vec<String> {
    list_profiles()
        .into_iter()
        .filter(|p| is_profile_running(p))
        .collect()
}

pub fn active_tooltip() -> String {
    match run_mode() {
        RunMode::Switch => current_profile().unwrap_or_else(|| "(unmanaged)".into()),
        RunMode::MultiInstance => {
            let running = running_profiles();
            if running.is_empty() {
                "none running".into()
            } else {
                running.join(", ")
            }
        }
    }
}

pub fn claude_app_dir() -> PathBuf {
    platform::claude_app_dir()
}

pub fn profiles_dir() -> PathBuf {
    platform::profiles_dir()
}

pub fn backups_dir() -> PathBuf {
    profiles_dir().join(BACKUPS_DIR_NAME)
}

pub fn list_profiles() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(profiles_dir()) {
        for entry in rd.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with('.') {
                        continue;
                    }
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

const ACTIVE_MARKER: &str = ".active";

fn read_active_marker() -> Option<String> {
    fs::read_to_string(profiles_dir().join(ACTIVE_MARKER))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_active_marker(name: &str) -> Result<(), String> {
    fs::write(profiles_dir().join(ACTIVE_MARKER), name)
        .map_err(|e| format!("write active marker: {e}"))
}

pub fn current_profile() -> Option<String> {
    let link = claude_app_dir();
    if let Some(from_link) = platform::read_link_target(&link).and_then(|target| {
        let pdir = fs::canonicalize(profiles_dir()).ok()?;
        let rel = target.strip_prefix(&pdir).ok()?;
        rel.components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
    }) {
        return Some(from_link);
    }
    read_active_marker()
}

fn dir_is_empty(p: &Path) -> bool {
    fs::read_dir(p).map(|mut i| i.next().is_none()).unwrap_or(true)
}

fn profile_has_session(path: &Path) -> bool {
    let config = path.join("config.json");
    fs::read_to_string(&config)
        .ok()
        .is_some_and(|s| s.contains("oauth:tokenCache"))
}

fn persist_live_at(app_dir: &Path, profile_path: &Path) -> Result<(), String> {
    if profile_path.exists() {
        if profile_has_session(profile_path) && !profile_has_session(app_dir) {
            return Err(format!(
                "refusing to overwrite authenticated profile '{}' with an unauthenticated session",
                profile_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("profile")
            ));
        }
        let stale = profile_path.with_extension("stale");
        fs::remove_dir_all(&stale).ok();
        fs::rename(profile_path, &stale)
            .map_err(|e| format!("rotate profile dir: {e}"))?;
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(stale);
        });
    }
    fs::rename(app_dir, profile_path).map_err(|e| format!("save live profile: {e}"))
}

fn sibling_profile(target: &str) -> Option<String> {
    let others: Vec<String> = list_profiles()
        .into_iter()
        .filter(|p| p != target)
        .collect();
    if others.len() == 1 {
        Some(others[0].clone())
    } else {
        None
    }
}

pub fn switch_profile(name: &str) -> Result<(), String> {
    let profiles = profiles_dir();
    fs::create_dir_all(&profiles).map_err(|e| format!("mkdir profiles: {e}"))?;
    let target = profiles.join(name);

    platform::kill_claude();

    let app_dir = claude_app_dir();
    let leaving = current_profile();

    if platform::is_link(&app_dir) {
        platform::remove_link(&app_dir)?;
    } else if app_dir.is_dir() {
        if let Some(ref src) = leaving {
            if src != name {
                persist_live_at(&app_dir, &profiles.join(src))?;
            } else if app_dir != target {
                persist_live_at(&app_dir, &target)?;
            }
        } else if !target.exists() || dir_is_empty(&target) {
            if target.exists() && dir_is_empty(&target) {
                fs::remove_dir(&target).ok();
            }
            fs::rename(&app_dir, &target)
                .map_err(|e| format!("migrate config -> profile: {e}"))?;
        } else if let Some(sibling) = sibling_profile(name) {
            persist_live_at(&app_dir, &profiles.join(&sibling))?;
        } else {
            backup_dir(&app_dir, "preswitch")?;
        }
    } else if app_dir.exists() {
        if platform::is_link(&app_dir) {
            platform::remove_link(&app_dir)?;
        } else {
            fs::remove_file(&app_dir).ok();
        }
    }

    fs::create_dir_all(&target).map_err(|e| format!("mkdir target: {e}"))?;
    platform::create_link(&target, &app_dir)?;
    write_active_marker(name)
}

fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

fn backup_path(label: &str, suffix: &str) -> Result<PathBuf, String> {
    let dir = backups_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir backups: {e}"))?;
    Ok(dir.join(format!("{}_{}_{}", timestamp(), label, suffix)))
}

fn backup_file(path: &Path, label: &str) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let suffix = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let dest = backup_path(label, suffix)?;
    fs::copy(path, &dest).map_err(|e| format!("backup copy: {e}"))?;
    Ok(Some(dest))
}

fn backup_dir(path: &Path, label: &str) -> Result<PathBuf, String> {
    let suffix = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(CLAUDE_DIR_NAME);
    let dest = backup_path(label, suffix)?;
    fs::rename(path, &dest).map_err(|e| format!("backup dir: {e}"))?;
    Ok(dest)
}

pub fn current_config_path() -> PathBuf {
    claude_app_dir().join(MCP_CONFIG_FILE)
}

pub fn profile_config_path(profile: &str) -> PathBuf {
    profiles_dir().join(profile).join(MCP_CONFIG_FILE)
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| json!({}))
}

pub fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let label = current_profile().unwrap_or_else(|| "unmanaged".into());
    backup_file(path, &label)?;
    let s = serde_json::to_string_pretty(v).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, s).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

pub fn list_mcp_servers(path: &Path) -> Vec<String> {
    let v = read_json(path);
    let mut names: Vec<String> = v
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

pub fn import_mcp_servers(from: &Path, to: &Path, names: &[String]) -> Result<usize, String> {
    let src = read_json(from);
    let src_servers = src
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .ok_or("source has no mcpServers")?;

    let mut dst = read_json(to);
    if !dst.is_object() {
        dst = json!({});
    }
    let dst_obj = dst.as_object_mut().unwrap();
    let entry = dst_obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    let dst_servers = entry.as_object_mut().unwrap();

    let mut copied = 0;
    for name in names {
        if let Some(server) = src_servers.get(name) {
            dst_servers.insert(name.clone(), server.clone());
            copied += 1;
        }
    }
    write_json(to, &dst)?;
    Ok(copied)
}

pub fn clear_mcp_servers(path: &Path) -> Result<(), String> {
    let mut v = read_json(path);
    if !v.is_object() {
        v = json!({});
    }
    if let Some(obj) = v.as_object_mut() {
        obj.insert("mcpServers".into(), json!({}));
    }
    write_json(path, &v)
}

pub fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.starts_with('.')
}

pub fn rename_profile(
    old: &str,
    new: &str,
    relaunch_after_switch: bool,
) -> Result<(), String> {
    if new == old {
        return Ok(());
    }
    let old_path = profiles_dir().join(old);
    let new_path = profiles_dir().join(new);
    if !old_path.exists() {
        return Err(format!("'{old}' not found"));
    }
    if new_path.exists() {
        return Err(format!("'{new}' already exists"));
    }
    #[cfg(target_os = "linux")]
    if code_profile_dir(new).exists() || code_profile_json(new).exists() {
        return Err(format!("'{new}' already exists (Claude Code)"));
    }

    let is_active = current_profile().as_deref() == Some(old);
    let was_running = platform::is_claude_running();

    if is_active {
        if was_running
            && !platform::confirm(&format!(
                "Claude is running and must close to rename the active profile '{old}'. Continue?"
            ))
        {
            return Err("cancelled".into());
        }
        platform::kill_claude();
        let app_dir = claude_app_dir();
        if platform::is_link(&app_dir) {
            platform::remove_link(&app_dir)?;
        }
        fs::rename(&old_path, &new_path).map_err(|e| format!("rename: {e}"))?;
        platform::create_link(&new_path, &app_dir)?;
        write_active_marker(new)?;
        #[cfg(target_os = "linux")]
        rename_code_profile(old, new, true)?;
        if was_running && relaunch_after_switch {
            platform::relaunch_claude();
        }
    } else {
        fs::rename(&old_path, &new_path).map_err(|e| format!("rename: {e}"))?;
        #[cfg(target_os = "linux")]
        rename_code_profile(old, new, false)?;
    }
    Ok(())
}

pub fn after_config_change() {
    if platform::is_claude_running()
        && platform::confirm(
            "Claude is running. Restart it now to load the new MCP config?",
        )
    {
        platform::kill_claude();
        platform::relaunch_claude();
    }
}

#[cfg(target_os = "linux")]
fn ensure_code_profile(name: &str) -> Result<(), String> {
    let dir = code_profile_dir(name);
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir code profile: {e}"))?;

    let legacy_json = code_profile_json(name);
    let in_dir_json = dir.join(".claude.json");
    if !in_dir_json.exists() {
        if legacy_json.exists() {
            fs::copy(&legacy_json, &in_dir_json).map_err(|e| format!("migrate code json: {e}"))?;
        } else {
            fs::write(&in_dir_json, "{}\n").map_err(|e| format!("init code json: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn ensure_code_profile(_name: &str) -> Result<(), String> {
    Ok(())
}

pub fn ensure_profile_ready(name: &str) -> Result<(), String> {
    if !valid_profile_name(name) {
        return Err("invalid profile name".into());
    }
    fs::create_dir_all(profiles_dir()).map_err(|e| format!("mkdir profiles: {e}"))?;
    fs::create_dir_all(profile_desktop_path(name))
        .map_err(|e| format!("mkdir desktop profile: {e}"))?;
    ensure_code_profile(name)
}

pub fn launch_profile(name: &str) -> Result<(), String> {
    ensure_profile_ready(name)?;
    if is_profile_running(name) {
        return Ok(());
    }
    platform::launch_profile_instance(name)
}

pub fn close_profile(name: &str) {
    if platform::is_profile_desktop_running(name) {
        platform::kill_profile_instance(name);
        return;
    }
    if current_profile().as_deref() == Some(name) && platform::is_default_desktop_running() {
        platform::kill_claude();
    }
}

pub fn toggle_profile(name: &str) {
    if is_profile_running(name) {
        close_profile(name);
        platform::notify(&format!("Closed '{name}'"));
    } else {
        match launch_profile(name) {
            Ok(()) => platform::notify(&format!("Launched '{name}'")),
            Err(e) => platform::notify(&format!("Launch failed: {e}")),
        }
    }
}

pub fn launch_all_profiles() {
    let profiles = list_profiles();
    if profiles.is_empty() {
        platform::notify("No profiles to launch");
        return;
    }
    let mut launched = 0usize;
    for name in &profiles {
        if !is_profile_running(name) {
            if launch_profile(name).is_ok() {
                launched += 1;
            }
        }
    }
    if launched == 0 {
        platform::notify("All profiles already running");
    } else {
        platform::notify(&format!("Launched {launched} profile(s)"));
    }
}

pub fn close_all_profiles() {
    if running_profiles().is_empty() && !platform::is_default_desktop_running() {
        platform::notify("No profiles running");
        return;
    }
    platform::kill_all_profile_instances();
    if platform::is_default_desktop_running() {
        platform::kill_claude();
    }
    platform::notify("Closed all profiles");
}

pub fn set_primary_profile(name: &str) -> Result<(), String> {
    let profiles = profiles_dir();
    let target = profiles.join(name);
    if !target.is_dir() {
        return Err(format!("'{name}' not found"));
    }

    let app_dir = claude_app_dir();
    if platform::is_link(&app_dir) {
        platform::remove_link(&app_dir)?;
    } else if app_dir.exists() {
        if app_dir.is_dir() {
            backup_dir(&app_dir, "primary-set")?;
        } else {
            fs::remove_file(&app_dir).ok();
        }
    }
    platform::create_link(&target, &app_dir)?;
    write_active_marker(name)?;

    #[cfg(target_os = "linux")]
    if current_code_profile().as_deref() != Some(name) {
        capture_unmanaged_code(name)?;
        link_code(name)?;
    }
    Ok(())
}

pub fn toggle_run_mode() -> RunMode {
    let next = match run_mode() {
        RunMode::Switch => RunMode::MultiInstance,
        RunMode::MultiInstance => RunMode::Switch,
    };
    if next == RunMode::Switch && !running_profiles().is_empty() {
        if !platform::confirm(
            "Switching to single-profile mode will close all running Claude instances. Continue?",
        ) {
            return run_mode();
        }
        close_all_profiles();
    }
    set_run_mode(next);
    next
}

pub fn do_new_profile(name: &str, relaunch_after_switch: bool) {
    match run_mode() {
        RunMode::Switch => do_switch(name, relaunch_after_switch),
        RunMode::MultiInstance => match ensure_profile_ready(name) {
            Ok(()) => toggle_profile(name),
            Err(e) => platform::notify(&format!("Create failed: {e}")),
        },
    }
}

pub fn do_switch(name: &str, relaunch_after_switch: bool) {
    if platform::is_claude_running()
        && !platform::confirm(
            "Claude Desktop and Claude Code will be closed to switch profile. Continue?",
        )
    {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let outgoing = current_profile().unwrap_or_else(|| name.to_string());
        platform::kill_claude();
        if let Err(e) = capture_unmanaged_code(&outgoing) {
            platform::notify(&format!("Claude Code capture failed: {e}"));
            return;
        }
    }
    match switch_profile(name) {
        Ok(()) => {
            #[cfg(target_os = "linux")]
            if let Err(e) = link_code(name) {
                platform::notify(&format!("Switched Desktop, but Claude Code link failed: {e}"));
                if relaunch_after_switch {
                    platform::relaunch_claude();
                }
                return;
            }
            platform::notify(&format!("Switched to '{name}'"));
            if relaunch_after_switch {
                platform::relaunch_claude();
            }
        }
        Err(e) => platform::notify(&format!("Switch failed: {e}")),
    }
}

pub fn backup_count() -> usize {
    fs::read_dir(backups_dir())
        .map(|rd| rd.flatten().count())
        .unwrap_or(0)
}

pub fn open_profiles_dir() {
    platform::open_path(&profiles_dir());
}

pub fn open_backups_dir() {
    platform::open_path(&backups_dir());
}

pub fn open_config() {
    platform::open_path(&current_config_path());
}

pub fn prompt_new_profile() -> Option<String> {
    platform::prompt_text("Claude Switcher", "New profile name:", None).and_then(|name| {
        if valid_profile_name(&name) {
            Some(name)
        } else {
            None
        }
    })
}

pub fn prompt_rename(old: &str) -> Option<String> {
    platform::prompt_text("Rename profile", &format!("Rename '{old}' to:"), Some(old))
        .and_then(|name| {
            if valid_profile_name(&name) && name != old {
                Some(name)
            } else {
                None
            }
        })
}

pub fn import_from_profile(source: &str) {
    let src = profile_config_path(source);
    let servers = list_mcp_servers(&src);
    if let Some(picks) = platform::pick_servers(source, &servers) {
        match import_mcp_servers(&src, &current_config_path(), &picks) {
            Ok(n) => {
                platform::notify(&format!("Imported {n} server(s) from '{source}'"));
                after_config_change();
            }
            Err(e) => platform::notify(&format!("Import failed: {e}")),
        }
    }
}

#[cfg(target_os = "windows")]
pub fn import_single_server(source: &str, server: &str) {
    if let Some(picks) = platform::pick_single_server(source, server) {
        let src = profile_config_path(source);
        match import_mcp_servers(&src, &current_config_path(), &picks) {
            Ok(n) => {
                platform::notify(&format!("Imported {n} server(s) from '{source}'"));
                after_config_change();
            }
            Err(e) => platform::notify(&format!("Import failed: {e}")),
        }
    }
}

pub fn clear_all_mcp() {
    if !platform::confirm("Remove all MCP servers from the active profile?") {
        return;
    }
    match clear_mcp_servers(&current_config_path()) {
        Ok(()) => {
            platform::notify("Cleared MCP servers");
            after_config_change();
        }
        Err(e) => platform::notify(&format!("Clear failed: {e}")),
    }
}

// ── Claude Code profile management (Linux only) ──────────────────────────────

#[cfg(target_os = "linux")]
fn code_profile_dir(name: &str) -> std::path::PathBuf {
    platform::code_profiles_dir().join(name)
}

#[cfg(target_os = "linux")]
fn code_profile_json(name: &str) -> std::path::PathBuf {
    platform::code_profiles_dir().join(format!("{name}.json"))
}

#[cfg(target_os = "linux")]
pub fn current_code_profile() -> Option<String> {
    platform::read_link_target(&platform::code_app_dir()).and_then(|target| {
        let base = fs::canonicalize(platform::code_profiles_dir()).ok()?;
        let rel = target.strip_prefix(&base).ok()?;
        rel.components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
    })
}

// Move a live, not-yet-managed ~/.claude (+ ~/.claude.json) into `owner`'s
// code profile. No-op if already a symlink. Caller must close Claude first.
#[cfg(target_os = "linux")]
pub fn capture_unmanaged_code(owner: &str) -> Result<(), String> {
    let app = platform::code_app_dir();
    match fs::symlink_metadata(&app) {
        Ok(m) if m.file_type().is_symlink() => {}
        Ok(m) if m.is_dir() => {
            fs::create_dir_all(platform::code_profiles_dir())
                .map_err(|e| format!("mkdir code profiles: {e}"))?;
            let target = code_profile_dir(owner);
            if !target.exists() {
                fs::rename(&app, &target).map_err(|e| format!("capture ~/.claude: {e}"))?;
            } else if dir_is_empty(&target) {
                fs::remove_dir(&target).ok();
                fs::rename(&app, &target).map_err(|e| format!("capture ~/.claude: {e}"))?;
            } else {
                backup_dir(&app, "preswitch-code")?;
            }
        }
        Ok(_) => { fs::remove_file(&app).ok(); }
        Err(_) => {}
    }

    let appjson = platform::code_app_json();
    match fs::symlink_metadata(&appjson) {
        Ok(m) if m.file_type().is_symlink() => {}
        Ok(_) => {
            fs::create_dir_all(platform::code_profiles_dir())
                .map_err(|e| format!("mkdir code profiles: {e}"))?;
            let target = code_profile_json(owner);
            if !target.exists() {
                fs::rename(&appjson, &target)
                    .map_err(|e| format!("capture ~/.claude.json: {e}"))?;
            } else {
                backup_file(&appjson, "preswitch-code")?;
                fs::remove_file(&appjson).ok();
            }
        }
        Err(_) => {}
    }
    Ok(())
}

// Point ~/.claude and ~/.claude.json at `name`'s code profile, creating it if
// needed. Caller must close Claude first.
#[cfg(target_os = "linux")]
pub fn link_code(name: &str) -> Result<(), String> {
    use std::os::unix::fs as unix_fs;

    let target = code_profile_dir(name);
    fs::create_dir_all(&target).map_err(|e| format!("mkdir code profile: {e}"))?;

    let app = platform::code_app_dir();
    match fs::symlink_metadata(&app) {
        Ok(m) if m.file_type().is_symlink() => {
            fs::remove_file(&app).map_err(|e| format!("unlink ~/.claude: {e}"))?;
        }
        Ok(m) if m.is_dir() => { backup_dir(&app, "preswitch-code")?; }
        Ok(_) => { fs::remove_file(&app).ok(); }
        Err(_) => {}
    }
    unix_fs::symlink(&target, &app).map_err(|e| format!("symlink ~/.claude: {e}"))?;

    let target_json = code_profile_json(name);
    if !target_json.exists() {
        fs::write(&target_json, "{}\n").map_err(|e| format!("init code json: {e}"))?;
    }
    let appjson = platform::code_app_json();
    match fs::symlink_metadata(&appjson) {
        Ok(m) if m.file_type().is_symlink() => {
            fs::remove_file(&appjson).map_err(|e| format!("unlink ~/.claude.json: {e}"))?;
        }
        Ok(_) => {
            backup_file(&appjson, "preswitch-code")?;
            fs::remove_file(&appjson).ok();
        }
        Err(_) => {}
    }
    unix_fs::symlink(&target_json, &appjson)
        .map_err(|e| format!("symlink ~/.claude.json: {e}"))?;
    Ok(())
}

// Rename a profile's Claude Code dir + json to match a Desktop-profile rename,
// repointing the live symlinks when the renamed profile is active.
#[cfg(target_os = "linux")]
pub fn rename_code_profile(old: &str, new: &str, active: bool) -> Result<(), String> {
    use std::os::unix::fs as unix_fs;

    let code_old = code_profile_dir(old);
    let code_new = code_profile_dir(new);
    let json_old = code_profile_json(old);
    let json_new = code_profile_json(new);

    if active {
        let app = platform::code_app_dir();
        if app.is_symlink() {
            fs::remove_file(&app).map_err(|e| format!("unlink ~/.claude: {e}"))?;
        }
        if code_old.exists() {
            fs::rename(&code_old, &code_new).map_err(|e| format!("rename code dir: {e}"))?;
        } else {
            fs::create_dir_all(&code_new).map_err(|e| format!("mkdir code dir: {e}"))?;
        }
        unix_fs::symlink(&code_new, &app).map_err(|e| format!("relink ~/.claude: {e}"))?;

        let appjson = platform::code_app_json();
        if appjson.is_symlink() {
            fs::remove_file(&appjson).map_err(|e| format!("unlink ~/.claude.json: {e}"))?;
        }
        if json_old.exists() {
            fs::rename(&json_old, &json_new).map_err(|e| format!("rename code json: {e}"))?;
        }
        if !json_new.exists() {
            fs::write(&json_new, "{}\n").map_err(|e| format!("init code json: {e}"))?;
        }
        unix_fs::symlink(&json_new, &appjson)
            .map_err(|e| format!("relink ~/.claude.json: {e}"))?;
    } else {
        if code_old.exists() {
            fs::rename(&code_old, &code_new).map_err(|e| format!("rename code dir: {e}"))?;
        }
        if json_old.exists() {
            fs::rename(&json_old, &json_new).map_err(|e| format!("rename code json: {e}"))?;
        }
    }
    Ok(())
}
