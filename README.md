# Claude Switcher

A system tray app for switching between multiple Claude Desktop profiles on **Linux** and **Windows**.

Each profile is a separate Claude config directory swapped in via symlink (Linux) or directory junction (Windows) — sessions, MCP configs, settings, and history stay isolated per profile.

## Requirements

### Linux

- KDE Plasma (Wayland or X11). Any DE that supports the StatusNotifierItem spec will work.
- `kdialog`, `notify-send`, `pkill`, `xdg-open` (preinstalled on Plasma).
- Rust toolchain (only for building).

### Windows

- Windows 10 or later.
- Claude Desktop installed.
- Rust toolchain (only for building).

## Install

```bash
cargo install --path .
```

### Linux autostart

```bash
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/claude-switcher.desktop <<EOF
[Desktop Entry]
Type=Application
Name=Claude Switcher
Exec=$HOME/.cargo/bin/claude_switcher
Icon=system-switch-user
Terminal=false
X-KDE-autostart-after=panel
EOF
```

### Windows autostart

Press `Win+R`, type `shell:startup`, and create a shortcut to `claude_switcher.exe` (`%USERPROFILE%\.cargo\bin\claude_switcher.exe` if you use `cargo install --path .`).

## Run

**Linux:**

```bash
nohup ~/.cargo/bin/claude_switcher >/dev/null 2>&1 & disown
```

**Windows:**

```powershell
Start-Process "$env:USERPROFILE\.cargo\bin\claude_switcher.exe"
```

A tray icon appears in the system tray / notification area.

## Usage

Click the tray icon:

- **Active: \<name\>** — current profile (read-only header).
- **\<profile names\>** — click to switch. The active one is marked `●` and disabled.
- **MCP servers ▸**
  - Lists the current profile's MCP servers.
  - **Import from ▸ \<profile\>** — on Linux, a checklist dialog; on Windows, import all or pick individual servers from a submenu. Selected entries merge into the current config (same-named entries overwrite, but the old config is backed up first).
  - **Edit config…** — opens `claude_desktop_config.json` in your default editor.
  - **Open backups folder (N)** — opens the backups directory.
  - **Clear all MCP servers** — wipes the `mcpServers` map (backed up first).
- **New profile…** — prompts for a name. On first use, this migrates your existing Claude config into a profile.
- **Rename profile ▸ \<profile\>** — prompts for a new name. Renaming the active profile closes Claude first (with confirmation) and re-points the link; inactive profiles rename in-place without touching Claude.
- **Relaunch Claude after switch** — toggle. When on, the app is restarted after switching/importing.
- **Open profiles folder** — opens the profiles directory.
- **Quit**.

If Claude is running, you get a confirmation before it's killed.

## How it works

**Linux:**

```
~/.config/Claude               → symlink to active profile
~/.config/claude-profiles/
    personal/                  → real Claude config dir
    work/
    .backups/                  → timestamped pre-write copies
```

**Windows:**

```
%APPDATA%\Claude               → junction to active profile
%APPDATA%\claude-profiles\
    personal\                  → real Claude config dir
    work\
    .backups\                  → timestamped pre-write copies
```

On Windows, if `%APPDATA%\Claude` does not exist yet (some MSIX installs use the package path instead), the switcher falls back to `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude`.

Each profile is a self-contained Electron `userData` directory.

## Backups

Every operation that could lose data writes a backup to `.backups/` first:

| Operation | What's saved |
|---|---|
| Import MCP servers | previous `claude_desktop_config.json` |
| Clear MCP servers | previous `claude_desktop_config.json` |
| Edit config… | atomic write (`.tmp` + rename) — no partial writes |
| Switch profile (live dir conflicts with target) | live dir moved to `.backups/<ts>_preswitch_Claude/` |

Backups are timestamped (`<unix-ts>_<profile>_<filename>`) and never auto-pruned. To restore, copy a backup back into the profile dir.

## Notes

- Closing Claude before switching is required — Electron locks its config dir. The switcher kills it automatically (after confirmation if it was running).
- Profile dirs are real directories on disk; the active one is reached via the symlink/junction. Manual edits to either are safe.
- If a Claude auto-update ever replaces the link with a real directory, the next switch will treat it as unmanaged state and back it up before re-linking.
- On Windows, MCP import uses confirm dialogs and per-server submenu items instead of KDE's checklist dialog.
