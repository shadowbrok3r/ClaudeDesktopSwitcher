use crate::profiles;
use std::sync::Mutex;
use std::time::Duration;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};
use winit::event::Event;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;

struct State {
    relaunch_after_switch: bool,
}

enum UserEvent {
    Menu(MenuEvent),
    Refresh,
}

fn menu_id(label: &str) -> MenuId {
    MenuId::new(label)
}

fn build_menu(state: &State) -> Menu {
    let menu = Menu::new();
    let current = profiles::current_profile();
    let profile_list = profiles::list_profiles();

    let header = match &current {
        Some(p) => format!("Active: {p}"),
        None => "Active: (unmanaged)".into(),
    };
    let _ = menu.append(&MenuItem::with_id(menu_id("header"), header, false, None));
    let _ = menu.append(&PredefinedMenuItem::separator());

    if profile_list.is_empty() {
        let _ = menu.append(&MenuItem::with_id(
            menu_id("no-profiles"),
            "No profiles yet — use New profile…",
            false,
            None,
        ));
    } else {
        for p in &profile_list {
            let is_cur = Some(p) == current.as_ref();
            let label = if is_cur {
                format!("● {p}")
            } else {
                format!("    {p}")
            };
            let _ = menu.append(&MenuItem::with_id(
                menu_id(&format!("switch:{p}")),
                label,
                !is_cur,
                None,
            ));
        }
    }

    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&build_mcp_submenu(&current));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(
        menu_id("new-profile"),
        "New profile…",
        true,
        None,
    ));
    let _ = menu.append(&build_rename_submenu(&profile_list));
    let _ = menu.append(&CheckMenuItem::with_id(
        menu_id("relaunch"),
        "Relaunch Claude after switch",
        true,
        state.relaunch_after_switch,
        None,
    ));
    let _ = menu.append(&MenuItem::with_id(
        menu_id("open-profiles"),
        "Open profiles folder",
        true,
        None,
    ));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(menu_id("quit"), "Quit", true, None));

    menu
}

fn build_mcp_submenu(current_profile_name: &Option<String>) -> Submenu {
    let mcp = Submenu::with_id(menu_id("mcp"), "MCP servers", true);
    let current_path = profiles::current_config_path();
    let current_servers = profiles::list_mcp_servers(&current_path);
    let all_profiles = profiles::list_profiles();
    let other_profiles: Vec<String> = all_profiles
        .into_iter()
        .filter(|p| Some(p) != current_profile_name.as_ref())
        .collect();

    let _ = mcp.append(&MenuItem::with_id(
        menu_id("mcp-count"),
        format!("Current servers: {}", current_servers.len()),
        false,
        None,
    ));
    if current_servers.is_empty() {
        let _ = mcp.append(&MenuItem::with_id(menu_id("mcp-none"), "    (none)", false, None));
    } else {
        for name in &current_servers {
            let _ = mcp.append(&MenuItem::with_id(
                menu_id(&format!("mcp-cur:{name}")),
                format!("    • {name}"),
                false,
                None,
            ));
        }
    }

    let _ = mcp.append(&PredefinedMenuItem::separator());

    let import = Submenu::with_id(menu_id("mcp-import"), "Import from", true);
    if other_profiles.is_empty() {
        let _ = import.append(&MenuItem::with_id(
            menu_id("mcp-import-none"),
            "(no other profiles)",
            false,
            None,
        ));
    } else {
        for p in &other_profiles {
            let src = profiles::profile_config_path(p);
            let servers = profiles::list_mcp_servers(&src);
            if servers.is_empty() {
                let _ = import.append(&MenuItem::with_id(
                    menu_id(&format!("mcp-import-all:{p}")),
                    format!("{p} (none)"),
                    false,
                    None,
                ));
            } else if servers.len() == 1 {
                let s = &servers[0];
                let _ = import.append(&MenuItem::with_id(
                    menu_id(&format!("mcp-import-one:{p}:{s}")),
                    format!("{p} → {s}"),
                    true,
                    None,
                ));
            } else {
                let sub = Submenu::with_id(menu_id(&format!("mcp-import-sub:{p}")), p, true);
                let _ = sub.append(&MenuItem::with_id(
                    menu_id(&format!("mcp-import-all:{p}")),
                    "Import all",
                    true,
                    None,
                ));
                for s in &servers {
                    let _ = sub.append(&MenuItem::with_id(
                        menu_id(&format!("mcp-import-one:{p}:{s}")),
                        s.clone(),
                        true,
                        None,
                    ));
                }
                let _ = import.append(&sub);
            }
        }
    }
    let _ = mcp.append(&import);

    let _ = mcp.append(&PredefinedMenuItem::separator());
    let _ = mcp.append(&MenuItem::with_id(
        menu_id("mcp-edit"),
        "Edit config…",
        true,
        None,
    ));

    let backup_count = profiles::backup_count();
    let _ = mcp.append(&MenuItem::with_id(
        menu_id("mcp-backups"),
        format!("Open backups folder ({backup_count})"),
        backup_count > 0,
        None,
    ));
    let _ = mcp.append(&MenuItem::with_id(
        menu_id("mcp-clear"),
        "Clear all MCP servers",
        !current_servers.is_empty(),
        None,
    ));

    mcp
}

fn build_rename_submenu(profile_list: &[String]) -> Submenu {
    let rename = Submenu::with_id(menu_id("rename"), "Rename profile", !profile_list.is_empty());
    if profile_list.is_empty() {
        let _ = rename.append(&MenuItem::with_id(
            menu_id("rename-none"),
            "(no profiles)",
            false,
            None,
        ));
    } else {
        for p in profile_list {
            let _ = rename.append(&MenuItem::with_id(
                menu_id(&format!("rename:{p}")),
                p,
                true,
                None,
            ));
        }
    }
    rename
}

fn handle_menu_event(id: &MenuId, state: &mut State) {
    let key = id.0.as_str();
    if let Some(name) = key.strip_prefix("switch:") {
        profiles::do_switch(name, state.relaunch_after_switch);
        return;
    }
    if key == "new-profile" {
        if let Some(name) = profiles::prompt_new_profile() {
            profiles::do_switch(&name, state.relaunch_after_switch);
        }
        return;
    }
    if let Some(rest) = key.strip_prefix("rename:") {
        if let Some(new) = profiles::prompt_rename(rest) {
            match profiles::rename_profile(rest, &new, state.relaunch_after_switch) {
                Ok(()) => crate::platform::notify(&format!("Renamed '{rest}' → '{new}'")),
                Err(e) => crate::platform::notify(&format!("Rename failed: {e}")),
            }
        }
        return;
    }
    if key == "relaunch" {
        state.relaunch_after_switch = !state.relaunch_after_switch;
        return;
    }
    if key == "open-profiles" {
        profiles::open_profiles_dir();
        return;
    }
    if key == "quit" {
        std::process::exit(0);
    }
    if key == "mcp-edit" {
        profiles::open_config();
        return;
    }
    if key == "mcp-backups" {
        profiles::open_backups_dir();
        return;
    }
    if key == "mcp-clear" {
        profiles::clear_all_mcp();
        return;
    }
    if let Some(p) = key.strip_prefix("mcp-import-all:") {
        profiles::import_from_profile(p);
        return;
    }
    if let Some(rest) = key.strip_prefix("mcp-import-one:") {
        if let Some((profile, server)) = rest.split_once(':') {
            profiles::import_single_server(profile, server);
        }
    }
}

fn tray_icon() -> Icon {
    let w = 16u32;
    let h = 16u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let head = x >= 5 && x <= 10 && y >= 1 && y <= 5;
            let body = x >= 3 && x <= 12 && y >= 5 && y <= 13;
            if head || body {
                rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            } else {
                rgba[i..i + 4].copy_from_slice(&[0, 120, 212, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, w, h).expect("icon rgba")
}

pub fn run() {
    let state = Mutex::new(State {
        relaunch_after_switch: true,
    });

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    builder.with_any_thread(true);
    let event_loop = builder.build().expect("event loop");

    let proxy = event_loop.create_proxy();
    let menu = build_menu(&state.lock().unwrap());
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Claude Switcher")
        .with_icon(tray_icon())
        .build()
        .expect("tray icon");
    let tray = Mutex::new(tray);

    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }
    }));

    std::thread::spawn({
        let proxy = proxy.clone();
        move || loop {
            std::thread::sleep(Duration::from_secs(3));
            let _ = proxy.send_event(UserEvent::Refresh);
        }
    });

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);
            match event {
                Event::UserEvent(UserEvent::Menu(ev)) => {
                    if let Ok(mut s) = state.lock() {
                        handle_menu_event(&ev.id, &mut s);
                    }
                }
                Event::UserEvent(UserEvent::Refresh) => {
                    if let Ok(s) = state.lock() {
                        let new_menu = build_menu(&s);
                        if let Ok(tray) = tray.lock() {
                            let _ = tray.set_menu(Some(Box::new(new_menu)));
                            let _ = tray.set_tooltip(Some(&format!(
                                "Claude Switcher — {}",
                                profiles::active_tooltip()
                            )));
                        }
                    }
                }
                _ => {}
            }
        })
        .expect("event loop run");
}
