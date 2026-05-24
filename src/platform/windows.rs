use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .expect("USERPROFILE not set")
}

fn msix_package_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let packages = local.join("Packages");
    let entry = fs::read_dir(packages).ok()?.flatten().find(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with("Claude_")
    })?;
    Some(entry.path())
}

fn msix_claude_dir() -> Option<PathBuf> {
    Some(
        msix_package_dir()?
            .join("LocalCache")
            .join("Roaming")
            .join("Claude"),
    )
}

pub fn claude_app_dir() -> PathBuf {
    if let Some(msix) = msix_claude_dir() {
        return msix;
    }
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("Claude"))
        .unwrap_or_else(|| home().join("AppData").join("Roaming").join("Claude"))
}

pub fn profiles_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("claude-profiles"))
        .unwrap_or_else(|| home().join("AppData").join("Roaming").join("claude-profiles"))
}

pub fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

pub fn remove_link(path: &Path) -> Result<(), String> {
    fs::remove_dir(path).map_err(|e| format!("remove junction: {e}"))
}

pub fn create_link(target: &Path, link: &Path) -> Result<(), String> {
    junction::create(target, link).map_err(|e| format!("junction: {e}"))
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
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq claude.exe"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_ascii_lowercase()
                .contains("claude.exe")
        })
        .unwrap_or(false)
}

pub fn kill_claude() {
    let _ = Command::new("taskkill")
        .args(["/IM", "claude.exe", "/F"])
        .status();
    for _ in 0..30 {
        if !is_claude_running() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn relaunch_claude() {
    let ps = r#"
$app = Get-StartApps | Where-Object { $_.Name -eq 'Claude' } | Select-Object -First 1
if ($app) {
    Start-Process explorer.exe "shell:AppsFolder\$($app.AppID)"
    exit 0
}
$exe = Join-Path $env:LOCALAPPDATA 'Programs\Claude\Claude.exe'
if (Test-Path $exe) { Start-Process $exe; exit 0 }
Start-Process explorer.exe 'shell:AppsFolder\Claude_pzs8sxrjxfjjc!Claude'
"#;
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps])
        .status();
}

pub fn open_path(path: &Path) {
    let _ = Command::new("explorer").arg(path).status();
}

pub fn confirm(question: &str) -> bool {
    let ps = format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
$result = [System.Windows.Forms.MessageBox]::Show(
    '{}', 'Claude Switcher',
    [System.Windows.Forms.MessageBoxButtons]::YesNo,
    [System.Windows.Forms.MessageBoxIcon]::Question)
if ($result -eq 'Yes') {{ exit 0 }} else {{ exit 1 }}
"#,
        question.replace('\'', "''")
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn notify(msg: &str) {
    let ps = format!(
        r#"
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
$xml = [Windows.Data.Xml.Dom.XmlDocument]::new()
$xml.LoadXml(@"
<toast>
  <visual>
    <binding template="ToastText02">
      <text id="1">Claude Switcher</text>
      <text id="2">{}</text>
    </binding>
  </visual>
</toast>
"@)
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Claude Switcher').Show($toast)
"#,
        msg.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status();
}

pub fn prompt_text(title: &str, prompt: &str, default: Option<&str>) -> Option<String> {
    let default = default.unwrap_or("");
    let ps = format!(
        r#"
Add-Type -AssemblyName Microsoft.VisualBasic
$v = [Microsoft.VisualBasic.Interaction]::InputBox('{}', '{}', '{}')
if ([string]::IsNullOrWhiteSpace($v)) {{ exit 1 }}
Write-Output $v
"#,
        prompt.replace('\'', "''"),
        title.replace('\'', "''"),
        default.replace('\'', "''")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .output()
        .ok()?;
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
  if confirm(&format!(
        "Import all {} MCP server(s) from '{source_label}'?\n\n{}",
        servers.len(),
        servers.join("\n")
    )) {
        Some(servers.to_vec())
    } else {
        None
    }
}

pub fn pick_single_server(source_label: &str, server: &str) -> Option<Vec<String>> {
    if confirm(&format!("Import MCP server '{server}' from '{source_label}'?")) {
        Some(vec![server.to_string()])
    } else {
        None
    }
}
