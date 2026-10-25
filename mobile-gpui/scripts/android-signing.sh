#!/usr/bin/env bash
# Adds a release signing config to the generated Gradle project, which `tauri
# android init` recreates without one. Run after init. Reads the upload
# keystore from the environment:
#
#   ANDROID_KEYSTORE_BASE64  ANDROID_KEYSTORE_PASSWORD
#   ANDROID_KEY_ALIAS
#
# The key password is the keystore password (PKCS12 keystores have one).
#
#   mobile-gpui/scripts/android-signing.sh
set -euo pipefail

APP=$(cd "$(dirname "$0")/.." && pwd)
ANDROID=$APP/gen/android
GRADLE=$ANDROID/app/build.gradle.kts

: "${ANDROID_KEYSTORE_BASE64:?}" "${ANDROID_KEYSTORE_PASSWORD:?}"
: "${ANDROID_KEY_ALIAS:?}"

echo "$ANDROID_KEYSTORE_BASE64" | base64 --decode > "$ANDROID/upload.jks"
cat > "$ANDROID/keystore.properties" <<EOF
storeFile=$ANDROID/upload.jks
storePassword=$ANDROID_KEYSTORE_PASSWORD
keyAlias=$ANDROID_KEY_ALIAS
keyPassword=$ANDROID_KEYSTORE_PASSWORD
EOF

# Declare the config ahead of buildTypes, then point the release type at it.
# `Properties` is the generated file's java.util.Properties import: written
# out as `java.util.Properties`, `java` resolves to the Java plugin extension.
perl -0pi -e '
  s/^(\s*)buildTypes \{/$1signingConfigs {
$1    create("release") {
$1        val props = Properties().apply {
$1            rootProject.file("keystore.properties").inputStream().use { load(it) }
$1        }
$1        storeFile = file(props.getProperty("storeFile"))
$1        storePassword = props.getProperty("storePassword")
$1        keyAlias = props.getProperty("keyAlias")
$1        keyPassword = props.getProperty("keyPassword")
$1    }
$1}
$1buildTypes {/m or die "no buildTypes block\n";
  s/(getByName\("release"\) \{)/$1\n            signingConfig = signingConfigs.getByName("release")/ or die "no release build type\n";
' "$GRADLE"
echo "added release signing to $GRADLE"
