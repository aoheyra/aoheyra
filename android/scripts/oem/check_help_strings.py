#!/usr/bin/env python3
"""AND-20: ensure the OEM help screen only references real string resources."""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
KOTLIN = REPO / "android/app/src/main/kotlin/com/aoheyra/app/onboarding/OemHelp.kt"
SCREEN = REPO / "android/app/src/main/kotlin/com/aoheyra/app/ui/OemHelpScreen.kt"
STRINGS = REPO / "android/app/src/main/res/values/strings.xml"


def check(root: Path = REPO) -> list[str]:
    declared = set(
        re.findall(
            r'<string name="([^"]+)"',
            (root / STRINGS.relative_to(root)).read_text(encoding="utf-8"),
        )
    )
    referenced: set[str] = set()
    for source in (KOTLIN, SCREEN):
        referenced.update(
            re.findall(
                r"R\.string\.([A-Za-z0-9_]+)",
                (root / source.relative_to(root)).read_text(encoding="utf-8"),
            )
        )
    return sorted(f"undefined string resource: {name}" for name in referenced - declared)


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print("usage: check_help_strings.py", file=sys.stderr)
        return 2
    errors = check()
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    print("OEM help strings: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
