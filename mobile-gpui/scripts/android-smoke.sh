#!/usr/bin/env bash
# Installs the treq-gpui debug APK on a running emulator, opens the signed-out
# landing page, swipes it, and checks the app renders without crashing. Screenshots
# and logcat go to the output directory.
#
#   mobile-gpui/scripts/android-smoke.sh <app.apk> [output-dir]
set -euo pipefail

APK=$1
OUT=${2:-target/android-smoke}
PKG=com.treq
mkdir -p "$OUT"
# Bound every adb call so a wedged device fails the run instead of hanging it.
ADB_BIN=$(command -v adb)
adb() { timeout 90 "$ADB_BIN" "$@"; }
# System ANR dialogs ("Pixel Launcher isn't responding") cover the app and eat
# `input` events, so close them before interacting.
dismiss_dialogs() { adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS > /dev/null || true; }

timeout 300 "$ADB_BIN" wait-for-device
adb shell settings put global hide_error_dialogs 1 || true
adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS > /dev/null || true
adb logcat -c
adb install -r -g "$APK"
# Let the install settle before the first launch. The system's post-install
# handling (PACKAGE_ADDED broadcast, `onPackageAdded` in the launcher) relaunches
# an activity that is already running, and tauri-plugin-gpui does not yet
# re-attach to a recreated activity, so the screen would go blank. Both waits
# are best-effort: `wait-for-broadcast-idle` needs API 30+, the logcat wait is
# capped at 20 s.
adb shell am wait-for-broadcast-idle || sleep 5
for _ in $(seq 20); do
  if adb logcat -d | grep "onPackageAdded: $PKG" > /dev/null; then break; fi
  sleep 1
done
# Keep a live capture so the logs survive an emulator crash.
"$ADB_BIN" logcat > "$OUT/logcat-stream.txt" 2>&1 &
LOGCAT_PID=$!
trap 'kill "$LOGCAT_PID" 2>/dev/null || true' EXIT
adb shell am start -n "$PKG/.MainActivity"

failures=0
shot() { adb exec-out screencap -p > "$OUT/$1.png"; }
wait_for() {
  local deadline=$((SECONDS + $2))
  while (( SECONDS < deadline )); do
    if adb logcat -d | grep -Eq "$1"; then return 0; fi
    sleep 1
  done
  echo "FAIL: no log line matching /$1/ within $2 s"
  failures=$((failures + 1))
  return 1
}

if ! wait_for "treq-gpui: attached" 120; then
  # Keep the evidence when the app never attached, then carry on to the checks.
  shot 00-not-attached || true
  adb logcat -d > "$OUT/logcat.txt" || true
fi
# The plugin logs this once GPUI has a GPU surface; without it the screen is blank.
if ! wait_for "tauri-plugin-gpui: GPUI attached" 60; then
  shot 00-gpui-not-attached || true
  adb logcat -d > "$OUT/logcat.txt" || true
fi
sleep 6
shot 01-home

# The signed-out landing page fits on one screen (no saved hosts on a fresh
# install), so the swipe only checks that scrolling it is harmless. Tapping
# "Sign in" would leave the app for the browser, so it is not exercised.
dismiss_dialogs
size=$(adb shell wm size | awk '/Physical/ {print $3}' | tr -d '\r')
width=${size%x*}
height=${size#*x}
adb shell input swipe $((width / 2)) $((height * 80 / 100)) $((width / 2)) $((height * 30 / 100)) 300
sleep 2
shot 02-home-scrolled
adb shell input keyevent 4
sleep 2

# Open "Use your own SSH host", the bottom landing button (24 dp page padding,
# 52 dp tall), for side-by-side review with the desktop add-host dialog. GPUI
# has no accessibility tree, so the tap is positional; the app draws
# edge-to-edge, so the offset is from the bottom of the screen. The window dump
# is kept to correct the offset if the layout moves.
# Back on the landing page closes the app, so bring it back first.
adb shell am start -n "$PKG/.MainActivity"
sleep 4
adb shell dumpsys window > "$OUT/dumpsys-window.txt" || true
density=$(adb shell wm density | awk '/Physical/ {print $3}' | tr -d '\r')
dp() { echo $(( $1 * density / 160 )); }
dismiss_dialogs
adb shell input tap $((width / 2)) $((height - $(dp 50)))
sleep 3
shot 04-add-host-empty

# Fill the host and username (field centres in dp from the top of a 640 dp
# screen), then read the host key. github.com:22 is reachable from the
# emulator, so the probe returns a real fingerprint. Back hides the keyboard
# only while it is shown; otherwise it would leave the screen.
hide_keyboard() {
  if adb shell dumpsys input_method | grep -q "mInputShown=true"; then
    adb shell input keyevent 4
    sleep 1
  fi
}
type_into() {
  adb shell input tap $((width / 2)) "$(dp "$1")"
  sleep 1
  adb shell input text "$2"
  sleep 1
  hide_keyboard
}
type_into 258 github.com
type_into 412 git
shot 05-add-host-filled
adb shell input swipe $((width / 2)) $((height * 80 / 100)) $((width / 2)) $((height * 30 / 100)) 300
sleep 2
shot 06-add-host-filled-scrolled
# A 0 ms tap right after a scroll is dropped by GPUI; hold it briefly instead.
adb shell input swipe $((width / 2)) "$(dp 408)" $((width / 2)) "$(dp 408)" 120
sleep 20
shot 07-add-host-confirm
adb shell input swipe $((width / 2)) $((height * 80 / 100)) $((width / 2)) $((height * 30 / 100)) 300
sleep 2
shot 08-add-host-confirm-scrolled
hide_keyboard
adb shell input keyevent 4
sleep 2

# Background and foreground: the GPU surface is destroyed and recreated.
adb shell input keyevent 3
sleep 2
adb shell am start -n "$PKG/.MainActivity"
sleep 4
shot 03-resumed

adb logcat -d > "$OUT/logcat.txt"
if ! adb shell pidof "$PKG" > /dev/null; then
  echo "FAIL: app is not running"
  failures=$((failures + 1))
fi
if grep -E "FATAL EXCEPTION|panicked at|attaching GPUI failed" "$OUT/logcat.txt"; then
  echo "FAIL: crash in logcat"
  failures=$((failures + 1))
fi
echo "treq-gpui smoke test: $failures failure(s)"
exit $(( failures > 0 ))
