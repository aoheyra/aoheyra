#!/usr/bin/env python3
"""AND-21: check accessibility and i18n resources without an Android SDK."""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
STRINGS = Path("android/app/src/main/res/values/strings.xml")
NIGHT_THEME = Path("android/app/src/main/res/values-night/themes.xml")
KOTLIN_DIR = Path("android/app/src/main/kotlin")
CHANNELS = Path("android/app/src/main/kotlin/com/aoheyra/app/notifications/NotificationChannels.kt")
DOCS = Path("android/docs/accessibility-i18n.md")
NON_TRANSLATABLE_ALLOWED = {"app_name"}


def check(root: Path = REPO) -> list[str]:
    errors: list[str] = []
    strings_path = root / STRINGS
    if not strings_path.is_file():
        return [f"missing {STRINGS}"]
    strings = strings_path.read_text(encoding="utf-8")
    declared = re.findall(r'<string name="([^"]+)"', strings)
    duplicates = sorted({name for name in declared if declared.count(name) > 1})
    if duplicates:
        errors.append(f"duplicate string names: {duplicates}")

    non_translatable = {
        match.group(1)
        for match in re.finditer(
            r'<string name="([^"]+)"[^>]*translatable="false"', strings
        )
    }
    for name in sorted(non_translatable - NON_TRANSLATABLE_ALLOWED):
        errors.append(f"unexpected non-translatable string: {name}")

    kotlin_files = sorted((root / KOTLIN_DIR).rglob("*.kt"))
    if not kotlin_files:
        errors.append(f"no Kotlin sources under {KOTLIN_DIR}")
    for source in kotlin_files:
        text = source.read_text(encoding="utf-8")
        for name in set(re.findall(r"R\.string\.([A-Za-z0-9_]+)", text)):
            if name not in declared:
                errors.append(f"{source.relative_to(root)}: undefined string {name}")
        for literal in re.findall(r'Text\(\s*"([^"]+)"', text):
            errors.append(f"{source.relative_to(root)}: hard-coded UI text: {literal!r}")

    night = root / NIGHT_THEME
    if not night.is_file() or "Theme.AoHeyra" not in night.read_text(encoding="utf-8"):
        errors.append("dark theme missing from values-night/themes.xml")

    channel_text = (root / CHANNELS).read_text(encoding="utf-8")
    channel_ids = set(re.findall(r'const val [A-Z_]+_ID = "([a-z_]+)"', channel_text))
    if len(channel_ids) != 3:
        errors.append(f"expected three notification channel ids, got {sorted(channel_ids)}")
    docs = (root / DOCS).read_text(encoding="utf-8")
    for channel_id in sorted(channel_ids):
        if f"`{channel_id}`" not in docs:
            errors.append(f"channel {channel_id} is not documented")
    return errors


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print("usage: check_resources.py", file=sys.stderr)
        return 2
    errors = check()
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    print("accessibility/i18n resources: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
