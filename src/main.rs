#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod platform;
mod profiles;
mod tray;

fn main() {
    tray::run();
}
