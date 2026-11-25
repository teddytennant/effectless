#!/usr/bin/env bash
# Run tern in a headless sway session and save a screenshot.
# usage: scripts/shot.sh out.png [seconds] [-- tern args]
set -euo pipefail
out=${1:?out.png}; secs=${2:-6}; shift 2 || true
[ "${1:-}" = "--" ] && shift
root=$(cd "$(dirname "$0")/.." && pwd)
rt=$(mktemp -d); chmod 700 "$rt"
cfg="$rt/sway.cfg"
printf 'output HEADLESS-1 resolution %s\ndefault_border none\nfor_window [app_id=".*"] floating enable, resize set 1440 900, move position 0 0\n' "${TERN_SHOT_RES:-1440x900}" > "$cfg"
export XDG_RUNTIME_DIR=$rt WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
unset WAYLAND_DISPLAY DISPLAY
nix run nixpkgs#sway -- -c "$cfg" >"$rt/sway.log" 2>&1 &
sway_pid=$!
for _ in $(seq 50); do ls "$rt"/wayland-* >/dev/null 2>&1 && break; sleep 0.2; done
export WAYLAND_DISPLAY=$(basename "$(ls "$rt"/wayland-? | head -1)")
"$root/target/debug/tern" "$@" >"$rt/tern.log" 2>&1 &
tern_pid=$!
sleep "$secs"
nix run nixpkgs#grim -- "$out"
kill $tern_pid $sway_pid 2>/dev/null || true
tail -5 "$rt/tern.log"
rm -rf "$rt"
