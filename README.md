# tauri-plugin-wayland-nvidia-quirk

[![Crates.io](https://img.shields.io/crates/v/tauri-plugin-wayland-nvidia-quirk)](https://crates.io/crates/tauri-plugin-wayland-nvidia-quirk)
[![Downloads](https://img.shields.io/crates/d/tauri-plugin-wayland-nvidia-quirk)](https://crates.io/crates/tauri-plugin-wayland-nvidia-quirk)
[![Docs.rs](https://docs.rs/tauri-plugin-wayland-nvidia-quirk/badge.svg)](https://docs.rs/tauri-plugin-wayland-nvidia-quirk)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Fixes the Wayland/Nvidia startup failure in Tauri v2 apps **without** disabling hardware acceleration.

```
Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.
```

## Do I need this?

Your app exits immediately or shows a blank window on startup, with that message on stderr. You are affected if all of these hold:

```sh
echo $XDG_SESSION_TYPE                    # wayland
cat /sys/class/drm/card*/device/vendor    # 0x10de somewhere
ls -l /sys/class/drm/card*/device/driver  # -> .../drivers/nvidia
```

Both the proprietary and the open kernel modules are affected. Nouveau, Intel and AMD are not (as far as has been [tested](https://github.com/arsalan-anwari/tauri-v2-wayland-nvidia-issue)).

## What it does

GTK decides at the start of every frame whether to draw with GL or into a shared-memory buffer, based on whether the window already has a paint GL context. WebKitGTK only creates one from inside the draw, so the first frame starts as a shared-memory frame and the window's EGL surface appears halfway through it. Nvidia's driver arms explicit sync as soon as that EGL surface exists; GTK then attaches the shared-memory buffer, which carries no acquire point, and the compositor drops the connection.

This plugin creates the GL context up front, so every frame is a GL frame and no shared-memory buffer is ever attached.

| | DMA-BUF renderer | Nvidia explicit sync | Scope |
|---|---|---|---|
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | **off** | on | process + children |
| `__NV_DISABLE_EXPLICIT_SYNC=1` | on | **off** | process + children |
| **this plugin** | on | on | one window, in-process |

## Install

```sh
cargo add tauri-plugin-wayland-nvidia-quirk
```

On non-affected targets the crate compiles to a no-op with no dependencies.

## Use

One line, should be the first alongside your other plugins:

```rust
// src-tauri/src/lib.rs
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_wayland_nvidia_quirk::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![/* ... */])
        .run(tauri::generate_context!())
        .expect("failed to start app");
}
```

### Windows created after startup

`init()` covers these too. It hooks the process's `GtkApplication`, which announces every window synchronously as GTK builds it, before the window is realized, and before the event loop draws a frame. 

To force the quirk on one window explicitly:

```rust
let window = tauri::WebviewWindowBuilder::new(app, "second", Default::default()).build()?;
tauri_plugin_wayland_nvidia_quirk::apply(&window)?;
```

Again, it is a no-op on unaffected systems and safe to call more than once.

## Checking that it worked

```rust
.setup(|app| {
    println!("quirk: {:?}", tauri_plugin_wayland_nvidia_quirk::status());
    Ok(())
})
```

```
quirk: Applied { gpu: "0x10de", driver: "nvidia", session: Wayland }
quirk: NotAffected { reason: NotNvidia }
quirk: NotAffected { reason: NotWayland }
quirk: Overridden { by: "WEBKIT_DISABLE_DMABUF_RENDERER=1" }
quirk: Failed { error: "no GL context for the window: ..." }
```

To confirm hardware acceleration survived:

```sh
WAYLAND_DEBUG=1 ./your-app 2>&1 | grep -c set_acquire_point               # > 0
WAYLAND_DEBUG=1 ./your-app 2>&1 | grep -c "wl_shm_pool.*create_buffer"    # 0
```

A correct run sets acquire points and allocates no shared-memory buffers.

## Try it

A demo app that prints what the plugin decided, and a script that runs it with and without the quirk and reads the protocol trace:

```sh
cargo run --manifest-path demos/minimal/Cargo.toml   # see it work
./demos/verify.sh                                    # measure it
```

See [demos/](demos/). No npm and no `tauri-cli` needed; not part of the published crate.

## Environment

| Variable | Effect |
|---|---|
| `TAURI_WAYLAND_NVIDIA_QUIRK=0` | Never apply, even if detected |
| `TAURI_WAYLAND_NVIDIA_QUIRK=1` | Always apply, skip detection |
| `TAURI_WAYLAND_NVIDIA_QUIRK_VERBOSE=1` | Log the decision and every signal to stderr |

## If it does not fix it

Fall back, in this order:

```sh
__NV_DISABLE_EXPLICIT_SYNC=1 ./your-app      # keeps DMA-BUF, drops explicit sync
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./your-app  # drops WebKit's GPU compositing
```

On a build you cannot recompile, GTK has a built-in equivalent that needs no code:

```sh
GDK_GL=always ./your-app
```

That forces the same early GL context, but for every window in the process, and in every child process that inherits the variable (including `WebKitWebProcess`), at roughly +5% peak RSS (based on results from [kana-trainer](https://github.com/arsalan-anwari/kana-trainer) app).

## Support

Please report failures with your `status()` output, `glxinfo -B`, and the driver version from `/proc/driver/nvidia/version`.

## Background

Root-cause analysis, six reproducers and annotated Wayland protocol traces: [tauri-v2-wayland-nvidia-issue](https://github.com/arsalan-anwari/tauri-v2-wayland-nvidia-issue).

Upstream issues: [tauri#10702](https://github.com/tauri-apps/tauri/issues/10702) · [webkit#280210](https://bugs.webkit.org/show_bug.cgi?id=280210) · [gtk#8056](https://gitlab.gnome.org/GNOME/gtk/-/issues/8056) · [egl-wayland#179](https://github.com/NVIDIA/egl-wayland/issues/179)

## License

Apache-2.0
