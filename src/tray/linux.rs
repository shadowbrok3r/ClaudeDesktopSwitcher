use crate::profiles::{self, RunMode};
use ksni::{
    MenuItem, Tray,
    blocking::TrayMethods,
    menu::{CheckmarkItem, StandardItem, SubMenu},
};
use std::time::Duration;

struct App {
    relaunch_after_switch: bool,
}

impl App {
    fn mcp_submenu(&self, current_profile_name: &Option<String>) -> MenuItem<Self> {
        let current_path = profiles::current_config_path();
        let current_servers = profiles::list_mcp_servers(&current_path);
        let all_profiles = profiles::list_profiles();
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
                            profiles::import_from_profile(&label);
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
                activate: Box::new(|_| profiles::open_config()),
                ..Default::default()
            }
            .into(),
        );
        let backup_count = profiles::backup_count();
        sub.push(
            StandardItem {
                label: format!("Open backups folder ({backup_count})"),
                enabled: backup_count > 0,
                activate: Box::new(|_| profiles::open_backups_dir()),
                ..Default::default()
            }
            .into(),
        );
        sub.push(
            StandardItem {
                label: "Clear all MCP servers".into(),
                enabled: !current_servers.is_empty(),
                activate: Box::new(|_| profiles::clear_all_mcp()),
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

    // One source→destination pair's memory, grouped by project key.
    fn memory_pair_submenu(&self, source: &str, dest: &str, label: String) -> MenuItem<Self> {
        let projects = profiles::list_memory_projects(source);
        let items: Vec<MenuItem<Self>> = if projects.is_empty() {
            vec![
                StandardItem {
                    label: "(no memory)".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            ]
        } else {
            projects
                .iter()
                .map(|project| {
                    let count = profiles::list_memory_files(source, project).len();
                    let src = source.to_string();
                    let dst = dest.to_string();
                    let key = project.clone();
                    StandardItem {
                        label: format!("{} ({count})", profiles::project_label(project)),
                        activate: Box::new(move |_: &mut Self| {
                            profiles::import_memory_from(&src, &dst, &key);
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect()
        };
        SubMenu {
            label,
            submenu: items,
            enabled: !projects.is_empty(),
            ..Default::default()
        }
        .into()
    }

    // Switch mode has one live profile to import into; multi-instance mode runs
    // them all, so every profile is a candidate destination.
    fn import_destinations(&self, mode: RunMode, current: &Option<String>) -> Vec<String> {
        match mode {
            RunMode::Switch => current.clone().into_iter().collect(),
            RunMode::MultiInstance => profiles::list_profiles(),
        }
    }

    fn memory_submenu(&self, mode: RunMode, current: &Option<String>) -> MenuItem<Self> {
        let dests = self.import_destinations(mode, current);
        // Name the destination only when there is more than one to choose from.
        let named = dests.len() > 1;
        let pairs: Vec<(String, String)> = dests
            .iter()
            .flat_map(|dest| {
                profiles::import_sources(Some(dest))
                    .into_iter()
                    .map(move |src| (src, dest.clone()))
            })
            .collect();

        let mut sub: Vec<MenuItem<Self>> = Vec::new();
        sub.push(
            StandardItem {
                label: "Nothing crosses profiles until imported here".into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );
        sub.push(
            StandardItem {
                label: "Imports are copies, not links".into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );
        sub.push(MenuItem::Separator);

        if dests.is_empty() {
            sub.push(
                StandardItem {
                    label: "(no active profile to import into)".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        } else if pairs.is_empty() {
            sub.push(
                StandardItem {
                    label: "(no other profiles)".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            let pair_label = |src: &str, dest: &str| {
                if named {
                    format!("{src} → {dest}")
                } else {
                    src.to_string()
                }
            };

            sub.push(
                SubMenu {
                    label: "Import memory from".into(),
                    submenu: pairs
                        .iter()
                        .map(|(src, dest)| {
                            self.memory_pair_submenu(src, dest, pair_label(src, dest))
                        })
                        .collect(),
                    ..Default::default()
                }
                .into(),
            );

            let skill_items: Vec<MenuItem<Self>> = pairs
                .iter()
                .map(|(source, dest)| {
                    let count = profiles::list_code_skills(source).len();
                    let src = source.clone();
                    let dst = dest.clone();
                    StandardItem {
                        label: format!("{} ({count})", pair_label(source, dest)),
                        enabled: count > 0,
                        activate: Box::new(move |_: &mut Self| {
                            profiles::import_skills_from(&src, &dst);
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect();
            sub.push(
                SubMenu {
                    label: "Import skills from".into(),
                    submenu: skill_items,
                    ..Default::default()
                }
                .into(),
            );
        }

        sub.push(MenuItem::Separator);
        sub.push(
            StandardItem {
                label: "Check isolation…".into(),
                activate: Box::new(|_| profiles::show_isolation_report()),
                ..Default::default()
            }
            .into(),
        );

        SubMenu {
            label: "Memory & skills".into(),
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
        let description = match profiles::run_mode() {
            RunMode::Switch => {
                let cur = profiles::active_tooltip();
                let code = profiles::current_code_profile().unwrap_or_else(|| "unmanaged".into());
                format!("Desktop: {cur}  •  Code: {code}")
            }
            RunMode::MultiInstance => {
                let running = profiles::running_profiles();
                if running.is_empty() {
                    "Multiple instances — none running".into()
                } else {
                    format!("Running: {}", running.join(", "))
                }
            }
        };
        ksni::ToolTip {
            title: "Claude Switcher".into(),
            description,
            icon_name: "system-switch-user".into(),
            icon_pixmap: vec![],
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mode = profiles::run_mode();
        let current = profiles::current_profile();
        let profile_list = profiles::list_profiles();
        let mut items: Vec<MenuItem<Self>> = Vec::new();

        match mode {
            RunMode::Switch => {
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

                let code_current = profiles::current_code_profile();
                let code_in_sync = current.is_some() && code_current == current;
                let code_label = match &code_current {
                    Some(c) if code_in_sync => format!("Claude Code: {c} ✓"),
                    Some(c) => format!("Claude Code: {c} (out of sync)"),
                    None => "Claude Code: unmanaged".into(),
                };
                items.push(
                    StandardItem {
                        label: code_label,
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
                if current.is_some() && !code_in_sync {
                    let relaunch = self.relaunch_after_switch;
                    items.push(
                        StandardItem {
                            label: format!(
                                "Link Claude Code → {}",
                                current.as_deref().unwrap_or("")
                            ),
                            activate: Box::new(move |_: &mut Self| do_link_code(relaunch)),
                            ..Default::default()
                        }
                        .into(),
                    );
                }
            }
            RunMode::MultiInstance => {
                let mcp = current
                    .clone()
                    .unwrap_or_else(|| "(none)".into());
                items.push(
                    StandardItem {
                        label: format!("MCP profile: {mcp}"),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
                let running = profiles::running_profiles();
                let running_label = if running.is_empty() {
                    "Running: none".into()
                } else {
                    format!("Running: {}", running.join(", "))
                };
                items.push(
                    StandardItem {
                        label: running_label,
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
            }
        }
        items.push(MenuItem::Separator);

        if profile_list.is_empty() {
            items.push(
                StandardItem {
                    label: "No profiles yet — use “New profile…”".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            match mode {
                RunMode::Switch => {
                    for p in &profile_list {
                        let is_cur = Some(p) == current.as_ref();
                        let label = if is_cur {
                            format!("● {p}")
                        } else {
                            format!("    {p}")
                        };
                        let name = p.clone();
                        let relaunch = self.relaunch_after_switch;
                        items.push(
                            StandardItem {
                                label,
                                enabled: !is_cur,
                                activate: Box::new(move |_: &mut Self| {
                                    profiles::do_switch(&name, relaunch);
                                }),
                                ..Default::default()
                            }
                            .into(),
                        );
                    }
                }
                RunMode::MultiInstance => {
                    for p in &profile_list {
                        let running = profiles::is_profile_running(p);
                        let is_mcp = Some(p) == current.as_ref();
                        let suffix = match (running, is_mcp) {
                            (true, true) => " ▶ running, MCP",
                            (true, false) => " ▶ running",
                            (false, true) => " (MCP)",
                            (false, false) => "",
                        };
                        let label = format!("{p}{suffix}");
                        let name = p.clone();
                        items.push(
                            StandardItem {
                                label,
                                activate: Box::new(move |_: &mut Self| {
                                    profiles::toggle_profile(&name);
                                }),
                                ..Default::default()
                            }
                            .into(),
                        );
                    }
                    items.push(MenuItem::Separator);
                    items.push(
                        StandardItem {
                            label: "Launch all".into(),
                            activate: Box::new(|_: &mut Self| profiles::launch_all_profiles()),
                            ..Default::default()
                        }
                        .into(),
                    );
                    items.push(
                        StandardItem {
                            label: "Close all".into(),
                            enabled: !profiles::running_profiles().is_empty(),
                            activate: Box::new(|_: &mut Self| profiles::close_all_profiles()),
                            ..Default::default()
                        }
                        .into(),
                    );
                    if profile_list.len() > 1 {
                        let set_mcp_items: Vec<MenuItem<Self>> = profile_list
                            .iter()
                            .map(|p| {
                                let name = p.clone();
                                let is_cur = Some(p) == current.as_ref();
                                StandardItem {
                                    label: if is_cur {
                                        format!("● {p}")
                                    } else {
                                        p.clone()
                                    },
                                    enabled: !is_cur,
                                    activate: Box::new(move |_: &mut Self| set_mcp_profile(&name)),
                                    ..Default::default()
                                }
                                .into()
                            })
                            .collect();
                        items.push(
                            SubMenu {
                                label: "Set MCP profile".into(),
                                submenu: set_mcp_items,
                                ..Default::default()
                            }
                            .into(),
                        );
                    }
                }
            }
        }

        items.push(MenuItem::Separator);
        items.push(self.mcp_submenu(&current));
        items.push(self.memory_submenu(mode, &current));

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "New profile…".into(),
                activate: Box::new(|this: &mut Self| {
                    if let Some(name) = profiles::prompt_new_profile() {
                        profiles::do_new_profile(&name, this.relaunch_after_switch);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        let rename_items: Vec<MenuItem<Self>> = if profile_list.is_empty() {
            vec![
                StandardItem {
                    label: "(no profiles)".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            ]
        } else {
            profile_list
                .iter()
                .map(|p| {
                    let old = p.clone();
                    StandardItem {
                        label: p.clone(),
                        activate: Box::new(move |this: &mut Self| {
                            if let Some(new) = profiles::prompt_rename(&old) {
                                match profiles::rename_profile(&old, &new, this.relaunch_after_switch)
                                {
                                    Ok(()) => {
                                        crate::platform::notify(&format!(
                                            "Renamed '{old}' → '{new}'"
                                        ));
                                    }
                                    Err(e) => {
                                        crate::platform::notify(&format!("Rename failed: {e}"))
                                    }
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
                enabled: !profile_list.is_empty(),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            CheckmarkItem {
                label: "Switch profiles (one at a time)".into(),
                checked: mode == RunMode::Switch,
                activate: Box::new(|_: &mut Self| {
                    if profiles::run_mode() != RunMode::Switch {
                        profiles::toggle_run_mode();
                    }
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            CheckmarkItem {
                label: "Multiple instances (side by side)".into(),
                checked: mode == RunMode::MultiInstance,
                activate: Box::new(|_: &mut Self| {
                    if profiles::run_mode() != RunMode::MultiInstance {
                        profiles::toggle_run_mode();
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        if mode == RunMode::Switch {
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
        }

        items.push(
            StandardItem {
                label: "Open profiles folder".into(),
                activate: Box::new(|_| profiles::open_profiles_dir()),
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

fn set_mcp_profile(name: &str) {
    if profiles::current_profile().as_deref() == Some(name) {
        return;
    }
    match profiles::set_primary_profile(name) {
        Ok(()) => crate::platform::notify(&format!("MCP profile set to '{name}'")),
        Err(e) => crate::platform::notify(&format!("Set MCP profile failed: {e}")),
    }
}

fn do_link_code(relaunch_after_switch: bool) {
    let Some(active) = profiles::current_profile() else {
        crate::platform::notify("No active profile — switch to one first");
        return;
    };
    if profiles::current_code_profile().as_deref() == Some(active.as_str()) {
        crate::platform::notify(&format!("Claude Code already linked to '{active}'"));
        return;
    }
    if crate::platform::is_claude_running()
        && !crate::platform::confirm(
            "Claude must close to capture its Claude Code data. Continue?",
        )
    {
        return;
    }
    crate::platform::kill_claude();
    if let Err(e) = profiles::capture_unmanaged_code(&active) {
        crate::platform::notify(&format!("Capture failed: {e}"));
        return;
    }
    if let Err(e) = profiles::link_code(&active) {
        crate::platform::notify(&format!("Link failed: {e}"));
        return;
    }
    crate::platform::notify(&format!("Claude Code linked to '{active}'"));
    if relaunch_after_switch {
        crate::platform::relaunch_claude();
    }
}

pub fn run() {
    let handle = App {
        relaunch_after_switch: true,
    }
    .spawn()
    .expect("register tray");

    loop {
        std::thread::sleep(Duration::from_secs(3));
        handle.update(|_| {});
    }
}
