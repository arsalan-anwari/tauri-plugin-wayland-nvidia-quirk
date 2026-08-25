//! Demo app for `tauri-plugin-wayland-nvidia-quirk`.
//!
//! ```sh
//! cargo run                                  # fix on  -> window opens
//! TAURI_WAYLAND_NVIDIA_QUIRK=0 cargo run     # fix off -> Gdk-Message: Error 71
//! ```

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Everything the UI shows, gathered in one round trip.
#[derive(Serialize)]
struct Report {
    status: String,
    applied: bool,
    session: Vec<(String, String)>,
    gpus: Vec<Gpu>,
    overrides: Vec<(String, String)>,
}

#[derive(Serialize)]
struct Gpu {
    card: String,
    vendor: String,
    driver: String,
    primary: bool,
}

const SESSION_VARS: [&str; 5] = [
    "XDG_SESSION_TYPE",
    "GDK_BACKEND",
    "WAYLAND_DISPLAY",
    "WAYLAND_SOCKET",
    "DISPLAY",
];

const OVERRIDE_VARS: [&str; 4] = [
    "TAURI_WAYLAND_NVIDIA_QUIRK",
    "TAURI_WAYLAND_NVIDIA_QUIRK_VERBOSE",
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "__NV_DISABLE_EXPLICIT_SYNC",
];

fn env_pairs(names: &[&str]) -> Vec<(String, String)> {
    names
        .iter()
        .map(|name| {
            let value = std::env::var(name).unwrap_or_else(|_| "(unset)".to_string());
            ((*name).to_string(), value)
        })
        .collect()
}

fn gpus() -> Vec<Gpu> {
    let read = |path: &Path| {
        std::fs::read_to_string(path)
            .ok()
            .map(|text| text.trim().to_string())
    };
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };

    let mut gpus: Vec<Gpu> = entries
        .flatten()
        .filter_map(|entry| {
            let card = entry.file_name().to_string_lossy().into_owned();
            if !card.starts_with("card") || card.contains('-') {
                return None;
            }
            let device = entry.path().join("device");
            Some(Gpu {
                vendor: read(&device.join("vendor")).unwrap_or_else(|| "?".to_string()),
                driver: std::fs::read_link(device.join("driver"))
                    .ok()
                    .and_then(|target| {
                        target
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| "?".to_string()),
                primary: read(&device.join("boot_vga")).as_deref() == Some("1"),
                card,
            })
        })
        .collect();
    gpus.sort_by(|a, b| a.card.cmp(&b.card));
    gpus
}

#[tauri::command]
fn report() -> Report {
    let status = tauri_plugin_wayland_nvidia_quirk::status();
    Report {
        applied: status.is_applied(),
        status: format!("{status:?}"),
        session: env_pairs(&SESSION_VARS),
        gpus: gpus(),
        overrides: env_pairs(&OVERRIDE_VARS),
    }
}

#[tauri::command]
fn open_second_window(app: tauri::AppHandle) -> Result<String, String> {
    if let Some(existing) = app.get_webview_window("second") {
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok("second window already open".to_string());
    }

    let window = WebviewWindowBuilder::new(&app, "second", WebviewUrl::App("index.html".into()))
        .title("second window (created at runtime)")
        .inner_size(700.0, 560.0)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;

    let applied = tauri_plugin_wayland_nvidia_quirk::apply(&window);
    window.show().map_err(|e| e.to_string())?;

    match applied {
        Ok(()) => Ok(format!(
            "second window opened; apply() ok, status now {:?}",
            tauri_plugin_wayland_nvidia_quirk::status()
        )),
        Err(error) => Err(format!("apply() failed: {error}")),
    }
}

fn millis_from_env(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.parse().ok()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_wayland_nvidia_quirk::init())
        .invoke_handler(tauri::generate_handler![report, open_second_window])
        .setup(|app| {
            // the line worth pasting into a bug report
            eprintln!("quirk: {:?}", tauri_plugin_wayland_nvidia_quirk::status());

            if let Some(millis) = millis_from_env("DEMO_OPEN_SECOND_AFTER_MS") {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(millis));
                    let opening = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        eprintln!("second window: {:?}", open_second_window(opening));
                    });
                });
            }

            // lets verify.sh run the app unattended
            if let Some(millis) = millis_from_env("DEMO_EXIT_AFTER_MS") {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(millis));
                    let exiting = handle.clone();
                    let _ = handle.run_on_main_thread(move || exiting.exit(0));
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start the demo app");
}
