#!/usr/bin/env python3
"""AND-19: tests for the budget checker and the wake-lock policy check."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from compute_budget import (
    BUDGET,
    REPO,
    check_sync,
    compare_profiles,
    evaluate,
    load_budget,
    parse_kotlin_profiles,
    validate_metrics,
)


class BatteryBudgetTest(unittest.TestCase):
    def setUp(self) -> None:
        self.budget = load_budget()

    def test_budget_sync_is_clean(self) -> None:
        self.assertEqual(check_sync(REPO), [])

    def test_kotlin_profiles_are_parsed_and_match_budget(self) -> None:
        kotlin = (REPO / "android/app/src/main/kotlin/com/aoheyra/app/power/BatteryProfiles.kt")
        parsed = parse_kotlin_profiles(kotlin.read_text(encoding="utf-8"))
        self.assertEqual(set(parsed), set(self.budget["profiles"]))
        self.assertEqual(compare_profiles(parsed, self.budget), [])

    def test_profile_drift_is_detected(self) -> None:
        parsed = parse_kotlin_profiles(
            (REPO / "android/app/src/main/kotlin/com/aoheyra/app/power/BatteryProfiles.kt")
            .read_text(encoding="utf-8")
            .replace("scanWindowMs = 4_000L", "scanWindowMs = 9_000L")
        )
        errors = compare_profiles(parsed, self.budget)
        self.assertTrue(any("balanced.scanWindowMs" in error for error in errors))

    def test_valid_screen_off_run(self) -> None:
        metrics = {"startBattery": 82, "endBattery": 78, "hours": 8, "screenOnMinutes": 0}
        self.assertEqual(validate_metrics(metrics, self.budget), [])
        self.assertEqual(evaluate(metrics, self.budget)["drainPercentPerHour"], 0.5)
        self.assertTrue(evaluate(metrics, self.budget)["withinBudget"])

    def test_screen_on_run_is_invalid(self) -> None:
        metrics = {"startBattery": 82, "endBattery": 78, "hours": 8, "screenOnMinutes": 12}
        self.assertTrue(any("screen was on" in error for error in validate_metrics(metrics, self.budget)))

    def test_bad_metrics_are_rejected(self) -> None:
        cases = [
            {"startBattery": 50, "endBattery": 60, "hours": 8},
            {"startBattery": 101, "endBattery": 90, "hours": 8},
            {"startBattery": 82, "endBattery": 78, "hours": 7},
            {"startBattery": 82, "endBattery": 78},
            {"startBattery": 82, "endBattery": 78, "hours": 8, "screenOnMinutes": -1},
        ]
        for case in cases:
            with self.subTest(case=case):
                self.assertNotEqual(validate_metrics(case, self.budget), [])

    def test_wake_lock_permission_is_forbidden(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            root = Path(tmp)
            rel = "android/app/src/main/AndroidManifest.xml"
            target = root / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(
                '<manifest><uses-permission android:name="android.permission.WAKE_LOCK"/></manifest>',
                encoding="utf-8",
            )
            errors = check_sync(REPO, manifest_path=target)
            self.assertTrue(any("WAKE_LOCK" in error for error in errors))

    def test_budget_file_is_json_with_required_runs(self) -> None:
        self.assertEqual(self.budget["runHours"], [1, 8, 24])
        self.assertEqual(self.budget["defaultProfile"], "balanced")
        self.assertTrue((REPO / BUDGET.relative_to(REPO)).is_file())

    def test_metrics_file_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "metrics.json"
            metrics = {"startBattery": 90, "endBattery": 60, "hours": 24, "screenOnMinutes": 0}
            path.write_text(json.dumps(metrics), encoding="utf-8")
            loaded = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(validate_metrics(loaded, self.budget), [])
            self.assertTrue(evaluate(loaded, self.budget)["withinBudget"])


if __name__ == "__main__":
    unittest.main()
