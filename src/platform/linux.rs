use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME not set")
}

pub fn claude_app_dir() -> PathBuf {
    home().join(".config").join("Claude")
}

pub fn profiles_dir() -> PathBuf {
    home().join(".config").join("claude-profiles")
}

pub fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

pub fn remove_link(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|e| format!("unlink: {e}"))
}

pub fn create_link(target: &Path, link: &Path) -> Result<(), String> {
    unix_fs::symlink(target, link).map_err(|e| format!("symlink: {e}"))
}

pub fn read_link_target(link: &Path) -> Option<PathBuf> {
    let target = fs::read_link(link).ok()?;
    let abs = if target.is_relative() {
        link.parent()?.join(target)
    } else {
        target
    };
    fs::canonicalize(&abs).ok()
}

pub fn is_claude_running() -> bool {
    Command::new("pgrep")
        .args(["-f", "[Cc]laude"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn kill_claude() {
    let _ = Command::new("pkill").args(["-x", "claude"]).status();
    let _ = Command::new("pkill").args(["-f", "Claude"]).status();
    std::thread::sleep(Duration::from_millis(600));
}

pub fn relaunch_claude() {
    let _ = Command::new("sh")
        .arg("-c")
        .arg("(setsid claude-desktop >/dev/null 2>&1 < /dev/null &) \
              || (setsid Claude       >/dev/null 2>&1 < /dev/null &) \
              || (setsid claude       >/dev/null 2>&1 < /dev/null &)")
        .status();
}

pub fn open_path(path: &Path) {
    let _ = Command::new("xdg-open").arg(path).status();
}

pub fn confirm(question: &str) -> bool {
    Command::new("kdialog")
        .args(["--title", "Claude Switcher", "--yesno", question])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn notify(msg: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "Claude Switcher", "Claude Switcher", msg])
        .status();
}

pub fn prompt_text(title: &str, prompt: &str, default: Option<&str>) -> Option<String> {
    let mut cmd = Command::new("kdialog");
    cmd.args(["--title", title, "--inputbox", prompt]);
    if let Some(d) = default {
        cmd.arg(d);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn pick_servers(source_label: &str, servers: &[String]) -> Option<Vec<String>> {
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
    if names.is_empty() {
        None
    } else {
        Some(names)
    }
}
