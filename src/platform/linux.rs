use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

pub fn code_app_dir() -> PathBuf {
    home().join(".claude")
}

pub fn code_app_json() -> PathBuf {
    home().join(".claude.json")
}

pub fn code_profiles_dir() -> PathBuf {
    home().join(".config").join("claude-code-profiles")
}

pub fn profile_desktop_dir(name: &str) -> PathBuf {
    profiles_dir().join(name)
}

pub fn profile_code_dir(name: &str) -> PathBuf {
    code_profiles_dir().join(name)
}

fn profile_user_data_arg(name: &str) -> String {
    format!("--user-data-dir={}", profile_desktop_dir(name).display())
}

pub fn is_profile_desktop_running(name: &str) -> bool {
    pgrep_hit(&["-f", "--", &profile_user_data_arg(name)])
}

pub fn is_default_desktop_running() -> bool {
    is_desktop_running() && !any_named_profile_desktop_running()
}

fn any_named_profile_desktop_running() -> bool {
    let Ok(rd) = fs::read_dir(profiles_dir()) else {
        return false;
    };
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if is_profile_desktop_running(name) {
            return true;
        }
    }
    false
}

// First Claude Desktop launcher found on PATH.
fn desktop_binary() -> Option<&'static str> {
    ["claude-desktop", "Claude"].into_iter().find(|bin| {
        Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {bin}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

pub fn launch_profile_instance(name: &str) -> Result<(), String> {
    let desktop = profile_desktop_dir(name);
    let code = profile_code_dir(name);
    fs::create_dir_all(&desktop).map_err(|e| format!("mkdir desktop profile: {e}"))?;
    fs::create_dir_all(&code).map_err(|e| format!("mkdir code profile: {e}"))?;

    let bin = desktop_binary().ok_or("claude-desktop not found in PATH")?;
    // Spawn without a shell: avoids quoting bugs (profile names may contain
    // spaces) and passes the Electron switches directly. No `--` separator —
    // after `--` Electron treats `--user-data-dir` as a positional arg, which
    // would silently defeat per-profile isolation. `setsid --fork` reparents the
    // app to init and exits immediately, so the window outlives the tray and the
    // wait() below returns at once (just reaping setsid, leaving no zombie).
    let mut child = Command::new("setsid")
        .arg("--fork")
        .arg(bin)
        .arg(format!("--class=Claude-{name}"))
        .arg(format!("--user-data-dir={}", desktop.display()))
        .env("CLAUDE_CONFIG_DIR", &code)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("launch: {e}"))?;
    let _ = child.wait();
    Ok(())
}

pub fn kill_profile_instance(name: &str) {
    let udd = profile_user_data_arg(name);
    let _ = Command::new("pkill").args(["-f", "--", &udd]).status();
    let class = format!("--class=Claude-{name}");
    let _ = Command::new("pkill").args(["-f", "--", &class]).status();
    std::thread::sleep(Duration::from_millis(300));
}

pub fn kill_all_profile_instances() {
    let Ok(rd) = fs::read_dir(profiles_dir()) else {
        return;
    };
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        kill_profile_instance(name);
    }
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

fn pgrep_hit(args: &[&str]) -> bool {
    Command::new("pgrep")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// Match the Electron WM class flag, not the substring "Claude" (which would
// also catch this switcher, editors, or shell sessions in a Claude-named path).
pub fn is_desktop_running() -> bool {
    pgrep_hit(&["-f", "--", "--class=Claude"])
}

// Claude Code CLI / Desktop's embedded agent — matched by exact process name.
pub fn is_code_running() -> bool {
    pgrep_hit(&["-x", "claude"])
}

pub fn is_claude_running() -> bool {
    is_desktop_running() || is_code_running()
}

pub fn kill_claude() {
    let _ = Command::new("pkill").args(["-f", "--", "--class=Claude"]).status();
    let udd = format!("--user-data-dir={}", claude_app_dir().display());
    let _ = Command::new("pkill").args(["-f", "--", &udd]).status();
    let _ = Command::new("pkill").args(["-f", "claude-desktop"]).status();
    let _ = Command::new("pkill").args(["-x", "claude"]).status();
    std::thread::sleep(Duration::from_millis(800));
}

pub fn relaunch_claude() {
    let _ = Command::new("sh")
        .arg("-c")
        .arg("(setsid claude-desktop >/dev/null 2>&1 < /dev/null &) \
              || (setsid Claude       >/dev/null 2>&1 < /dev/null &)")
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
