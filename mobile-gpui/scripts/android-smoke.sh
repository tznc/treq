#!/usr/bin/env bash
# Installs the treq-gpui debug APK on a running emulator, opens the home and
# add-host screens, and checks the app renders without crashing. Screenshots
# and logcat go to the output directory.
#
#   mobile-gpui/scripts/android-smoke.sh <app.apk> [output-dir]
set -euo pipefail

APK=$1
OUT=${2:-target/android-smoke}
PKG=dev.treq.gpui
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
adb install -r -g "$APK"
adb logcat -c
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

# "Add SSH host" sits below the account and remote cards; scroll and tap it
# by its approximate position, then come back with the back button.
dismiss_dialogs
size=$(adb shell wm size | awk '/Physical/ {print $3}' | tr -d '\r')
width=${size%x*}
height=${size#*x}
adb shell input swipe $((width / 2)) $((height * 80 / 100)) $((width / 2)) $((height * 30 / 100)) 300
sleep 2
shot 02-home-scrolled
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
