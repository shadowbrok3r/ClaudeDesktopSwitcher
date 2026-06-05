# Claude Switcher

A system tray app for switching between multiple Claude Desktop profiles on **Linux** and **Windows**.

On Linux, each profile swaps **both** Claude products in lockstep via symlink, so work and personal stay fully isolated — neither account can read the other's sessions, settings, history, MCP servers, memories, or skills:

| Product | Live path | Swapped to |
|---|---|---|
| Claude Desktop (Electron) | `~/.config/Claude` | `~/.config/claude-profiles/<name>/` |
| Claude Code (CLI / Desktop's embedded agent) | `~/.claude` + `~/.claude.json` | `~/.config/claude-code-profiles/<name>/` + `<name>.json` |

> Claude Desktop and Claude Code keep entirely separate config trees. Switching only the Desktop dir would leave both accounts sharing the same logged-in Claude Code session, memories, skills, and MCP servers — so the switcher manages both.

On **Windows**, each profile is a separate Claude config directory swapped in via directory junction — sessions, MCP configs, settings, and history stay isolated per profile.

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

- **Active: \<name\>** — current Desktop profile (read-only header).
- **Claude Code: \<name\>** — read-only status. `✓` means `~/.claude` is linked to the same profile; `unmanaged` / `out of sync` means it isn't yet.
- **Link Claude Code → \<active\>** — only shown when Claude Code isn't linked to the active profile. Captures the live `~/.claude` into the active profile and links it, without switching Desktop. Use this once to adopt your current Claude Code config.
- **\<profile names\>** — in *switch* mode, click to switch (swaps Desktop **and** Claude Code). The active one is marked `●` and disabled.
- **Switch profiles (one at a time)** / **Multiple instances (side by side)** — choose how profiles run:
  - *Switch* (default): one profile at a time via symlinks; switching closes Claude.
  - *Multiple instances*: launch Personal, Work, or both side by side. Each instance gets its own Desktop `userData` dir and (on Linux) its own `CLAUDE_CONFIG_DIR` for the embedded Claude Code agent. Click a profile to open/close it; **Launch all** / **Close all** for bulk control. **Set MCP profile** picks which profile's Desktop MCP config the tray edits (does not close running windows).
- **MCP servers ▸** — Claude *Desktop* MCP config (`claude_desktop_config.json`). Claude Code's MCP servers live in `~/.claude.json` and are isolated automatically by the profile swap.
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
~/.config/Claude               → symlink to active Desktop profile
~/.claude                      → symlink to active Claude Code profile
~/.claude.json                 → symlink to active Claude Code state file

~/.config/claude-profiles/         (Claude Desktop — Electron userData)
    personal/
    work/
    .backups/                      → timestamped pre-write copies

~/.config/claude-code-profiles/    (Claude Code — CLI / embedded agent)
    personal/      personal.json
    work/          work.json
```

Switching closes Claude, repoints all three symlinks to the chosen profile, and (optionally) relaunches. A brand-new profile starts with an empty Claude Code dir, so that account logs in fresh.

### Adopting your existing Claude Code config

The first time you switch (or via **Link Claude Code → \<active\>**), your live `~/.claude` and `~/.claude.json` are **moved** into the *currently active* profile — so whatever account you're logged into now is preserved under that name. From then on they're symlinks the switcher manages. The originals are captured by `fs::rename` (a move, same filesystem); on any conflict the live copy is backed up first (see below).

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
| `Edit config…` | atomic write (`.tmp` + rename) — no partial writes |
| Switch profile (live Desktop dir conflicts with target) | live dir moved to `.backups/<ts>_preswitch_Claude/` |
| Switch / link Claude Code (live `~/.claude` conflicts with target) | live dir moved to `.backups/<ts>_preswitch-code_.claude/` |
| Switch / link Claude Code (live `~/.claude.json` conflicts) | previous file copied to `.backups/<ts>_preswitch-code_.claude.json` |

Backups are timestamped (`<unix-ts>_<profile>_<filename>`) and never auto-pruned. To restore, copy a backup back into the profile dir.

## Notes

- Closing Claude before switching is required — Electron locks its config dir, and Claude Code holds `~/.claude` open. The switcher kills both automatically (after confirmation if anything was running). **This includes Claude Desktop's embedded agent**, which is a `claude` process using `~/.claude`, so an in-progress agent session ends on switch.
- Process matching is deliberately narrow so a switch doesn't kill unrelated things: Claude Desktop is matched by the Electron flag `--class=Claude` (plus its `--user-data-dir` helpers and the `claude-desktop` launcher); Claude Code is matched by the **exact** process name `claude`. A shell or editor whose path merely contains "Claude" is left alone.
- In *multiple instances* mode, each profile is launched with `--user-data-dir=~/.config/claude-profiles/<name>/` and `--class=Claude-<name>`. On Linux, `CLAUDE_CONFIG_DIR=~/.config/claude-code-profiles/<name>/` isolates the embedded agent. Switch-mode instances (via symlink) are not killed when toggling other profiles.
- Profile dirs are real directories on disk; the active one is reached via the symlinks at `~/.config/Claude`, `~/.claude`, and `~/.claude.json` on Linux, or via junction at `%APPDATA%\Claude` on Windows. Manual edits to either are safe.
- Isolation is logical, not OS-enforced: all profiles belong to your Unix user, so the guarantee is "the inactive account's data isn't on any path Claude reads," not a permission boundary.
- If a Claude auto-update ever replaces a symlink with a real directory, the next switch treats it as unmanaged state and backs it up before re-linking.
- On Windows, MCP import uses confirm dialogs and per-server submenu items instead of KDE's checklist dialog.
