//! Fixes the Wayland/Nvidia startup failure in Tauri v2 apps without giving up
//! hardware acceleration.

#![deny(missing_docs)]

mod status;

pub use status::{Error, NotAffectedReason, SessionType, Status};

#[cfg(target_os = "linux")]
mod detect;
#[cfg(target_os = "linux")]
mod imp;

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime, WebviewWindow,
};

/// Registers the quirk.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("wayland-nvidia-quirk")
        .setup(|_app, _api| {
            #[cfg(target_os = "linux")]
            imp::setup(_app);
            Ok(())
        })
        .build()
}

/// Applies the quirk to a single window.
pub fn apply<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        imp::apply(window)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window;
        Ok(())
    }
}

/// What the quirk decided and whether it worked.
pub fn status() -> Status {
    #[cfg(target_os = "linux")]
    {
        imp::status()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Status::NotAffected {
            reason: NotAffectedReason::NotLinux,
        }
    }
}
