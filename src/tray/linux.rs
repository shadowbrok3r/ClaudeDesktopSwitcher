use crate::profiles;
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
        ksni::ToolTip {
            title: "Claude Switcher".into(),
            description: profiles::active_tooltip(),
            icon_name: "system-switch-user".into(),
            icon_pixmap: vec![],
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let current = profiles::current_profile();
        let profile_list = profiles::list_profiles();
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

        items.push(MenuItem::Separator);
        items.push(self.mcp_submenu(&current));

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "New profile…".into(),
                activate: Box::new(|this: &mut Self| {
                    if let Some(name) = profiles::prompt_new_profile() {
                        profiles::do_switch(&name, this.relaunch_after_switch);
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
                    let relaunch = self.relaunch_after_switch;
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
