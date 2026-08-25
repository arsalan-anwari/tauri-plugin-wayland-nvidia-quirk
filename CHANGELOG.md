# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

### Added

- `init()`, a Tauri v2 plugin that forces a GL paint context on the app's windows before the first frame, fixing `Gdk-Message: Error 71` on Wayland with the Nvidia kernel driver while keeping WebKit's DMA-BUF renderer and the driver's explicit sync path. Windows are caught through `GtkApplication::window-added`, which fires synchronously as each one is constructed; Tauri's own window hooks are queued on the event loop and land after the first frame.
- `apply()`, to force the quirk on a single window explicitly.
- `status()`, reporting whether the quirk applied, was not needed, was overridden, or failed.
- Detection from `/sys/class/drm` (sandbox-safe, hybrid-graphics aware) and from `GDK_BACKEND`/`XDG_SESSION_TYPE`/socket variables.
- `TAURI_WAYLAND_NVIDIA_QUIRK` and `TAURI_WAYLAND_NVIDIA_QUIRK_VERBOSE` overrides; stands down for `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
