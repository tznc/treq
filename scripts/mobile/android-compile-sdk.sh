#!/usr/bin/env bash
# Compiles every Android library module against API 36, which `tauri-android`
# requires of the modules that depend on it. Some plugins (tauri-plugin-web-auth
# 1.0.0) still set compileSdk 34, which fails checkReleaseAarMetadata. Only the
# compile SDK changes: minSdk and targetSdk stay as each module sets them.
# `tauri android init` recreates the root build file, so run this after init.
#
#   scripts/mobile/android-compile-sdk.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../src-tauri/gen/android" && pwd)/build.gradle.kts
MARKER="// treq: android-compile-sdk"

grep -qF "$MARKER" "$ROOT" && exit 0
cat >> "$ROOT" <<KTS

$MARKER
subprojects {
    afterEvaluate {
        extensions.findByType(com.android.build.gradle.LibraryExtension::class.java)
            ?.compileSdk = 36
    }
}
KTS
