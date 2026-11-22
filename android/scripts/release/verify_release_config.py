#!/usr/bin/env python3
"""AND-22: verify the release configuration without exposing signing material."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
BUILD_GRADLE = Path("android/app/build.gradle.kts")
RULES = Path("android/app/proguard-rules.pro")
MANIFEST = Path("android/app/src/main/AndroidManifest.xml")
DOCS = Path("android/docs/release.md")
SCRIPT = Path("android/scripts/release/build-release.sh")
ABIS = ("arm64-v8a", "armeabi-v7a", "x86_64")
SECRET_VARS = (
    "ANDROID_KEYSTORE_PATH",
    "ANDROID_KEYSTORE_PASSWORD",
    "ANDROID_KEY_ALIAS",
    "ANDROID_KEY_PASSWORD",
)
KEEP_PATTERNS = (
    "-keep class com.aoheyra.core.** { *; }",
    "-keep interface com.aoheyra.core.** { *; }",
    "-keep class * implements com.aoheyra.core.UniFFILib { *; }",
    "-keep class com.sun.jna.** { *; }",
    "native <methods>;",
)
FORBIDDEN_SDK = re.compile(r"firebase|crashlytics|sentry|acra", re.IGNORECASE)


def check(root: Path = REPO) -> list[str]:
    errors: list[str] = []
    gradle = (root / BUILD_GRADLE).read_text(encoding="utf-8")
    for needle in ("isMinifyEnabled = true", "isShrinkResources = true", "signingConfigs", "splits {"):
        if needle not in gradle:
            errors.append(f"build.gradle.kts is missing: {needle}")
    for abi in ABIS:
        if abi not in gradle:
            errors.append(f"ABI split missing: {abi}")
    for variable in SECRET_VARS:
        if variable not in gradle or variable not in (root / SCRIPT).read_text(encoding="utf-8"):
            errors.append(f"signing variable not wired: {variable}")

    rules = (root / RULES).read_text(encoding="utf-8")
    for needle in KEEP_PATTERNS:
        if needle not in rules:
            errors.append(f"R8 keep rule missing: {needle}")

    manifest = (root / MANIFEST).read_text(encoding="utf-8")
    if 'android:allowBackup="false"' not in manifest:
        errors.append("auto-backup must stay disabled")
    if "android.permission.INTERNET" in manifest:
        errors.append("release manifest must not declare INTERNET")
    if "android.permission.WAKE_LOCK" in manifest:
        errors.append("release manifest must not declare WAKE_LOCK")

    docs = (root / DOCS).read_text(encoding="utf-8")
    for needle in (*SECRET_VARS, "Play Store", "open-source store", "apksigner verify"):
        if needle not in docs:
            errors.append(f"release notes missing: {needle}")

    for gradle_file in (root / "android").rglob("*.gradle.kts"):
        text = gradle_file.read_text(encoding="utf-8")
        if FORBIDDEN_SDK.search(text):
            errors.append(f"uploading crash SDK in {gradle_file.relative_to(root)}")

    script = (root / SCRIPT).read_text(encoding="utf-8")
    if "echo" in script and any(f'echo "${{{variable}}}"' in script for variable in SECRET_VARS):
        errors.append("build script prints signing material")
    return errors


def check_tracked_keystores(root: Path = REPO) -> list[str]:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "ls-files"],
            check=True,
            capture_output=True,
            text=True,
            timeout=60,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        return [f"cannot inspect tracked files: {error}"]
    return [
        f"tracked signing material: {name}"
        for name in result.stdout.splitlines()
        if name.endswith((".jks", ".keystore", ".p12")) or name.endswith("local.properties")
    ]


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print("usage: verify_release_config.py", file=sys.stderr)
        return 2
    errors = check() + check_tracked_keystores()
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    print("release configuration: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
