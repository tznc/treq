#!/usr/bin/env bash
# Copies the Treq app icons (src-tauri/icons/{android,ios}, from `npm run
# icons:generate`) into the generated mobile projects, which `tauri android|ios
# init` creates with Tauri's default icon. Run after init; skips a platform
# whose project has not been generated.
#
#   mobile-gpui/scripts/apply-icons.sh
set -euo pipefail

APP=$(cd "$(dirname "$0")/.." && pwd)
ICONS=$APP/../src-tauri/icons

res=$APP/gen/android/app/src/main/res
if [ -d "$res" ]; then
  cp -R "$ICONS"/android/. "$res"/
  echo "applied Android icons to $res"
fi

appicon=$APP/gen/apple/Assets.xcassets/AppIcon.appiconset
if [ -d "$appicon" ]; then
  # App Store Connect rejects an app icon with an alpha channel (ITMS-90717).
  # PNG colour types 4 and 6 carry one.
  for icon in "$ICONS"/ios/*.png; do
    case $(od -An -tu1 -j25 -N1 "$icon" | tr -d ' ') in
      4 | 6)
        echo "error: $icon has an alpha channel; run \`node scripts/flatten-ios-icons.js\`" >&2
        exit 1
        ;;
    esac
  done
  cp "$ICONS"/ios/*.png "$appicon"/
  echo "applied iOS icons to $appicon"
fi
