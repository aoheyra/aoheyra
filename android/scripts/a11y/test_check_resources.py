#!/usr/bin/env python3
"""AND-21: tests for the resource checker."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_resources import CHANNELS, DOCS, KOTLIN_DIR, NIGHT_THEME, REPO, STRINGS, check

MINIMAL_STRINGS = """<?xml version="1.0" encoding="utf-8"?>
<resources>
    <string name="app_name" translatable="false">aoheyra</string>
    <string name="channel_name">Mesh</string>
    <string name="channel_description">Mesh channel</string>
    <string name="cd_screen">Main screen</string>
</resources>
"""
MINIMAL_KOTLIN = """package com.aoheyra.app
val name = R.string.channel_name
val description = R.string.channel_description
val description2 = R.string.cd_screen
"""


def write_tree(root: Path, *, strings: str = MINIMAL_STRINGS, kotlin: str = MINIMAL_KOTLIN) -> None:
    (root / STRINGS).parent.mkdir(parents=True, exist_ok=True)
    (root / STRINGS).write_text(strings, encoding="utf-8")
    (root / NIGHT_THEME).parent.mkdir(parents=True, exist_ok=True)
    (root / NIGHT_THEME).write_text(
        '<resources><style name="Theme.AoHeyra" /></resources>', encoding="utf-8"
    )
    source = root / KOTLIN_DIR / "com/aoheyra/app/MainActivity.kt"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_text(kotlin, encoding="utf-8")
    channel = root / CHANNELS
    channel.parent.mkdir(parents=True, exist_ok=True)
    channel.write_text(
        'const val MESH_SERVICE_ID = "mesh_service"\n'
        'const val MESSAGES_ID = "messages"\n'
        'const val LOCATION_SHARING_ID = "location_sharing"\n',
        encoding="utf-8",
    )
    docs = root / DOCS
    docs.parent.mkdir(parents=True, exist_ok=True)
    docs.write_text(
        "`mesh_service` `messages` `location_sharing`", encoding="utf-8"
    )


class ResourceCheckTest(unittest.TestCase):
    def test_checkout_is_clean(self) -> None:
        self.assertEqual(check(REPO), [])

    def test_minimal_tree_is_clean(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            write_tree(Path(tmp))
            self.assertEqual(check(Path(tmp)), [])

    def test_missing_and_hardcoded_strings_are_found(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            write_tree(
                Path(tmp),
                kotlin=MINIMAL_KOTLIN + '\nText("Hello")\nval bad = R.string.missing\n',
            )
            errors = check(Path(tmp))
            self.assertTrue(any("hard-coded UI text" in error for error in errors))
            self.assertTrue(any("undefined string missing" in error for error in errors))

    def test_translatable_flag_is_enforced(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            write_tree(
                Path(tmp),
                strings=MINIMAL_STRINGS.replace(
                    '<string name="channel_name">',
                    '<string name="channel_name" translatable="false">',
                ),
            )
            errors = check(Path(tmp))
            self.assertTrue(any("unexpected non-translatable" in error for error in errors))

    def test_dark_theme_is_enforced(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            write_tree(Path(tmp))
            (Path(tmp) / NIGHT_THEME).unlink()
            errors = check(Path(tmp))
            self.assertTrue(any("dark theme missing" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
