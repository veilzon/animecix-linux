mod api;
mod app;
mod aria;
mod auth;
mod covers;
mod desktop;
mod diagnostics;
mod download;
mod download_folders;
mod font;
mod http;
mod icons;
mod local_playback;
mod memory;
mod music;
mod play_quality;
mod player;
mod segmented;
mod skip;
mod startup;
mod stats;
mod theme;
mod ui;
mod update;

use adw::prelude::*;

pub(crate) use desktop::{
    appimage_integration_disabled, check_all_dependencies, check_desktop_entry_installed,
    install_desktop_entry, uninstall_application,
};

pub fn restart_app() {
    let exe = std::env::var("APPIMAGE")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.is_file())
        .or_else(|| std::env::current_exe().ok());
    if let Some(exe) = exe {
        update::spawn_restart(&exe);
        std::process::exit(0);
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.iter().any(|a| a == "--check-shaders") {
        return diagnostics::cmd_check_shaders();
    }
    if raw_args.iter().any(|a| a == "--check-icons") {
        return diagnostics::cmd_check_icons();
    }
    let mut filtered: Vec<String> = Vec::new();
    let mut it = raw_args.iter();
    if let Some(p) = it.next() {
        filtered.push(p.clone());
    }
    while let Some(a) = it.next() {
        if a == "--goto" {
            let _ = it.next(); // değeri atla
        } else if a.starts_with("--goto=") {
        } else {
            filtered.push(a.clone());
        }
    }

    std::env::set_var("MALLOC_ARENA_MAX", "2");
    // Hafif mod bayrağı: tüm Client kurmadan doğrudan ayar dosyasından
    // okunur (Client::new HTTP/runtime kurar, açılışı yavaşlatır).
    let light_mode = std::fs::read_to_string(api::Client::settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("light_mode").and_then(|b| b.as_bool()))
        .unwrap_or(false);
    if light_mode {
        std::env::set_var("GSK_RENDERER", "cairo");
        eprintln!("[STARTUP] hafif mod aktif (cairo renderer)");
    }

    let app = adw::Application::builder()
        .application_id("tr.com.animecix")
        .build();

    app.connect_activate(startup::activate);

    let argv: Vec<&str> = filtered.iter().map(|s| s.as_str()).collect();
    app.run_with_args(&argv);
}
