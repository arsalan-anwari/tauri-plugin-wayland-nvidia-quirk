//! Deciding whether this machine has the bug.

use std::path::{Path, PathBuf};

use crate::status::{NotAffectedReason, SessionType};

/// NVIDIA's PCI vendor id.
const NVIDIA_VENDOR: u32 = 0x10de;

/// The environment the decision is made from.
#[derive(Debug, Clone, Default)]
pub(crate) struct Env {
    pub gdk_backend: Option<String>,
    pub xdg_session_type: Option<String>,
    pub wayland_display: Option<String>,
    pub wayland_socket: Option<String>,
    pub display: Option<String>,
    pub webkit_disable_dmabuf: Option<String>,
    pub prime_offload: Option<String>,
    /// `TAURI_WAYLAND_NVIDIA_QUIRK`
    pub force: Option<String>,
    /// `TAURI_WAYLAND_NVIDIA_QUIRK_VERBOSE`
    pub verbose: Option<String>,
}

impl Env {
    pub(crate) fn from_process() -> Self {
        let get = |k: &str| std::env::var(k).ok();
        Self {
            gdk_backend: get("GDK_BACKEND"),
            xdg_session_type: get("XDG_SESSION_TYPE"),
            wayland_display: get("WAYLAND_DISPLAY"),
            wayland_socket: get("WAYLAND_SOCKET"),
            display: get("DISPLAY"),
            webkit_disable_dmabuf: get("WEBKIT_DISABLE_DMABUF_RENDERER"),
            prime_offload: get("__NV_PRIME_RENDER_OFFLOAD"),
            force: get("TAURI_WAYLAND_NVIDIA_QUIRK"),
            verbose: get("TAURI_WAYLAND_NVIDIA_QUIRK_VERBOSE"),
        }
    }

    pub(crate) fn is_verbose(&self) -> bool {
        matches!(self.verbose.as_deref(), Some("1"))
    }
}

/// A GPU as `/sys/class/drm` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Gpu {
    pub card: String,
    pub vendor: Option<u32>,
    pub driver: Option<String>,
    /// `boot_vga` or `boot_display`: this is the GPU driving the session.
    pub primary: bool,
}

impl Gpu {
    fn is_nvidia(&self) -> bool {
        self.vendor == Some(NVIDIA_VENDOR) && self.driver.as_deref() == Some("nvidia")
    }
}

/// What the plugin should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    Apply {
        gpu: String,
        driver: String,
        session: SessionType,
    },
    NotAffected(NotAffectedReason),
    Overridden(String),
}

/// Parses `GDK_BACKEND` into the backend GDK will actually pick.
pub(crate) fn parse_gdk_backend(list: Option<&str>) -> Option<SessionType> {
    for item in list?.split(',') {
        match item.trim() {
            "wayland" => return Some(SessionType::Wayland),
            "x11" => return Some(SessionType::X11),
            _ => continue,
        }
    }
    None
}

/// `GDK_BACKEND` outranks everything, because it is what GDK and WebKitGTK act
/// on. `XDG_SESSION_TYPE` comes next, then the socket variables, which are all
/// that is left when the process inherited no session environment (a systemd
/// unit, `sudo`, a `.desktop` launch with a scrubbed environment).
pub(crate) fn session_type(env: &Env) -> SessionType {
    if let Some(session) = parse_gdk_backend(env.gdk_backend.as_deref()) {
        return session;
    }
    match env.xdg_session_type.as_deref() {
        Some("wayland") => return SessionType::Wayland,
        Some("x11") => return SessionType::X11,
        _ => {}
    }
    if env.wayland_display.is_some() || env.wayland_socket.is_some() {
        return SessionType::Wayland;
    }
    if env.display.is_some() {
        return SessionType::X11;
    }
    SessionType::Unknown
}

/// Reads a `/sys` file, trimming the trailing newline.
fn read_sysfs(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// The kernel driver bound to a card, from the `device/driver` symlink.
fn driver_name(card: &Path) -> Option<String> {
    std::fs::read_link(card.join("device/driver"))
        .ok()?
        .file_name()?
        .to_str()
        .map(str::to_string)
}

/// Enumerates GPUs under a `/sys/class/drm` style directory.
pub(crate) fn enumerate_gpus(drm_root: &Path) -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir(drm_root) else {
        return Vec::new();
    };

    let mut gpus: Vec<Gpu> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !name.starts_with("card") || name.contains('-') {
                return None;
            }
            let path: PathBuf = entry.path();
            let vendor = read_sysfs(&path.join("device/vendor"))
                .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());
            let primary = read_sysfs(&path.join("device/boot_vga")).as_deref() == Some("1")
                || read_sysfs(&path.join("device/boot_display")).as_deref() == Some("1");
            Some(Gpu {
                card: name,
                vendor,
                driver: driver_name(&path),
                primary,
            })
        })
        .collect();

    gpus.sort_by(|a, b| a.card.cmp(&b.card));
    gpus
}

/// Picks the NVIDIA GPU that matters, if there is one.
pub(crate) fn nvidia_target(gpus: &[Gpu], prime_offload: bool) -> Option<&Gpu> {
    let no_primary_marked = !gpus.iter().any(|gpu| gpu.primary);
    if prime_offload || no_primary_marked {
        return gpus.iter().find(|gpu| gpu.is_nvidia());
    }
    gpus.iter().find(|gpu| gpu.primary && gpu.is_nvidia())
}

/// The whole decision, from inputs a test can fabricate.
pub(crate) fn decide(env: &Env, drm_root: &Path) -> Decision {
    match env.force.as_deref() {
        Some("0") => {
            return Decision::Overridden("TAURI_WAYLAND_NVIDIA_QUIRK=0".to_string());
        }
        Some("1") => {
            let gpus = enumerate_gpus(drm_root);
            let target = nvidia_target(&gpus, env.prime_offload.is_some());
            return Decision::Apply {
                gpu: target
                    .and_then(|gpu| gpu.vendor)
                    .map_or_else(|| "unknown".to_string(), |v| format!("{v:#06x}")),
                driver: target
                    .and_then(|gpu| gpu.driver.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                session: session_type(env),
            };
        }
        _ => {}
    }

    // taking WebKit off the GL path already avoids the bad frame, and the two
    // workarounds together are more disruptive than either alone
    if env.webkit_disable_dmabuf.as_deref() == Some("1") {
        return Decision::Overridden("WEBKIT_DISABLE_DMABUF_RENDERER=1".to_string());
    }

    let session = session_type(env);
    if session != SessionType::Wayland {
        return Decision::NotAffected(NotAffectedReason::NotWayland);
    }

    let gpus = enumerate_gpus(drm_root);
    match nvidia_target(&gpus, env.prime_offload.is_some()) {
        Some(gpu) => Decision::Apply {
            gpu: gpu
                .vendor
                .map_or_else(|| "unknown".to_string(), |vendor| format!("{vendor:#06x}")),
            driver: gpu.driver.clone().unwrap_or_else(|| "unknown".to_string()),
            session,
        },
        None => Decision::NotAffected(NotAffectedReason::NotNvidia),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wayland_env() -> Env {
        Env {
            xdg_session_type: Some("wayland".into()),
            ..Default::default()
        }
    }

    #[test]
    fn gdk_backend_first_recognized_entry_wins() {
        assert_eq!(parse_gdk_backend(None), None);
        assert_eq!(parse_gdk_backend(Some("")), None);
        assert_eq!(parse_gdk_backend(Some("*")), None);
        assert_eq!(
            parse_gdk_backend(Some("wayland")),
            Some(SessionType::Wayland)
        );
        assert_eq!(parse_gdk_backend(Some("x11")), Some(SessionType::X11));
        assert_eq!(
            parse_gdk_backend(Some("wayland,x11")),
            Some(SessionType::Wayland)
        );
        assert_eq!(
            parse_gdk_backend(Some("x11,wayland")),
            Some(SessionType::X11)
        );
        assert_eq!(
            parse_gdk_backend(Some(" wayland , x11 ")),
            Some(SessionType::Wayland)
        );
    }

    #[test]
    fn unrecognized_gdk_backend_entries_are_skipped_not_fatal() {
        assert_eq!(
            parse_gdk_backend(Some("broadway,wayland")),
            Some(SessionType::Wayland)
        );
        assert_eq!(parse_gdk_backend(Some("broadway")), None);
    }

    #[test]
    fn gdk_backend_outranks_session_type() {
        let env = Env {
            gdk_backend: Some("x11".into()),
            xdg_session_type: Some("wayland".into()),
            ..Default::default()
        };
        assert_eq!(session_type(&env), SessionType::X11);
    }

    #[test]
    fn socket_variables_cover_a_scrubbed_environment() {
        let env = Env {
            wayland_display: Some("wayland-0".into()),
            ..Default::default()
        };
        assert_eq!(session_type(&env), SessionType::Wayland);

        let env = Env {
            wayland_socket: Some("7".into()),
            ..Default::default()
        };
        assert_eq!(session_type(&env), SessionType::Wayland);

        assert_eq!(session_type(&Env::default()), SessionType::Unknown);
    }

    #[test]
    fn display_alone_means_x11() {
        let env = Env {
            display: Some(":0".into()),
            ..Default::default()
        };
        assert_eq!(session_type(&env), SessionType::X11);
    }

    struct FakeDrm(PathBuf);

    impl FakeDrm {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "twnq-{}-{}-{name}",
                std::process::id(),
                name.len()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn card(&self, name: &str, vendor: &str, driver: Option<&str>, primary: bool) -> &Self {
            let device = self.0.join(name).join("device");
            std::fs::create_dir_all(&device).unwrap();
            std::fs::write(device.join("vendor"), format!("{vendor}\n")).unwrap();
            if primary {
                std::fs::write(device.join("boot_vga"), "1\n").unwrap();
            }
            if let Some(driver) = driver {
                let target = self.0.join("drivers").join(driver);
                std::fs::create_dir_all(&target).unwrap();
                std::os::unix::fs::symlink(&target, device.join("driver")).unwrap();
            }
            self
        }

        fn connector(&self, name: &str) -> &Self {
            std::fs::create_dir_all(self.0.join(name)).unwrap();
            self
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FakeDrm {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn enumerates_cards_and_ignores_connectors() {
        let drm = FakeDrm::new("enum");
        drm.card("card1", "0x10de", Some("nvidia"), true)
            .connector("card1-DP-1")
            .connector("renderD128");

        let gpus = enumerate_gpus(drm.path());
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].card, "card1");
        assert_eq!(gpus[0].vendor, Some(0x10de));
        assert_eq!(gpus[0].driver.as_deref(), Some("nvidia"));
        assert!(gpus[0].primary);
    }

    #[test]
    fn missing_drm_directory_is_not_a_panic() {
        assert!(enumerate_gpus(Path::new("/nonexistent/drm")).is_empty());
    }

    #[test]
    fn nouveau_is_not_a_match() {
        let drm = FakeDrm::new("nouveau");
        drm.card("card0", "0x10de", Some("nouveau"), true);
        assert_eq!(
            decide(&wayland_env(), drm.path()),
            Decision::NotAffected(NotAffectedReason::NotNvidia)
        );
    }

    #[test]
    fn amd_is_not_a_match() {
        let drm = FakeDrm::new("amd");
        drm.card("card0", "0x1002", Some("amdgpu"), true);
        assert_eq!(
            decide(&wayland_env(), drm.path()),
            Decision::NotAffected(NotAffectedReason::NotNvidia)
        );
    }

    #[test]
    fn nvidia_on_wayland_is_a_match() {
        let drm = FakeDrm::new("nv");
        drm.card("card1", "0x10de", Some("nvidia"), true);
        assert_eq!(
            decide(&wayland_env(), drm.path()),
            Decision::Apply {
                gpu: "0x10de".into(),
                driver: "nvidia".into(),
                session: SessionType::Wayland,
            }
        );
    }

    #[test]
    fn nvidia_on_x11_is_left_alone() {
        let drm = FakeDrm::new("x11");
        drm.card("card1", "0x10de", Some("nvidia"), true);
        let env = Env {
            xdg_session_type: Some("x11".into()),
            ..Default::default()
        };
        assert_eq!(
            decide(&env, drm.path()),
            Decision::NotAffected(NotAffectedReason::NotWayland)
        );
    }

    #[test]
    fn hybrid_laptop_rendering_on_the_igpu_is_left_alone() {
        let drm = FakeDrm::new("hybrid");
        drm.card("card0", "0x8086", Some("i915"), true).card(
            "card1",
            "0x10de",
            Some("nvidia"),
            false,
        );
        assert_eq!(
            decide(&wayland_env(), drm.path()),
            Decision::NotAffected(NotAffectedReason::NotNvidia)
        );
    }

    #[test]
    fn hybrid_laptop_under_prime_offload_is_a_match() {
        let drm = FakeDrm::new("prime");
        drm.card("card0", "0x8086", Some("i915"), true).card(
            "card1",
            "0x10de",
            Some("nvidia"),
            false,
        );
        let env = Env {
            prime_offload: Some("1".into()),
            ..wayland_env()
        };
        assert!(matches!(decide(&env, drm.path()), Decision::Apply { .. }));
    }

    #[test]
    fn no_primary_marked_falls_back_to_any_nvidia() {
        let drm = FakeDrm::new("noprimary");
        drm.card("card1", "0x10de", Some("nvidia"), false);
        assert!(matches!(
            decide(&wayland_env(), drm.path()),
            Decision::Apply { .. }
        ));
    }

    #[test]
    fn quirk_can_be_switched_off() {
        let drm = FakeDrm::new("off");
        drm.card("card1", "0x10de", Some("nvidia"), true);
        let env = Env {
            force: Some("0".into()),
            ..wayland_env()
        };
        assert_eq!(
            decide(&env, drm.path()),
            Decision::Overridden("TAURI_WAYLAND_NVIDIA_QUIRK=0".into())
        );
    }

    #[test]
    fn quirk_can_be_forced_on_undetected_hardware() {
        let drm = FakeDrm::new("on");
        let env = Env {
            force: Some("1".into()),
            ..Default::default()
        };
        assert!(matches!(decide(&env, drm.path()), Decision::Apply { .. }));
    }

    #[test]
    fn dmabuf_override_wins_over_detection() {
        let drm = FakeDrm::new("dmabuf");
        drm.card("card1", "0x10de", Some("nvidia"), true);
        let env = Env {
            webkit_disable_dmabuf: Some("1".into()),
            ..wayland_env()
        };
        assert_eq!(
            decide(&env, drm.path()),
            Decision::Overridden("WEBKIT_DISABLE_DMABUF_RENDERER=1".into())
        );
    }

    #[test]
    fn explicit_on_beats_the_dmabuf_override() {
        let drm = FakeDrm::new("bothflags");
        drm.card("card1", "0x10de", Some("nvidia"), true);
        let env = Env {
            force: Some("1".into()),
            webkit_disable_dmabuf: Some("1".into()),
            ..wayland_env()
        };
        assert!(matches!(decide(&env, drm.path()), Decision::Apply { .. }));
    }
}

#[cfg(test)]
mod probe {
    //! `cargo test -- --ignored --nocapture probe`
    use super::*;

    #[test]
    #[ignore = "machine-specific probe, not a pass/fail assertion"]
    fn decision_on_this_machine() {
        let env = Env::from_process();
        println!("session  = {:?}", session_type(&env));
        for gpu in enumerate_gpus(Path::new("/sys/class/drm")) {
            println!("gpu      = {gpu:?}");
        }
        println!("decision = {:?}", decide(&env, Path::new("/sys/class/drm")));
    }
}
