# Claude Switcher

A KDE Plasma tray applet for switching between multiple Claude Desktop profiles on Linux.

Each profile is a separate `~/.config/Claude` directory swapped in via symlink — sessions, MCP configs, settings, and history stay isolated per profile.

## Requirements

- Linux + KDE Plasma (Wayland or X11). Any DE that supports the StatusNotifierItem spec will work.
- `kdialog`, `notify-send`, `pkill`, `xdg-open` (preinstalled on Plasma).
- Rust toolchain (only for building).

## Install

```bash
cargo install --path .
```

Autostart on login:

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

## Run

```bash
nohup ~/.cargo/bin/claude_switcher >/dev/null 2>&1 & disown
```

A "switch user" icon appears in the system tray.

## Usage

Click the tray icon:

- **Active: \<name\>** — current profile (read-only header).
- **\<profile names\>** — click to switch. The active one is marked `●` and disabled.
- **MCP servers ▸**
  - Lists the current profile's MCP servers.
  - **Import from ▸ \<profile\>** — checklist dialog of that profile's MCP servers. Selected entries merge into the current config (same-named entries overwrite, but the old config is backed up first).
  - **Edit config…** — opens `claude_desktop_config.json` in your default editor.
  - **Open backups folder (N)** — opens `~/.config/claude-profiles/.backups/`.
  - **Clear all MCP servers** — wipes the `mcpServers` map (backed up first).
- **New profile…** — prompts for a name. On first use, this migrates your existing `~/.config/Claude` into a profile.
- **Rename profile ▸ \<profile\>** — prompts for a new name. Renaming the active profile closes Claude first (with confirmation) and re-points the symlink; inactive profiles rename in-place without touching Claude.
- **Relaunch Claude after switch** — toggle. When on, the app is restarted after switching/importing.
- **Open profiles folder** — opens `~/.config/claude-profiles/`.
- **Quit**.

If Claude is running, you get a confirmation before it's killed.

## How it works

```
~/.config/Claude               → symlink to active profile
~/.config/claude-profiles/
    personal/                  → real Claude config dir
    work/
    .backups/                  → timestamped pre-write copies
```

Each profile is a self-contained Electron `userData` directory.

## Backups

Every operation that could lose data writes a backup to `~/.config/claude-profiles/.backups/` first:

| Operation | What's saved |
|---|---|
| Import MCP servers | previous `claude_desktop_config.json` |
| Clear MCP servers | previous `claude_desktop_config.json` |
| `Edit config…` | atomic write (`.tmp` + rename) — no partial writes |
| Switch profile (live dir conflicts with target) | live dir moved to `.backups/<ts>_preswitch_Claude/` |

Backups are timestamped (`<unix-ts>_<profile>_<filename>`) and never auto-pruned. To restore, copy a backup back into the profile dir.

## Notes

- Closing Claude before switching is required — Electron locks its config dir. The switcher kills it automatically (after confirmation if it was running).
- Profile dirs are real directories on disk; the active one is reached via the symlink at `~/.config/Claude`. Manual edits to either are safe.
- If a Claude auto-update ever replaces the symlink with a real directory, the next switch will treat it as unmanaged state and back it up before re-linking.
