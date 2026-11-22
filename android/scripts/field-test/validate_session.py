#!/usr/bin/env python3
"""AND-23: validate and score one field-test session file.

Metrics come from diagnostics bundles exported on the phones; this script only
checks the recorded values and applies the targets written in the runbook.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[3]
PHASES = {"M3_SHORT", "M5_SHORT", "M7_FULL"}
SCENARIOS = {"outdoor", "corridor", "multi_floor", "moving_crowd", "spammer"}
SESSION_ID = re.compile(r"^(short-m[35]|full-m7)-[a-z0-9-]+$")
DELIVERY_TARGET = {"normal": 0.90, "spammer": 0.70}
LATENCY_TARGET_P95_MS = {"normal": 60_000, "spammer": 120_000}


def load(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        data = json.load(handle)
    if not isinstance(data, dict):
        raise ValueError("session file must contain a JSON object")
    return data


def _is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def validate_schema(session: dict[str, Any], degree_cap: int | None) -> list[str]:
    errors: list[str] = []
    status = session.get("status")
    if status not in {"PLANNED", "COMPLETED"}:
        return [f"status must be PLANNED or COMPLETED, got {status!r}"]
    if status == "PLANNED":
        for field in ("buildCommit", "deviceCount", "startedUtc", "endedUtc", "degreeCapConfigured"):
            if session.get(field) is not None:
                errors.append(f"PLANNED session must not contain {field}")
        if session.get("messages") or session.get("degreeSamples") or session.get("diagnosticsBundleIds"):
            errors.append("PLANNED session must not contain results")
        return errors

    if not SESSION_ID.fullmatch(str(session.get("sessionId", ""))):
        errors.append("bad sessionId")
    if session.get("phase") not in PHASES:
        errors.append("bad phase")
    if session.get("scenario") not in SCENARIOS:
        errors.append("bad scenario")
    if not isinstance(session.get("buildCommit"), str) or len(session["buildCommit"]) < 7:
        errors.append("buildCommit must be a full-enough git sha")

    devices = session.get("devices")
    count = session.get("deviceCount")
    if not _is_int(count) or not 5 <= count <= 10:
        errors.append("deviceCount must be between 5 and 10")
    if not isinstance(devices, list) or len(devices) != count:
        errors.append("devices length must equal deviceCount")
        devices = []
    labels: set[str] = set()
    for index, device in enumerate(devices):
        label = device.get("label") if isinstance(device, dict) else None
        if not isinstance(label, str) or not label:
            errors.append(f"device {index}: missing label")
        elif label in labels:
            errors.append(f"duplicate device label: {label}")
        else:
            labels.add(label)
        if not _is_int(device.get("androidVersion")) or device.get("vendor") not in {
            "pixel", "samsung", "xiaomi", "oneplus", "other"
        }:
            errors.append(f"device {index}: bad androidVersion or vendor")

    for field in ("startedUtc", "endedUtc"):
        try:
            datetime.fromisoformat(str(session.get(field, "")).replace("Z", "+00:00"))
        except ValueError:
            errors.append(f"{field} must be an ISO-8601 UTC timestamp")

    bundles = session.get("diagnosticsBundleIds")
    if not isinstance(bundles, list) or len(bundles) != count or not all(
        isinstance(item, str) and item for item in bundles
    ):
        errors.append("one diagnostics bundle id per device is required")

    configured_cap = session.get("degreeCapConfigured")
    if not _is_int(configured_cap) or configured_cap <= 0:
        errors.append("degreeCapConfigured must be a positive integer")
    elif degree_cap is not None and configured_cap != degree_cap:
        errors.append("degreeCapConfigured does not match --degree-cap")

    messages = session.get("messages")
    if not isinstance(messages, list) or not messages:
        errors.append("COMPLETED session needs synthetic message records")
        messages = []
    message_ids: set[str] = set()
    for index, message in enumerate(messages):
        identifier = message.get("id")
        if not isinstance(identifier, str) or not identifier or identifier in message_ids:
            errors.append(f"message {index}: missing or duplicate id")
        else:
            message_ids.add(identifier)
        if message.get("sentBy") not in labels:
            errors.append(f"message {index}: unknown sender")
        if not _is_int(message.get("sentAtMs")) or message["sentAtMs"] < 0:
            errors.append(f"message {index}: bad sentAtMs")
        receivers = message.get("receivedBy")
        arrivals = message.get("arrivalsMs")
        if not isinstance(receivers, list) or not isinstance(arrivals, list):
            errors.append(f"message {index}: receivedBy and arrivalsMs must be arrays")
            continue
        if len(receivers) != len(arrivals) or any(label not in labels for label in receivers):
            errors.append(f"message {index}: receivers and arrivals do not line up")
        if any(not _is_int(at) or at < message.get("sentAtMs", 0) for at in arrivals):
            errors.append(f"message {index}: arrival before send")

    samples = session.get("degreeSamples")
    if not isinstance(samples, list) or not samples or any(
        not _is_int(value) or value < 0 for value in samples
    ):
        errors.append("degreeSamples must be a non-empty array of counts")
    if isinstance(samples, list) and samples and _is_int(configured_cap) and max(samples) > configured_cap:
        errors.append("observed degree exceeds the configured cap")

    battery = session.get("battery")
    if not isinstance(battery, dict):
        errors.append("battery must be an object")
    else:
        for field in ("startPercent", "endPercent"):
            value = battery.get(field)
            if not _is_int(value) or not 0 <= value <= 100:
                errors.append(f"battery.{field} must be a percent")
        if _is_int(battery.get("startPercent")) and _is_int(battery.get("endPercent")):
            if battery["endPercent"] > battery["startPercent"]:
                errors.append("battery.endPercent exceeds startPercent")
        hours = battery.get("hours")
        if not isinstance(hours, (int, float)) or hours <= 0:
            errors.append("battery.hours must be positive")
    return errors


def percentile(values: list[int], fraction: float) -> int:
    """Nearest-rank percentile; no dependency and deterministic."""
    ordered = sorted(values)
    rank = math.ceil(fraction * len(ordered))
    return ordered[max(0, rank - 1)]


def compute_metrics(session: dict[str, Any], budget: dict[str, Any] | None) -> dict[str, Any]:
    scenario = session["scenario"]
    kind = "spammer" if scenario == "spammer" else "normal"
    messages = session["messages"]
    latencies = [
        arrival - message["sentAtMs"]
        for message in messages
        for arrival in message["arrivalsMs"]
    ]
    delivered = sum(1 for message in messages if message["arrivalsMs"])
    ratio = delivered / len(messages) if messages else 0.0
    p95 = percentile(latencies, 0.95) if latencies else -1
    max_degree = max(session["degreeSamples"])
    battery = session["battery"]
    drain = (battery["startPercent"] - battery["endPercent"]) / battery["hours"]
    budget_target = budget["targetPercentPerHour"] if budget else None
    return {
        "deliveryRatio": round(ratio, 3),
        "deliveryTarget": DELIVERY_TARGET[kind],
        "latencyP50Ms": percentile(latencies, 0.5) if latencies else -1,
        "latencyP95Ms": p95,
        "latencyTargetMs": LATENCY_TARGET_P95_MS[kind],
        "maxDegree": max_degree,
        "degreeCap": session["degreeCapConfigured"],
        "batteryDrainPerHour": round(drain, 3),
        "batteryTargetPerHour": budget_target,
        "passes": {
            "delivery": ratio >= DELIVERY_TARGET[kind],
            "latency": p95 <= LATENCY_TARGET_P95_MS[kind],
            "degree": max_degree <= session["degreeCapConfigured"],
            "battery": budget_target is None or drain <= budget_target,
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("session")
    parser.add_argument("--degree-cap", type=int)
    parser.add_argument("--budget", help="path to android/docs/battery-budget.json")
    args = parser.parse_args(argv)
    session = load(Path(args.session))
    errors = validate_schema(session, args.degree_cap)
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    if session["status"] == "PLANNED":
        print(f"{args.session}: PLANNED, no measurements to score")
        return 0
    budget = None
    if args.budget:
        with Path(args.budget).open(encoding="utf-8") as handle:
            budget = json.load(handle)
    metrics = compute_metrics(session, budget)
    for key, value in metrics.items():
        print(f"{key}: {value}")
    return 0 if all(metrics["passes"].values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
