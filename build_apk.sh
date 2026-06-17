#!/usr/bin/env bash
# Build the Android APK for HORSE Poker (arm64). Requires:
#   * rustup target: aarch64-linux-android
#   * cargo-apk      (cargo install cargo-apk)
#   * Android SDK + NDK + a JDK
# Edit the paths below if your toolchain lives elsewhere.
set -euo pipefail

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}"
export JAVA_HOME="${JAVA_HOME:-$(ls -d "$HOME"/.jdks/* | sort -V | tail -1)/Contents/Home}"
BUILD_TOOLS="$(ls "$ANDROID_HOME"/build-tools | sort -V | tail -1)"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/build-tools/$BUILD_TOOLS:$PATH"

echo "NDK:  $ANDROID_NDK_ROOT"
echo "JDK:  $JAVA_HOME"

# Generate a throwaway self-signed dev keystore on first run (kept out of git).
if [ ! -f release.keystore ]; then
  echo "Generating release.keystore (dev key)..."
  "$JAVA_HOME/bin/keytool" -genkeypair -v -keystore release.keystore -alias horsepoker \
    -keyalg RSA -keysize 2048 -validity 10000 \
    -storepass android -keypass android -dname "CN=HORSE Poker, O=JacobStephens2, C=US"
fi

cargo apk build --lib --release

OUT="target/release/apk/bevy-uspsa.apk"
DEST="target/bevy-uspsa.apk"
cp "$OUT" "$DEST"
echo "APK:  $DEST"
