use crate::platform;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const CLAUDE_DIR_NAME: &str = "Claude";
const MCP_CONFIG_FILE: &str = "claude_desktop_config.json";
const BACKUPS_DIR_NAME: &str = ".backups";

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

fn persist_live_at(app_dir: &Path, profile_path: &Path) -> Result<(), String> {
    if profile_path.exists() {
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
        if was_running && relaunch_after_switch {
            platform::relaunch_claude();
        }
    } else {
        fs::rename(&old_path, &new_path).map_err(|e| format!("rename: {e}"))?;
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
        && !platform::confirm("Claude is running and will be closed to switch profile. Continue?")
    {
        return;
    }
    match switch_profile(name) {
        Ok(()) => {
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

pub fn active_tooltip() -> String {
    let cur = current_profile().unwrap_or_else(|| "(unmanaged)".into());
    format!("Active profile: {cur}")
}
