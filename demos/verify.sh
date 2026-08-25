#!/usr/bin/env bash
# Runs the demo app twice, once with the quirk disabled, once with it on and
# reports what the compositor saw.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$here/minimal"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

bold=$'\e[1m'; green=$'\e[32m'; red=$'\e[31m'; dim=$'\e[2m'; reset=$'\e[0m'
lifetime_ms="${DEMO_EXIT_AFTER_MS:-4000}"

echo "${bold}building${reset}"
cargo build --manifest-path "$app/Cargo.toml" --quiet || exit 1
binary="$app/target/debug/wayland-nvidia-quirk-demo"

run_case() {
  local label="$1"; shift
  local log="$out/$label.log"

  env WAYLAND_DEBUG=1 \
      TAURI_WAYLAND_NVIDIA_QUIRK_VERBOSE=1 \
      DEMO_EXIT_AFTER_MS="$lifetime_ms" \
      "$@" "$binary" >"$log" 2>&1
  echo "$?" >"$out/$label.status"
}

# counts, so the checks below and the report agree by construction
protocol_errors() { grep -c 'Error 71 (Protocol error)' "$out/$1.log"; }
acquire_points()  { grep -c 'set_acquire_point' "$out/$1.log"; }
shm_buffers()     { grep -cE 'wl_shm_pool.*create_buffer' "$out/$1.log"; }

report_case() {
  local label="$1" expectation="$2"
  local log="$out/$label.log"
  local status; status="$(cat "$out/$label.status")"
  local protocol_error acquire shm verdict

  protocol_error="$(protocol_errors "$label")"
  acquire="$(acquire_points "$label")"
  shm="$(shm_buffers "$label")"
  verdict="$(grep -m1 '^quirk: ' "$log" || echo 'quirk: (never printed)')"

  echo
  echo "${bold}$label${reset} ${dim}($expectation)${reset}"
  echo "  ${verdict#quirk: }"
  echo "  exit code ............... $status"
  echo "  Gdk protocol errors ..... $protocol_error"
  echo "  set_acquire_point ....... $acquire   ${dim}(GL frames)${reset}"
  echo "  wl_shm_pool create_buffer $shm   ${dim}(software frames)${reset}"
}

echo "${bold}run 1/3${reset}  quirk disabled ${dim}(TAURI_WAYLAND_NVIDIA_QUIRK=0)${reset}"
run_case control TAURI_WAYLAND_NVIDIA_QUIRK=0
echo "${bold}run 2/3${reset}  quirk enabled"
run_case quirk
echo "${bold}run 3/3${reset}  quirk enabled, plus a window created after startup"
run_case runtime_window DEMO_OPEN_SECOND_AFTER_MS=$((lifetime_ms / 2))

report_case control 'the bug, unmitigated'
report_case quirk 'the plugin doing its job'
report_case runtime_window 'a second window, opened later'

control_errors="$(protocol_errors control)"
quirk_errors="$(protocol_errors quirk)"
quirk_status="$(cat "$out/quirk.status")"
quirk_acquire="$(acquire_points quirk)"
quirk_shm="$(shm_buffers quirk)"
runtime_errors="$(protocol_errors runtime_window)"
runtime_status="$(cat "$out/runtime_window.status")"

echo
if grep -q '^quirk: NotAffected' "$out/quirk.log"; then
  echo "${bold}inconclusive${reset} — this machine does not have the bug, so there is"
  echo "nothing for the plugin to fix here. Both runs are expected to succeed."
  [ "$quirk_status" = 0 ] && exit 0 || exit 1
fi

fail=0
[ "$control_errors" -gt 0 ] || { echo "${red}✗${reset} the control run did not reproduce the bug"; fail=1; }
[ "$quirk_errors" -eq 0 ]   || { echo "${red}✗${reset} the app still hit the protocol error with the quirk on"; fail=1; }
[ "$quirk_status" = 0 ]     || { echo "${red}✗${reset} the app exited $quirk_status with the quirk on"; fail=1; }
[ "$quirk_acquire" -gt 0 ]  || { echo "${red}✗${reset} no acquire points: explicit sync is not in use"; fail=1; }
[ "$quirk_shm" -eq 0 ]      || { echo "${red}✗${reset} $quirk_shm shared-memory buffers: hardware acceleration was lost"; fail=1; }
[ "$runtime_errors" -eq 0 ] || { echo "${red}✗${reset} the window created after startup hit the protocol error"; fail=1; }
[ "$runtime_status" = 0 ]   || { echo "${red}✗${reset} the app exited $runtime_status after opening a second window"; fail=1; }

if [ "$fail" = 0 ]; then
  echo "${green}${bold}✓ verified${reset} — the bug reproduces without the plugin, and with it the app"
  echo "  starts, keeps explicit sync, and never falls back to software buffers —
  for the window from tauri.conf.json and for one opened later alike."
fi
exit "$fail"
