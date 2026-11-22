#!/usr/bin/env bash
# AND-20: collect redactable device evidence for one OEM matrix row.
# Raw dumps are written to ANDROID_OEM_EVIDENCE_DIR (default: ./oem-evidence,
# which is git-ignored). They must not be committed: only the distilled table
# in android/docs/oem-matrix.md is versioned.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <device-label>   (for example: pixel-7a)" >&2
    exit 2
fi

label="$1"
if ! command -v adb >/dev/null 2>&1; then
    echo "adb not found: install platform-tools and connect a physical phone" >&2
    exit 3
fi

out_dir="${ANDROID_OEM_EVIDENCE_DIR:-$(pwd)/oem-evidence}/$label"
mkdir -p "$out_dir"

adb shell getprop > "$out_dir/getprop.txt"
adb shell dumpsys bluetooth_manager > "$out_dir/bluetooth_manager.txt"
adb shell dumpsys package com.aoheyra.app > "$out_dir/package.txt"
adb shell dumpsys deviceidle whitelist > "$out_dir/deviceidle_whitelist.txt"

{
    echo "AND-20 evidence for $label"
    echo "captured_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "Raw dumps are in this directory; review and redact before quoting them."
    echo "connection_cap=NOT_MEASURED"
    echo "advertising_limit=NOT_MEASURED"
    echo "background_survival_8h=NOT_MEASURED"
    echo "workaround=NOT_MEASURED"
} > "$out_dir/session.txt"

echo "Evidence written to $out_dir (not versioned)"
