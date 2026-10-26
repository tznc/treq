#!/usr/bin/env bash
# Copies the Treq app icons (src-tauri/icons/{android,ios}, from `npm run
# icons:generate`) into the generated mobile projects, which `tauri android|ios
# init` creates with Tauri's default icon. Run after init; skips a platform
# whose project has not been generated.
#
#   scripts/mobile/apply-icons.sh
set -euo pipefail

APP=$(cd "$(dirname "$0")/../../src-tauri" && pwd)
ICONS=$APP/icons

res=$APP/gen/android/app/src/main/res
if [ -d "$res" ]; then
  cp -R "$ICONS"/android/. "$res"/
  echo "applied Android icons to $res"
fi

appicon=$APP/gen/apple/Assets.xcassets/AppIcon.appiconset
if [ -d "$appicon" ]; then
  cp "$ICONS"/ios/*.png "$appicon"/
  echo "applied iOS icons to $appicon"
fi
