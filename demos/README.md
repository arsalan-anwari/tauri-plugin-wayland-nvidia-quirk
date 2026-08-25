# Demos

## `minimal/`

The smallest Tauri v2 app that shows what the plugin decided: one window that
prints `status()`, the session variables and the contents of `/sys/class/drm`,
plus a button that opens a second window at runtime.

No npm, no `tauri-cli` and the frontend is one static HTML file:

```sh
cargo run --manifest-path demos/minimal/Cargo.toml
```

On an affected machine (Wayland + the `nvidia` kernel driver), turn the plugin
off to watch the bug happen:

```sh
TAURI_WAYLAND_NVIDIA_QUIRK=0 cargo run --manifest-path demos/minimal/Cargo.toml
# Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.
```

`TAURI_WAYLAND_NVIDIA_QUIRK_VERBOSE=1` prints every step of the decision.

## `verify.sh`

Runs the demo three times and reads the Wayland protocol traces.

```sh
./demos/verify.sh
```

| run | expected |
|---|---|
| quirk disabled | the app dies: `Error 71`, shared-memory buffers, no acquire points |
| quirk enabled | the app runs: no protocol error, acquire points, zero shared-memory buffers |
| quirk enabled, second window opened at runtime | same, for a window `tauri.conf.json` never declared |

Real output from a GeForce on the open kernel module, driver 610.57.04, KDE Plasma 6.7.4
on Wayland, WebKitGTK 2.52.5:

```
control          (the bug, unmitigated)
  Overridden { by: "TAURI_WAYLAND_NVIDIA_QUIRK=0" }
  exit code ............... 1
  Gdk protocol errors ..... 1
  set_acquire_point ....... 0     (GL frames)
  wl_shm_pool create_buffer 3     (software frames)

quirk            (the plugin doing its job)
  Applied { gpu: "0x10de", driver: "nvidia", session: Wayland }
  exit code ............... 0
  Gdk protocol errors ..... 0
  set_acquire_point ....... 221   (GL frames)
  wl_shm_pool create_buffer 0     (software frames)
```

## Environment the demos add

Only for unattended runs; the plugin itself ignores them.

| Variable | Effect |
|---|---|
| `DEMO_EXIT_AFTER_MS` | quit on a timer instead of waiting for a event |
| `DEMO_OPEN_SECOND_AFTER_MS` | open the runtime window on a timer |
