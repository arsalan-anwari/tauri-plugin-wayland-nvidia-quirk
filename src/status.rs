//! Types reported back to the host application.

use std::fmt;

/// The display server the process decided it is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    /// A Wayland session, where the bug lives.
    Wayland,
    /// An X11 session, including XWayland.
    X11,
    /// Nothing in the environment said either way.
    Unknown,
}

/// Why the quirk decided to stay out of the way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotAffectedReason {
    /// Built for a target that is not Linux.
    NotLinux,
    /// The session is X11, or no display server could be identified.
    NotWayland,
    /// No GPU bound to the `nvidia` kernel driver drives the display.
    NotNvidia,
}

/// What the quirk did, for diagnostics and bug reports.
///
/// Read it with [`crate::status`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    /// Nothing has run yet: the plugin's setup hook has not been reached.
    NotRun,
    /// A GL paint context was forced on at least one window.
    Applied {
        /// PCI vendor of the GPU that made this system a match, e.g. `0x10de`.
        gpu: String,
        /// Kernel driver bound to it, e.g. `nvidia`.
        driver: String,
        /// The display server that was detected.
        session: SessionType,
    },
    /// This system does not have the bug.
    NotAffected {
        /// Which check ruled this system out.
        reason: NotAffectedReason,
    },
    /// An environment variable told the quirk to stand down.
    Overridden {
        /// The variable and value that switched the quirk off, e.g.
        /// `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
        by: String,
    },
    /// The system matched but GDK would not hand over a GL context. The app
    /// will most likely still hit the protocol error.
    Failed {
        /// What GDK reported.
        error: String,
    },
}

impl Status {
    /// Whether the workaround is in effect.
    pub fn is_applied(&self) -> bool {
        matches!(self, Status::Applied { .. })
    }
}

/// Failure to apply the quirk to a specific window.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Error {
    /// Tauri could not hand back the underlying `gtk::ApplicationWindow`.
    Window(String),
    /// `gdk_window_create_gl_context()` failed.
    GlContext(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Window(e) => write!(f, "no GTK window for this Tauri window: {e}"),
            Error::GlContext(e) => write!(f, "no GL context for the window: {e}"),
        }
    }
}

impl std::error::Error for Error {}
