//! The Linux half: decide once, then force the GL paint context.

use std::path::Path;
use std::sync::{OnceLock, RwLock};

use gtk::gio;
use gtk::glib::object::IsA;
use gtk::prelude::*;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

use crate::detect::{self, Decision, Env};
use crate::status::{Error, SessionType, Status};

const DRM_ROOT: &str = "/sys/class/drm";

static DECISION: OnceLock<Decision> = OnceLock::new();
static VERBOSE: OnceLock<bool> = OnceLock::new();
static STATUS: RwLock<Status> = RwLock::new(Status::NotRun);

/// Detection runs once per process; nothing it reads changes while we run.
fn decision() -> &'static Decision {
    DECISION.get_or_init(|| detect::decide(&Env::from_process(), Path::new(DRM_ROOT)))
}

fn verbose() -> bool {
    *VERBOSE.get_or_init(|| Env::from_process().is_verbose())
}

fn log(message: &str) {
    if verbose() {
        eprintln!("tauri-plugin-wayland-nvidia-quirk: {message}");
    }
}

fn set_status(status: Status) {
    if let Ok(mut slot) = STATUS.write() {
        *slot = status;
    }
}

pub(crate) fn status() -> Status {
    STATUS
        .read()
        .map(|slot| slot.clone())
        .unwrap_or(Status::NotRun)
}

/// What the detection matched on, carried to whichever window arrives first.
#[derive(Clone)]
struct Matched {
    gpu: String,
    driver: String,
    session: SessionType,
}

impl Matched {
    fn status(&self) -> Status {
        Status::Applied {
            gpu: self.gpu.clone(),
            driver: self.driver.clone(),
            session: self.session,
        }
    }
}

/// Runs from the plugin's setup hook, which Tauri calls from `Builder::build()`
/// *before* it creates the windows in `tauri.conf.json`.
pub(crate) fn setup<R: Runtime>(app: &AppHandle<R>) {
    match decision() {
        Decision::Overridden(by) => {
            log(&format!("standing down, {by} is set"));
            set_status(Status::Overridden { by: by.clone() });
        }
        Decision::NotAffected(reason) => {
            log(&format!("not affected ({reason:?}), nothing to do"));
            set_status(Status::NotAffected { reason: *reason });
        }
        Decision::Apply {
            gpu,
            driver,
            session,
        } => {
            log(&format!(
                "affected system (gpu {gpu}, driver {driver}, session {session:?}), forcing an early GL context"
            ));
            let matched = Matched {
                gpu: gpu.clone(),
                driver: driver.clone(),
                session: *session,
            };
            watch_application_windows(&matched);

            // nothing here yet in the normal case; harmless if a host app
            // built a window before registering the plugin
            for (label, window) in app.webview_windows() {
                match apply(&window) {
                    Ok(()) => log(&format!("window {label:?}: already open, armed directly")),
                    Err(error) => log(&format!("window {label:?}: {error}")),
                }
            }
        }
    }
}

/// Arms every window the process will ever open, at the only moment that is
/// both early enough and reliable.
fn watch_application_windows(matched: &Matched) {
    let Some(application) = gio::Application::default() else {
        let error = "no GtkApplication in this process".to_string();
        log(&format!("cannot watch for windows: {error}"));
        set_status(Status::Failed { error });
        return;
    };
    let application = match application.downcast::<gtk::Application>() {
        Ok(application) => application,
        Err(_) => {
            let error = "the default GApplication is not a GtkApplication".to_string();
            log(&format!("cannot watch for windows: {error}"));
            set_status(Status::Failed { error });
            return;
        }
    };

    let matched = matched.clone();
    application.connect_window_added(move |application, window| {
        // tao sets the title after construction, so there is nothing to name
        // the window by yet; its position in the application is all we have
        let nth = application.windows().len();
        match force_paint_gl_context(window, &matched) {
            Ok(()) => log(&format!("window {nth}: armed before its first frame")),
            Err(error) => log(&format!("window {nth}: {error}")),
        }
    });
}

/// Applies the quirk fix to one window.
pub(crate) fn apply<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), Error> {
    let matched = match decision() {
        Decision::Overridden(by) => {
            set_status(Status::Overridden { by: by.clone() });
            return Ok(());
        }
        Decision::NotAffected(reason) => {
            set_status(Status::NotAffected { reason: *reason });
            return Ok(());
        }
        Decision::Apply {
            gpu,
            driver,
            session,
        } => Matched {
            gpu: gpu.clone(),
            driver: driver.clone(),
            session: *session,
        },
    };

    let gtk_window = match window.gtk_window() {
        Ok(gtk_window) => gtk_window,
        Err(error) => {
            let error = Error::Window(error.to_string());
            set_status(Status::Failed {
                error: error.to_string(),
            });
            return Err(error);
        }
    };

    force_paint_gl_context(&gtk_window, &matched)
}

/// GTK3 picks GL or shared memory per frame based on whether the window already
/// has a paint GL context. Creating one up front makes every frame a GL frame,
/// so the shared memory buffer the compositor rejects is never attached.
fn force_paint_gl_context<W>(window: &W, matched: &Matched) -> Result<(), Error>
where
    W: IsA<gtk::Widget>,
{
    if window.is_realized() {
        return record(create_gl_context(window), matched);
    }

    // "realize" is G_SIGNAL_RUN_FIRST, so the GdkWindow exists by the time this
    // runs, and it still lands before the first frame
    let matched = matched.clone();
    window.connect_realize(move |window| {
        if let Err(error) = record(create_gl_context(window), &matched) {
            log(&format!("on realize: {error}"));
        }
    });
    Ok(())
}

fn record(result: Result<(), Error>, matched: &Matched) -> Result<(), Error> {
    match &result {
        Ok(()) => set_status(matched.status()),
        Err(error) => set_status(Status::Failed {
            error: error.to_string(),
        }),
    }
    result
}

fn create_gl_context<W>(window: &W) -> Result<(), Error>
where
    W: IsA<gtk::Widget>,
{
    let gdk_window = window
        .window()
        .ok_or_else(|| Error::Window("the widget has no GdkWindow".to_string()))?;

    // the returned context is dropped straight away; only the paint context
    // this forces GDK to create as a side effect matters
    gdk_window
        .create_gl_context()
        .map_err(|error| Error::GlContext(error.to_string()))?;
    Ok(())
}
