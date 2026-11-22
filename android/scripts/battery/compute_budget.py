#!/usr/bin/env python3
"""AND-19: check a Battery Historian run against the recorded budget.

The budget is machine-readable in android/docs/battery-budget.json. When it or
the Kotlin profile constants drift, check_sync() reports the mismatch.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[3]
BUDGET = REPO / "android/docs/battery-budget.json"
KOTLIN = REPO / "android/app/src/main/kotlin/com/aoheyra/app/power/BatteryProfiles.kt"
MANIFEST = REPO / "android/app/src/main/AndroidManifest.xml"
DOCS = REPO / "android/docs/battery.md"
FIELD_NAMES = (
    "scanWindowMs",
    "scanIntervalMs",
    "advertiseIntervalMs",
    "syncDwellMs",
    "maxContinuousWakeMs",
)
KOTLIN_NAMES = {
    "balanced": "balanced",
    "low_power": "lowPower",
    "performance": "performance",
}


def load_budget(path: Path = BUDGET) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def validate_metrics(metrics: dict[str, Any], budget: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    for field in ("startBattery", "endBattery", "hours"):
        if field not in metrics:
            errors.append(f"missing field: {field}")
    if errors:
        return errors
    start, end, hours = metrics["startBattery"], metrics["endBattery"], metrics["hours"]
    if not isinstance(start, int) or not isinstance(end, int) or not isinstance(hours, int):
        return ["startBattery, endBattery and hours must be integers"]
    if not 0 <= start <= 100 or not 0 <= end <= 100:
        errors.append("battery levels must be percentages between 0 and 100")
    if end > start:
        errors.append("endBattery must not exceed startBattery")
    if hours not in budget["runHours"]:
        errors.append(f"hours must be one of {budget['runHours']}")
    screen_on = metrics.get("screenOnMinutes", 0)
    if not isinstance(screen_on, int) or screen_on < 0:
        errors.append("screenOnMinutes must be a non-negative integer")
    elif screen_on > 0:
        errors.append("screen was on: this is not a screen-off run")
    return errors


def evaluate(metrics: dict[str, Any], budget: dict[str, Any]) -> dict[str, Any]:
    drain_per_hour = (metrics["startBattery"] - metrics["endBattery"]) / metrics["hours"]
    return {
        "drainPercentPerHour": round(drain_per_hour, 3),
        "targetPercentPerHour": budget["targetPercentPerHour"],
        "withinBudget": drain_per_hour <= budget["targetPercentPerHour"],
    }


def parse_kotlin_profiles(text: str) -> dict[str, dict[str, int]]:
    profiles: dict[str, dict[str, int]] = {}
    for key, kotlin_name in KOTLIN_NAMES.items():
        pattern = rf"val {kotlin_name} = BatteryProfile\((.*?)\n    \)"
        match = re.search(pattern, text, re.DOTALL)
        if match is None:
            continue
        values: dict[str, int] = {}
        for field in FIELD_NAMES:
            found = re.search(rf"{field} = ([\d_]+)L", match.group(1))
            if found is not None:
                values[field] = int(found.group(1).replace("_", ""))
        profiles[key] = values
    return profiles


def compare_profiles(parsed: dict[str, dict[str, int]], budget: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    expected = budget["profiles"]
    if set(parsed) != set(expected):
        return [f"profile keys differ: Kotlin {sorted(parsed)}, budget {sorted(expected)}"]
    for key in expected:
        for field in FIELD_NAMES:
            if parsed[key].get(field) != expected[key][field]:
                errors.append(
                    f"{key}.{field}: Kotlin {parsed[key].get(field)} != "
                    f"budget {expected[key][field]}"
                )
    return errors


def check_sync(root: Path = REPO, manifest_path: Path | None = None) -> list[str]:
    budget = load_budget(root / BUDGET.relative_to(root))
    parsed = parse_kotlin_profiles(
        (root / KOTLIN.relative_to(root)).read_text(encoding="utf-8")
    )
    errors = compare_profiles(parsed, budget)
    kotlin_text = (root / KOTLIN.relative_to(root)).read_text(encoding="utf-8")
    default_key = KOTLIN_NAMES.get(budget["defaultProfile"])
    if default_key is None or f"val default: BatteryProfile = {default_key}" not in kotlin_text:
        errors.append("Kotlin default profile does not match the budget")
    manifest_path = manifest_path or root / MANIFEST.relative_to(root)
    manifest = manifest_path.read_text(encoding="utf-8")
    if "android.permission.WAKE_LOCK" in manifest:
        errors.append("wake-lock policy violated: manifest declares WAKE_LOCK")
    docs = (root / DOCS.relative_to(root)).read_text(encoding="utf-8")
    if f"{budget['targetPercentPerHour']:.1f} %" not in docs:
        errors.append("budget table does not show the target from battery-budget.json")
    return errors


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: compute_budget.py <metrics.json>", file=sys.stderr)
        return 2
    metrics_path = Path(argv[1])
    with metrics_path.open(encoding="utf-8") as handle:
        metrics = json.load(handle)
    budget = load_budget()
    errors = validate_metrics(metrics, budget) + check_sync()
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    result = evaluate(metrics, budget)
    print(f"drain {result['drainPercentPerHour']} %/h; target {result['targetPercentPerHour']} %/h")
    return 0 if result["withinBudget"] else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
