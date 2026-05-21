use ksni::{
    MenuItem, Tray,
    blocking::TrayMethods,
    menu::{CheckmarkItem, StandardItem, SubMenu},
};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const CLAUDE_DIR_NAME: &str = "Claude";
const PROFILES_DIR_NAME: &str = "claude-profiles";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME not set")
}

fn claude_app_dir() -> PathBuf {
    home().join(".config").join(CLAUDE_DIR_NAME)
}

fn profiles_dir() -> PathBuf {
    home().join(".config").join(PROFILES_DIR_NAME)
}

fn list_profiles() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(profiles_dir()) {
        for entry in rd.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

fn current_profile() -> Option<String> {
    let link = claude_app_dir();
    let target = fs::read_link(&link).ok()?;
    let abs = if target.is_relative() {
        link.parent()?.join(target)
    } else {
        target
    };
    let canon = fs::canonicalize(&abs).ok()?;
    let pdir = fs::canonicalize(profiles_dir()).ok()?;
    let rel = canon.strip_prefix(&pdir).ok()?;
    rel.components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
}

fn is_claude_running() -> bool {
    Command::new("pgrep")
        .args(["-f", "[Cc]laude"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn kill_claude() {
    let _ = Command::new("pkill").args(["-x", "claude"]).status();
    let _ = Command::new("pkill").args(["-f", "Claude"]).status();
    std::thread::sleep(Duration::from_millis(600));
}

fn dir_is_empty(p: &std::path::Path) -> bool {
    fs::read_dir(p).map(|mut i| i.next().is_none()).unwrap_or(true)
}

fn switch_profile(name: &str) -> Result<(), String> {
    let profiles = profiles_dir();
    fs::create_dir_all(&profiles).map_err(|e| format!("mkdir profiles: {e}"))?;
    let target = profiles.join(name);

    kill_claude();

    let app_dir = claude_app_dir();
    match fs::symlink_metadata(&app_dir) {
        Ok(m) if m.file_type().is_symlink() => {
            fs::remove_file(&app_dir).map_err(|e| format!("unlink: {e}"))?;
            fs::create_dir_all(&target).map_err(|e| format!("mkdir target: {e}"))?;
        }
        Ok(m) if m.is_dir() => {
            // Capture config as this profile, or back-up.
            let target_missing = !target.exists();
            let target_empty = target.exists() && dir_is_empty(&target);
            if target_missing {
                fs::rename(&app_dir, &target)
                    .map_err(|e| format!("migrate config -> profile: {e}"))?;
            } else if target_empty {
                fs::remove_dir(&target).ok();
                fs::rename(&app_dir, &target)
                    .map_err(|e| format!("migrate config -> profile: {e}"))?;
            } else {
                backup_dir(&app_dir, "preswitch")?;
            }
        }
        Ok(_) => {
            fs::remove_file(&app_dir).ok();
            fs::create_dir_all(&target).map_err(|e| format!("mkdir target: {e}"))?;
        }
        Err(_) => {
            fs::create_dir_all(&target).map_err(|e| format!("mkdir target: {e}"))?;
        }
    }

    unix_fs::symlink(&target, &app_dir).map_err(|e| format!("symlink: {e}"))?;
    Ok(())
}

const MCP_CONFIG_FILE: &str = "claude_desktop_config.json";
const BACKUPS_DIR_NAME: &str = ".backups";

fn backups_dir() -> PathBuf {
    profiles_dir().join(BACKUPS_DIR_NAME)
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

// Copy a file to the backups dir before overwrite
fn backup_file(path: &Path, label: &str) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let suffix = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let dest = backup_path(label, suffix)?;
    fs::copy(path, &dest).map_err(|e| format!("backup copy: {e}"))?;
    Ok(Some(dest))
}

// Move directory into the backup directory
fn backup_dir(path: &Path, label: &str) -> Result<PathBuf, String> {
    let suffix = path.file_name().and_then(|n| n.to_str()).unwrap_or("dir");
    let dest = backup_path(label, suffix)?;
    fs::rename(path, &dest).map_err(|e| format!("backup dir: {e}"))?;
    Ok(dest)
}


fn current_config_path() -> PathBuf {
    // Goes through the symlink, so it resolves to whichever profile is active.
    claude_app_dir().join(MCP_CONFIG_FILE)
}

fn profile_config_path(profile: &str) -> PathBuf {
    profiles_dir().join(profile).join(MCP_CONFIG_FILE)
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_json(path: &Path, v: &Value) -> Result<(), String> {
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

fn list_mcp_servers(path: &Path) -> Vec<String> {
    let v = read_json(path);
    let mut names: Vec<String> = v
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

fn import_mcp_servers(from: &Path, to: &Path, names: &[String]) -> Result<usize, String> {
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

fn clear_mcp_servers(path: &Path) -> Result<(), String> {
    let mut v = read_json(path);
    if !v.is_object() {
        v = json!({});
    }
    if let Some(obj) = v.as_object_mut() {
        obj.insert("mcpServers".into(), json!({}));
    }
    write_json(path, &v)
}

fn pick_servers_kdialog(source_label: &str, servers: &[String]) -> Option<Vec<String>> {
    if servers.is_empty() {
        notify(&format!("'{source_label}' has no MCP servers"));
        return None;
    }
    let mut cmd = Command::new("kdialog");
    cmd.args([
        "--title",
        "Claude Switcher",
        "--separate-output",
        "--checklist",
        &format!("Import from '{source_label}' (same-named servers will be overwritten):"),
    ]);
    for s in servers {
        cmd.arg(s).arg(s).arg("off");
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if names.is_empty() { None } else { Some(names) }
}

fn after_config_change() {
    if is_claude_running() && confirm("Claude is running. Restart it now to load the new MCP config?") {
        kill_claude();
        relaunch_claude();
    }
}

fn prompt_new_profile() -> Option<String> {
    let out = Command::new("kdialog")
        .args(["--title", "Claude Switcher", "--inputbox", "New profile name:"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return None;
    }
    Some(name)
}

fn prompt_rename(old: &str) -> Option<String> {
    let out = Command::new("kdialog")
        .args([
            "--title",
            "Rename profile",
            "--inputbox",
            &format!("Rename '{old}' to:"),
            old,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() || name == old || name.contains('/') || name.starts_with('.') {
        return None;
    }
    Some(name)
}

fn confirm(question: &str) -> bool {
    Command::new("kdialog")
        .args(["--title", "Claude Switcher", "--yesno", question])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn notify(msg: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "Claude Switcher", "Claude Switcher", msg])
        .status();
}

fn relaunch_claude() {
    let _ = Command::new("sh")
        .arg("-c")
        .arg("(setsid claude-desktop >/dev/null 2>&1 < /dev/null &) \
              || (setsid Claude       >/dev/null 2>&1 < /dev/null &) \
              || (setsid claude       >/dev/null 2>&1 < /dev/null &)")
        .status();
}

struct App {
    relaunch_after_switch: bool,
}

impl App {
    fn do_rename(&self, old: &str, new: &str) -> Result<(), String> {
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
        let was_running = is_claude_running();

        if is_active {
            if was_running {
                if !confirm(&format!(
                    "Claude is running and must close to rename the active profile '{old}'. Continue?"
                )) {
                    return Err("cancelled".into());
                }
                kill_claude();
            }
            // Drop the symlink, rename the underlying dir, point the symlink at the new name.
            let app_dir = claude_app_dir();
            if app_dir.is_symlink() {
                fs::remove_file(&app_dir).map_err(|e| format!("unlink: {e}"))?;
            }
            fs::rename(&old_path, &new_path).map_err(|e| format!("rename: {e}"))?;
            unix_fs::symlink(&new_path, &app_dir).map_err(|e| format!("relink: {e}"))?;
            if was_running && self.relaunch_after_switch {
                relaunch_claude();
            }
        } else {
            fs::rename(&old_path, &new_path).map_err(|e| format!("rename: {e}"))?;
        }
        Ok(())
    }

    fn do_switch(&self, name: &str) {
        if is_claude_running()
            && !confirm("Claude is running and will be closed to switch profile. Continue?")
        {
            return;
        }
        match switch_profile(name) {
            Ok(()) => {
                notify(&format!("Switched to '{name}'"));
                if self.relaunch_after_switch {
                    relaunch_claude();
                }
            }
            Err(e) => notify(&format!("Switch failed: {e}")),
        }
    }
}

impl App {
    fn mcp_submenu(&self, current_profile_name: &Option<String>) -> MenuItem<Self> {
        let current_path = current_config_path();
        let current_servers = list_mcp_servers(&current_path);
        let all_profiles = list_profiles();
        let other_profiles: Vec<String> = all_profiles
            .into_iter()
            .filter(|p| Some(p) != current_profile_name.as_ref())
            .collect();

        let mut sub: Vec<MenuItem<Self>> = Vec::new();
        sub.push(
            StandardItem {
                label: format!("Current servers: {}", current_servers.len()),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );
        if current_servers.is_empty() {
            sub.push(
                StandardItem {
                    label: "    (none)".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            for name in &current_servers {
                sub.push(
                    StandardItem {
                        label: format!("    • {name}"),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
            }
        }

        sub.push(MenuItem::Separator);

        let import_items: Vec<MenuItem<Self>> = if other_profiles.is_empty() {
            vec![
                StandardItem {
                    label: "(no other profiles)".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            ]
        } else {
            other_profiles
                .into_iter()
                .map(|p| {
                    let label = p.clone();
                    StandardItem {
                        label: p.clone(),
                        activate: Box::new(move |_this: &mut Self| {
                            let src = profile_config_path(&label);
                            let servers = list_mcp_servers(&src);
                            if let Some(picks) = pick_servers_kdialog(&label, &servers) {
                                match import_mcp_servers(&src, &current_config_path(), &picks) {
                                    Ok(n) => {
                                        notify(&format!(
                                            "Imported {n} server(s) from '{label}'"
                                        ));
                                        after_config_change();
                                    }
                                    Err(e) => notify(&format!("Import failed: {e}")),
                                }
                            }
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect()
        };

        sub.push(
            SubMenu {
                label: "Import from".into(),
                submenu: import_items,
                ..Default::default()
            }
            .into(),
        );

        sub.push(MenuItem::Separator);
        sub.push(
            StandardItem {
                label: "Edit config…".into(),
                activate: Box::new(|_| {
                    let _ = Command::new("xdg-open").arg(current_config_path()).status();
                }),
                ..Default::default()
            }
            .into(),
        );
        let backup_count = fs::read_dir(backups_dir())
            .map(|rd| rd.flatten().count())
            .unwrap_or(0);
        sub.push(
            StandardItem {
                label: format!("Open backups folder ({backup_count})"),
                enabled: backup_count > 0,
                activate: Box::new(|_| {
                    let _ = Command::new("xdg-open").arg(backups_dir()).status();
                }),
                ..Default::default()
            }
            .into(),
        );
        sub.push(
            StandardItem {
                label: "Clear all MCP servers".into(),
                enabled: !current_servers.is_empty(),
                activate: Box::new(|_| {
                    if !confirm("Remove all MCP servers from the active profile?") {
                        return;
                    }
                    match clear_mcp_servers(&current_config_path()) {
                        Ok(()) => {
                            notify("Cleared MCP servers");
                            after_config_change();
                        }
                        Err(e) => notify(&format!("Clear failed: {e}")),
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        SubMenu {
            label: "MCP servers".into(),
            submenu: sub,
            ..Default::default()
        }
        .into()
    }
}

impl Tray for App {
    fn id(&self) -> String {
        "claude-switcher".into()
    }
    fn title(&self) -> String {
        "Claude Switcher".into()
    }
    fn icon_name(&self) -> String {
        "system-switch-user".into()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let cur = current_profile().unwrap_or_else(|| "(unmanaged)".into());
        ksni::ToolTip {
            title: "Claude Switcher".into(),
            description: format!("Active profile: {cur}"),
            icon_name: "system-switch-user".into(),
            icon_pixmap: vec![],
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let current = current_profile();
        let profiles = list_profiles();
        let mut items: Vec<MenuItem<Self>> = Vec::new();

        let header = match &current {
            Some(p) => format!("Active: {p}"),
            None => "Active: (unmanaged)".into(),
        };
        items.push(
            StandardItem {
                label: header,
                enabled: false,
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);

        if profiles.is_empty() {
            items.push(
                StandardItem {
                    label: "No profiles yet — use “New profile…”".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            for p in &profiles {
                let is_cur = Some(p) == current.as_ref();
                let label = if is_cur {
                    format!("● {p}")
                } else {
                    format!("    {p}")
                };
                let name = p.clone();
                items.push(
                    StandardItem {
                        label,
                        enabled: !is_cur,
                        activate: Box::new(move |this: &mut Self| this.do_switch(&name)),
                        ..Default::default()
                    }
                    .into(),
                );
            }
        }

        items.push(MenuItem::Separator);
        items.push(self.mcp_submenu(&current));

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "New profile…".into(),
                activate: Box::new(|this: &mut Self| {
                    if let Some(name) = prompt_new_profile() {
                        this.do_switch(&name);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        let rename_items: Vec<MenuItem<Self>> = if profiles.is_empty() {
            vec![
                StandardItem {
                    label: "(no profiles)".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            ]
        } else {
            profiles
                .iter()
                .map(|p| {
                    let old = p.clone();
                    StandardItem {
                        label: p.clone(),
                        activate: Box::new(move |this: &mut Self| {
                            if let Some(new) = prompt_rename(&old) {
                                match this.do_rename(&old, &new) {
                                    Ok(()) => {
                                        notify(&format!("Renamed '{old}' → '{new}'"));
                                    }
                                    Err(e) => notify(&format!("Rename failed: {e}")),
                                }
                            }
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect()
        };
        items.push(
            SubMenu {
                label: "Rename profile".into(),
                submenu: rename_items,
                enabled: !profiles.is_empty(),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            CheckmarkItem {
                label: "Relaunch Claude after switch".into(),
                checked: self.relaunch_after_switch,
                activate: Box::new(|this: &mut Self| {
                    this.relaunch_after_switch = !this.relaunch_after_switch;
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            StandardItem {
                label: "Open profiles folder".into(),
                activate: Box::new(|_| {
                    let _ = Command::new("xdg-open").arg(profiles_dir()).status();
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

fn main() {
    let handle = App {
        relaunch_after_switch: true,
    }
    .spawn()
    .expect("register tray");

    // Refresh the menu so the active-profile marker tracks profile changes
    loop {
        std::thread::sleep(Duration::from_secs(3));
        handle.update(|_| {});
    }
}
