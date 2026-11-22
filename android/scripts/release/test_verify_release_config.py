#!/usr/bin/env python3
"""AND-22: tests for the release configuration gate."""

from __future__ import annotations

import shutil
import tempfile
import unittest
from pathlib import Path

from verify_release_config import (
    ABIS,
    BUILD_GRADLE,
    DOCS,
    MANIFEST,
    REPO,
    RULES,
    SCRIPT,
    SECRET_VARS,
    check,
    check_tracked_keystores,
)

FILES = (BUILD_GRADLE, RULES, MANIFEST, DOCS, SCRIPT)


def overlay_root(root: Path) -> None:
    for relative in FILES:
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(REPO / relative, target)
    # check() scans all Gradle files under android/.
    (root / "android/ble/build.gradle.kts").parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(
        REPO / "android/ble/build.gradle.kts",
        root / "android/ble/build.gradle.kts",
    )


class ReleaseConfigTest(unittest.TestCase):
    def test_checkout_is_clean(self) -> None:
        self.assertEqual(check(REPO), [])
        self.assertEqual(check_tracked_keystores(REPO), [])

    def test_all_abis_are_required(self) -> None:
        self.assertEqual(len(ABIS), 3)
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            root = Path(tmp)
            overlay_root(root)
            target = root / BUILD_GRADLE
            target.write_text(
                target.read_text(encoding="utf-8").replace(f'include("{ABIS[0]}",', "include("),
                encoding="utf-8",
            )
            self.assertTrue(any("ABI split missing" in e for e in check(root)))

    def test_uniffi_keep_rules_are_required(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            root = Path(tmp)
            overlay_root(root)
            target = root / RULES
            target.write_text("-keep class other.** { *; }\n", encoding="utf-8")
            errors = check(root)
            self.assertTrue(any("R8 keep rule missing" in e for e in errors))
            self.assertTrue(any("com.aoheyra.core" in e for e in errors))

    def test_manifest_forbids_network_and_backup(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            root = Path(tmp)
            overlay_root(root)
            target = root / MANIFEST
            target.write_text(
                '<manifest><uses-permission android:name="android.permission.INTERNET"/></manifest>',
                encoding="utf-8",
            )
            errors = check(root)
            self.assertTrue(any("INTERNET" in e for e in errors))
            self.assertTrue(any("auto-backup" in e for e in errors))

    def test_crash_sdk_is_forbidden(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            root = Path(tmp)
            overlay_root(root)
            target = root / "android/ble/build.gradle.kts"
            target.write_text('dependencies { implementation("com.google.firebase:crashlytics") }')
            errors = check(root)
            self.assertTrue(any("uploading crash SDK" in e for e in errors))

    def test_signing_variables_are_wired(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            root = Path(tmp)
            overlay_root(root)
            target = root / SCRIPT
            target.write_text(
                target.read_text(encoding="utf-8").replace(SECRET_VARS[-1], "UNUSED"),
                encoding="utf-8",
            )
            self.assertTrue(any("signing variable not wired" in e for e in check(root)))


if __name__ == "__main__":
    unittest.main()
