#!/usr/bin/env bash
# AND-22: build signed release artefacts. Never prints signing material.
set -euo pipefail

for variable in ANDROID_KEYSTORE_PATH ANDROID_KEYSTORE_PASSWORD ANDROID_KEY_ALIAS ANDROID_KEY_PASSWORD; do
    if [ -z "${!variable:-}" ]; then
        echo "missing required environment variable: $variable" >&2
        exit 2
    fi
done
[ -f "$ANDROID_KEYSTORE_PATH" ] || { echo "keystore file not found" >&2; exit 3; }

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
android_dir="$(cd "$script_dir/../.." && pwd)"

cd "$android_dir"
./gradlew clean assembleRelease bundleRelease
cd "$android_dir/.."

python3 android/scripts/release/verify_release_config.py
find android/app/build/outputs -type f \( -name '*.apk' -o -name '*.aab' \) -print0 \
    | sort -z | xargs -0 -r sha256sum
