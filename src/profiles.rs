use crate::platform;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const CLAUDE_DIR_NAME: &str = "Claude";
const MCP_CONFIG_FILE: &str = "claude_desktop_config.json";
const BACKUPS_DIR_NAME: &str = ".backups";

#[cfg(target_os = "linux")]
const PROJECTS_DIR_NAME: &str = "projects";
#[cfg(target_os = "linux")]
const MEMORY_DIR_NAME: &str = "memory";
#[cfg(target_os = "linux")]
const MEMORY_INDEX_FILE: &str = "MEMORY.md";
#[cfg(target_os = "linux")]
const SKILLS_DIR_NAME: &str = "skills";
#[cfg(target_os = "linux")]
const IMPORT_LOG_FILE: &str = ".imports.log";
#[cfg(target_os = "linux")]
const COPY_MAX_DEPTH: usize = 32;
#[cfg(target_os = "linux")]
const SCAN_MAX_DEPTH: usize = 24;
// A Desktop profile's Electron caches run to tens of thousands of entries.
#[cfg(target_os = "linux")]
const SCAN_MAX_ENTRIES: usize = 200_000;

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

pub fn active_tooltip() -> String {
    let desktop = current_profile().unwrap_or_else(|| "(unmanaged)".into());
    #[cfg(target_os = "linux")]
    let out = {
        let code = current_code_profile().unwrap_or_else(|| "unmanaged".into());
        format!("Desktop: {desktop}  •  Code: {code}")
    };
    #[cfg(not(target_os = "linux"))]
    let out = format!("Desktop: {desktop}");
    out
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

// ── Cross-profile memory / skills import (Linux only) ────────────────────────
//
// Profiles are isolated by the symlink swap: nothing in an inactive profile sits
// on a path Claude reads. These functions are the only bridge across that line.
// They run only from an explicit menu pick, transfer only the entries checked in
// the dialog, and always write *content copies* — never links — so an import is
// a snapshot the source profile can no longer influence.

#[cfg(target_os = "linux")]
fn code_projects_dir(profile: &str) -> PathBuf {
    code_profile_dir(profile).join(PROJECTS_DIR_NAME)
}

#[cfg(target_os = "linux")]
fn code_memory_dir(profile: &str, project: &str) -> PathBuf {
    code_projects_dir(profile).join(project).join(MEMORY_DIR_NAME)
}

#[cfg(target_os = "linux")]
fn code_skills_dir(profile: &str) -> PathBuf {
    code_profile_dir(profile).join(SKILLS_DIR_NAME)
}

// Rejects names that would escape the directory they were listed from.
#[cfg(target_os = "linux")]
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

#[cfg(target_os = "linux")]
fn entry_names(dir: &Path, keep: impl Fn(&Path, &str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !is_plain_name(&name) {
                continue;
            }
            if keep(&e.path(), &name) {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

#[cfg(target_os = "linux")]
pub fn list_memory_files(profile: &str, project: &str) -> Vec<String> {
    entry_names(&code_memory_dir(profile, project), |p, name| {
        name != MEMORY_INDEX_FILE && name.ends_with(".md") && p.is_file()
    })
}

#[cfg(target_os = "linux")]
pub fn list_memory_projects(profile: &str) -> Vec<String> {
    entry_names(&code_projects_dir(profile), |p, _| {
        p.join(MEMORY_DIR_NAME).is_dir()
    })
    .into_iter()
    .filter(|project| !list_memory_files(profile, project).is_empty())
    .collect()
}

#[cfg(target_os = "linux")]
pub fn list_code_skills(profile: &str) -> Vec<String> {
    entry_names(&code_skills_dir(profile), |_, _| true)
}

// Profiles that can be imported from: Desktop and Claude Code profile dirs,
// minus `exclude` (the active one).
#[cfg(target_os = "linux")]
pub fn import_sources(exclude: Option<&str>) -> Vec<String> {
    let mut out = list_profiles();
    out.extend(entry_names(&platform::code_profiles_dir(), |p, _| p.is_dir()));
    out.sort();
    out.dedup();
    out.retain(|p| Some(p.as_str()) != exclude);
    out
}

// Claude Code project keys are the project path with every '/' turned into '-'.
#[cfg(target_os = "linux")]
pub fn project_label(key: &str) -> String {
    let home_key = platform::home().to_string_lossy().replace('/', "-");
    match key.strip_prefix(&home_key) {
        Some(rest) => format!("~{}", rest.replacen('-', "/", 1)),
        None => key.to_string(),
    }
}

#[cfg(target_os = "linux")]
fn write_text(path: &Path, s: &str) -> Result<(), String> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, s).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
}

// The source index's pointer line for `file`, else one built from its frontmatter.
#[cfg(target_os = "linux")]
fn index_line_for(src_index: &str, file: &str, body: &str) -> String {
    let needle = format!("]({file})");
    if let Some(line) = src_index.lines().find(|l| l.contains(&needle)) {
        return line.trim_end().to_string();
    }
    let mut title = file.trim_end_matches(".md").to_string();
    let mut hook = String::new();
    for line in body.lines().take(12) {
        if let Some(v) = line.strip_prefix("name:") {
            title = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("description:") {
            hook = v.trim().to_string();
        }
    }
    if hook.is_empty() {
        format!("- [{title}]({file})")
    } else {
        format!("- [{title}]({file}) — {hook}")
    }
}

// Append pointer lines for the imported files to the destination MEMORY.md,
// leaving lines already there untouched.
#[cfg(target_os = "linux")]
fn merge_memory_index(dst_dir: &Path, src_dir: &Path, files: &[String]) -> Result<(), String> {
    let src_index = fs::read_to_string(src_dir.join(MEMORY_INDEX_FILE)).unwrap_or_default();
    let dst_path = dst_dir.join(MEMORY_INDEX_FILE);
    let mut dst = fs::read_to_string(&dst_path).unwrap_or_default();

    let added: Vec<String> = files
        .iter()
        .filter(|f| !dst.contains(&format!("]({f})")))
        .map(|f| {
            let body = fs::read_to_string(dst_dir.join(f)).unwrap_or_default();
            index_line_for(&src_index, f, &body)
        })
        .collect();
    if added.is_empty() {
        return Ok(());
    }

    backup_file(&dst_path, "memory-import")?;
    if dst.trim().is_empty() {
        dst = "# Memory\n\n".into();
    } else if !dst.ends_with('\n') {
        dst.push('\n');
    }
    dst.push_str(&added.join("\n"));
    dst.push('\n');
    write_text(&dst_path, &dst)
}

// Content copy. Symlinks are dereferenced so an imported entry never points back
// into the source profile or a library both profiles share; a dangling link is
// carried over verbatim.
#[cfg(target_os = "linux")]
fn copy_tree(src: &Path, dst: &Path, depth: usize) -> Result<(), String> {
    use std::os::unix::fs as unix_fs;

    if depth == 0 {
        return Err(format!("nesting too deep at {}", src.display()));
    }
    let meta = fs::symlink_metadata(src).map_err(|e| format!("stat {}: {e}", src.display()))?;
    if meta.file_type().is_symlink() {
        return match fs::canonicalize(src) {
            Ok(real) => copy_tree(&real, dst, depth - 1),
            Err(_) => {
                let target =
                    fs::read_link(src).map_err(|e| format!("readlink {}: {e}", src.display()))?;
                unix_fs::symlink(target, dst)
                    .map_err(|e| format!("symlink {}: {e}", dst.display()))
            }
        };
    }
    if meta.is_dir() {
        fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
        for e in fs::read_dir(src)
            .map_err(|e| format!("read {}: {e}", src.display()))?
            .flatten()
        {
            copy_tree(&e.path(), &dst.join(e.file_name()), depth - 1)?;
        }
        return Ok(());
    }
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("copy {}: {e}", src.display()))
}

// Back up whatever sits at `dst`, drop it, and put a content copy of `src` there.
// A link at `dst` is only unlinked — its target belongs to something else.
#[cfg(target_os = "linux")]
fn replace_with_copy(src: &Path, dst: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(dst) {
        Ok(m) if m.file_type().is_symlink() => {
            fs::remove_file(dst).map_err(|e| format!("unlink {}: {e}", dst.display()))?;
        }
        Ok(m) if m.is_dir() => {
            backup_dir(dst, label)?;
        }
        Ok(_) => {
            backup_file(dst, label)?;
            fs::remove_file(dst).ok();
        }
        Err(_) => {}
    }
    copy_tree(src, dst, COPY_MAX_DEPTH)
}

#[cfg(target_os = "linux")]
fn is_dangling(p: &Path) -> bool {
    fs::symlink_metadata(p)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
        && fs::canonicalize(p).is_err()
}

// Logged outside every profile dir, so the log is not on a path Claude reads.
#[cfg(target_os = "linux")]
fn log_import(source: &str, dest: &str, what: &str) {
    use std::io::Write;

    let path = platform::code_profiles_dir().join(IMPORT_LOG_FILE);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {source} -> {dest}: {what}", timestamp());
    }
}

// Copy the named memory files from `source`'s `project` into the same project key
// under the active profile, so recall keeps working for that working directory.
#[cfg(target_os = "linux")]
pub fn import_memory(source: &str, project: &str, files: &[String]) -> Result<usize, String> {
    let active = current_profile().ok_or("no active profile")?;
    if source == active {
        return Err("source is the active profile".into());
    }
    if !is_plain_name(source) || !is_plain_name(project) {
        return Err("bad source or project name".into());
    }
    let src_dir = code_memory_dir(source, project);
    let dst_dir = code_memory_dir(&active, project);
    fs::create_dir_all(&dst_dir).map_err(|e| format!("mkdir memory: {e}"))?;

    let mut copied = Vec::new();
    for f in files {
        if !is_plain_name(f) {
            continue;
        }
        let src = src_dir.join(f);
        if !src.is_file() {
            continue;
        }
        replace_with_copy(&src, &dst_dir.join(f), "memory-import")?;
        copied.push(f.clone());
    }
    if !copied.is_empty() {
        merge_memory_index(&dst_dir, &src_dir, &copied)?;
        log_import(
            source,
            &active,
            &format!("memory {project}: {}", copied.join(" ")),
        );
    }
    Ok(copied.len())
}

// Returns (copied, dangling-links-copied-as-is).
#[cfg(target_os = "linux")]
pub fn import_skills(source: &str, names: &[String]) -> Result<(usize, usize), String> {
    let active = current_profile().ok_or("no active profile")?;
    if source == active {
        return Err("source is the active profile".into());
    }
    if !is_plain_name(source) {
        return Err("bad source name".into());
    }
    let src_dir = code_skills_dir(source);
    let dst_dir = code_skills_dir(&active);
    fs::create_dir_all(&dst_dir).map_err(|e| format!("mkdir skills: {e}"))?;

    let mut copied = Vec::new();
    let mut dangling = 0;
    for n in names {
        if !is_plain_name(n) {
            continue;
        }
        let src = src_dir.join(n);
        if fs::symlink_metadata(&src).is_err() {
            continue;
        }
        if is_dangling(&src) {
            dangling += 1;
        }
        replace_with_copy(&src, &dst_dir.join(n), "skill-import")?;
        copied.push(n.clone());
    }
    if !copied.is_empty() {
        log_import(source, &active, &format!("skills: {}", copied.join(" ")));
    }
    Ok((copied.len(), dangling))
}

// ── Isolation audit (Linux only) ─────────────────────────────────────────────

// Name of the profile `target` lives under, if any.
#[cfg(target_os = "linux")]
fn owning_profile(target: &Path) -> Option<String> {
    for base in [platform::code_profiles_dir(), profiles_dir()] {
        let Ok(base) = fs::canonicalize(&base) else {
            continue;
        };
        if let Ok(rel) = target.strip_prefix(&base) {
            let first = rel.components().next()?;
            let name = first.as_os_str().to_string_lossy();
            return Some(name.trim_end_matches(".json").to_string());
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn check_live_link(path: &Path, label: &str, active: &str, out: &mut Vec<String>) {
    match platform::read_link_target(path) {
        Some(target) => match owning_profile(&target) {
            Some(name) if name == active => {}
            Some(name) => out.push(format!("{label} resolves to profile '{name}', not '{active}'")),
            None => out.push(format!(
                "{label} resolves outside the profiles tree: {}",
                target.display()
            )),
        },
        None if path.exists() => out.push(format!("{label} is real state, not a link — unmanaged")),
        None => out.push(format!("{label} is missing")),
    }
}

#[cfg(target_os = "linux")]
fn scan_escapes(dir: &Path, active: &str, out: &mut Vec<String>, depth: usize, budget: &mut usize) {
    if depth == 0 || *budget == 0 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let p = e.path();
        let Ok(m) = fs::symlink_metadata(&p) else {
            continue;
        };
        if m.file_type().is_symlink() {
            if let Some(name) = platform::read_link_target(&p).as_deref().and_then(owning_profile)
                && name != active
            {
                out.push(format!("{} reaches into profile '{name}'", p.display()));
            }
        } else if m.is_dir() {
            scan_escapes(&p, active, out, depth - 1, budget);
        }
    }
}

// Every way the active profile could still see another profile's data. Empty
// means isolated.
#[cfg(target_os = "linux")]
pub fn audit_isolation() -> Vec<String> {
    let Some(active) = current_profile() else {
        return vec!["No active profile — Claude's config is unmanaged.".into()];
    };
    let mut out = Vec::new();
    check_live_link(&claude_app_dir(), "~/.config/Claude", &active, &mut out);
    check_live_link(&platform::code_app_dir(), "~/.claude", &active, &mut out);
    check_live_link(&platform::code_app_json(), "~/.claude.json", &active, &mut out);

    let mut budget = SCAN_MAX_ENTRIES;
    for root in [code_profile_dir(&active), profiles_dir().join(&active)] {
        scan_escapes(&root, &active, &mut out, SCAN_MAX_DEPTH, &mut budget);
    }
    if budget == 0 {
        out.push(format!(
            "Scan stopped after {SCAN_MAX_ENTRIES} entries — result is partial."
        ));
    }
    out
}

// ── Menu actions (Linux only) ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub fn import_memory_from(source: &str, project: &str) {
    let files = list_memory_files(source, project);
    if files.is_empty() {
        platform::notify(&format!(
            "'{source}' has no memory for {}",
            project_label(project)
        ));
        return;
    }
    let Some(picks) = platform::pick_items(
        &format!(
            "Copy memory from '{source}' into the active profile for {}.\n\
             Only what you check is copied — the rest of '{source}' stays out of reach.",
            project_label(project)
        ),
        &files,
    ) else {
        return;
    };
    match import_memory(source, project, &picks) {
        Ok(n) => platform::notify(&format!(
            "Imported {n} memory file(s) from '{source}' — picked up by new Claude Code sessions"
        )),
        Err(e) => platform::notify(&format!("Memory import failed: {e}")),
    }
}

#[cfg(target_os = "linux")]
pub fn import_skills_from(source: &str) {
    let skills = list_code_skills(source);
    if skills.is_empty() {
        platform::notify(&format!("'{source}' has no skills"));
        return;
    }
    let Some(picks) = platform::pick_items(
        &format!(
            "Copy skills from '{source}' into the active profile.\n\
             Only what you check is copied — the rest of '{source}' stays out of reach."
        ),
        &skills,
    ) else {
        return;
    };
    match import_skills(source, &picks) {
        Ok((n, 0)) => platform::notify(&format!(
            "Imported {n} skill(s) from '{source}' — picked up by new Claude Code sessions"
        )),
        Ok((n, d)) => platform::notify(&format!(
            "Imported {n} skill(s) from '{source}' — {d} dangling symlink(s) copied as-is"
        )),
        Err(e) => platform::notify(&format!("Skill import failed: {e}")),
    }
}

#[cfg(target_os = "linux")]
pub fn show_isolation_report() {
    let findings = audit_isolation();
    let body = if findings.is_empty() {
        format!(
            "Isolation OK.\n\n\
             ~/.config/Claude, ~/.claude and ~/.claude.json all resolve inside the active \
             profile, and nothing inside it links into another profile.\n\n\
             Imports are content copies, logged to:\n{}",
            platform::code_profiles_dir().join(IMPORT_LOG_FILE).display()
        )
    } else {
        format!(
            "{} issue(s) found:\n\n• {}",
            findings.len(),
            findings.join("\n• ")
        )
    };
    platform::info("Claude Switcher — isolation", &body);
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs as unix_fs;

    const KEY: &str = "-home-user-proj";

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn is_symlink(path: &Path) -> bool {
        fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    // A two-profile install with 'Dst' active, matching the real layout.
    fn sandbox() -> PathBuf {
        let home = std::env::temp_dir().join(format!("claude-switcher-t{}", std::process::id()));
        fs::remove_dir_all(&home).ok();
        fs::create_dir_all(home.join(".config")).unwrap();
        unsafe { std::env::set_var("HOME", &home) };

        let desktop = home.join(".config").join("claude-profiles");
        let code = home.join(".config").join("claude-code-profiles");
        for p in ["Src", "Dst"] {
            fs::create_dir_all(desktop.join(p)).unwrap();
            fs::create_dir_all(code.join(p)).unwrap();
        }

        let src_mem = code.join("Src").join("projects").join(KEY).join("memory");
        write(
            &src_mem.join(MEMORY_INDEX_FILE),
            "# Memory\n\n- [Alpha](a.md) — alpha hook\n",
        );
        write(&src_mem.join("a.md"), "---\nname: alpha\n---\n\nalpha body\n");
        write(&src_mem.join("b.md"), "---\nname: beta\n---\n\nbeta body\n");

        let src_skills = code.join("Src").join(SKILLS_DIR_NAME);
        write(&src_skills.join("plain").join("SKILL.md"), "plain skill\n");
        write(&home.join("shared").join("lib").join("SKILL.md"), "shared skill\n");
        unix_fs::symlink(home.join("shared").join("lib"), src_skills.join("linked")).unwrap();
        unix_fs::symlink("../../nowhere", src_skills.join("broken")).unwrap();

        write(
            &code.join("Dst").join("projects").join(KEY).join("memory").join(MEMORY_INDEX_FILE),
            "# Memory\n\n- [Kept](kept.md) — pre-existing\n",
        );

        write(&code.join("Dst.json"), "{}\n");
        unix_fs::symlink(desktop.join("Dst"), home.join(".config").join("Claude")).unwrap();
        unix_fs::symlink(code.join("Dst"), home.join(".claude")).unwrap();
        unix_fs::symlink(code.join("Dst.json"), home.join(".claude.json")).unwrap();
        home
    }

    #[test]
    fn import_copies_only_what_was_picked_and_keeps_profiles_isolated() {
        let home = sandbox();
        let code = home.join(".config").join("claude-code-profiles");
        assert_eq!(current_profile().as_deref(), Some("Dst"));

        assert_eq!(list_memory_projects("Src"), vec![KEY.to_string()]);
        assert_eq!(list_memory_files("Src", KEY), vec!["a.md", "b.md"]);
        assert_eq!(import_sources(Some("Dst")), vec!["Src".to_string()]);
        let home_key = home.to_string_lossy().replace('/', "-");
        assert_eq!(project_label(&format!("{home_key}-proj")), "~/proj");
        assert_eq!(project_label("-home-user-proj"), "-home-user-proj");

        assert_eq!(import_memory("Src", KEY, &["a.md".into()]).unwrap(), 1);
        let dst_mem = code.join("Dst").join("projects").join(KEY).join("memory");
        assert_eq!(fs::read_to_string(dst_mem.join("a.md")).unwrap(), "---\nname: alpha\n---\n\nalpha body\n");
        assert!(!is_symlink(&dst_mem.join("a.md")), "import must be a copy, not a link");
        assert!(!dst_mem.join("b.md").exists(), "unpicked memory must not cross");

        let index = fs::read_to_string(dst_mem.join(MEMORY_INDEX_FILE)).unwrap();
        assert!(index.contains("- [Kept](kept.md) — pre-existing"));
        assert!(index.contains("- [Alpha](a.md) — alpha hook"));
        assert!(!index.contains("(b.md)"));

        // Re-importing the same file must not duplicate its index line.
        assert_eq!(import_memory("Src", KEY, &["a.md".into()]).unwrap(), 1);
        let index = fs::read_to_string(dst_mem.join(MEMORY_INDEX_FILE)).unwrap();
        assert_eq!(index.matches("(a.md)").count(), 1);

        let picks = ["plain".into(), "linked".into(), "broken".into()];
        assert_eq!(import_skills("Src", &picks).unwrap(), (3, 1));
        let dst_skills = code.join("Dst").join(SKILLS_DIR_NAME);
        assert_eq!(fs::read_to_string(dst_skills.join("plain").join("SKILL.md")).unwrap(), "plain skill\n");
        assert!(!is_symlink(&dst_skills.join("plain")));
        assert!(!is_symlink(&dst_skills.join("linked")), "a linked skill must be dereferenced");
        assert_eq!(fs::read_to_string(dst_skills.join("linked").join("SKILL.md")).unwrap(), "shared skill\n");
        assert!(is_symlink(&dst_skills.join("broken")), "a dangling link is carried over as-is");

        assert!(import_memory("Dst", KEY, &["a.md".into()]).is_err());
        assert_eq!(audit_isolation(), Vec::<String>::new());

        unix_fs::symlink(
            code.join("Src").join(SKILLS_DIR_NAME).join("plain"),
            dst_skills.join("leak"),
        )
        .unwrap();
        let findings = audit_isolation();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("reaches into profile 'Src'"));

        fs::remove_dir_all(&home).ok();
    }
}
