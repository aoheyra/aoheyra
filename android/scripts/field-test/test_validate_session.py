#!/usr/bin/env python3
"""AND-23: tests for the field-session validator and metric computation."""

from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

from validate_session import REPO, compute_metrics, load, validate_schema

TEMPLATE = REPO / "android/docs/field-test/session-template.json"
# The budget file is owned by AND-19; keep this branch's tests independent.
BUDGET = {"targetPercentPerHour": 3.0}


def completed_session() -> dict:
    session = copy.deepcopy(load(TEMPLATE))
    session.update(
        {
            "sessionId": "full-m7-corridor-1",
            "phase": "M7_FULL",
            "scenario": "corridor",
            "buildCommit": "adcf9434645e56e6333d28e9dcafa482e73365a1",
            "deviceCount": 5,
            "devices": [
                {"label": f"p{i}", "androidVersion": 34, "vendor": "pixel"} for i in range(5)
            ],
            "startedUtc": "2026-11-22T08:00:00Z",
            "endedUtc": "2026-11-22T08:20:00Z",
            "diagnosticsBundleIds": [f"bundle-{i}" for i in range(5)],
            "degreeCapConfigured": 6,
            "messages": [
                {
                    "id": f"synthetic-{i}",
                    "sentBy": "p0",
                    "sentAtMs": i * 60_000,
                    "receivedBy": ["p4"],
                    "arrivalsMs": [i * 60_000 + 5_000],
                }
                for i in range(10)
            ],
            "degreeSamples": [3, 4, 5, 4],
            "battery": {"startPercent": 80, "endPercent": 76, "hours": 8},
            "status": "COMPLETED",
            "notes": "Synthetic traffic only",
        }
    )
    return session


class FieldSessionTest(unittest.TestCase):
    def test_template_is_planned_and_empty(self) -> None:
        template = load(TEMPLATE)
        self.assertEqual(template["status"], "PLANNED")
        self.assertEqual(validate_schema(template, None), [])

    def test_completed_session_is_valid_and_scores(self) -> None:
        session = completed_session()
        self.assertEqual(validate_schema(session, 6), [])
        budget = BUDGET
        metrics = compute_metrics(session, budget)
        self.assertEqual(metrics["deliveryRatio"], 1.0)
        self.assertEqual(metrics["latencyP95Ms"], 5_000)
        self.assertEqual(metrics["maxDegree"], 5)
        self.assertEqual(metrics["batteryDrainPerHour"], 0.5)
        self.assertTrue(all(metrics["passes"].values()))

    def test_spammer_uses_its_own_targets(self) -> None:
        session = completed_session()
        session["scenario"] = "spammer"
        budget = BUDGET
        metrics = compute_metrics(session, budget)
        self.assertEqual(metrics["deliveryTarget"], 0.70)
        self.assertEqual(metrics["latencyTargetMs"], 120_000)

    def test_bad_records_are_rejected(self) -> None:
        cases = []
        bad_arrival = completed_session()
        bad_arrival["messages"][0]["arrivalsMs"] = [
            bad_arrival["messages"][0]["sentAtMs"] + 1_000,
            bad_arrival["messages"][0]["sentAtMs"] + 2_000,
        ]
        cases.append(("arrival count", bad_arrival))

        unknown_sender = completed_session()
        unknown_sender["messages"][0]["sentBy"] = "unknown"
        cases.append(("unknown sender", unknown_sender))

        cap = completed_session()
        cap["degreeSamples"] = [9]
        cases.append(("degree cap", cap))

        bundles = completed_session()
        bundles["diagnosticsBundleIds"] = []
        cases.append(("bundle ids", bundles))

        for label, session in cases:
            with self.subTest(case=label):
                self.assertNotEqual(validate_schema(session, 6), [])

    def test_cap_cli_must_match_session(self) -> None:
        errors = validate_schema(completed_session(), 7)
        self.assertTrue(any("--degree-cap" in error for error in errors))

    def test_low_delivery_fails_target(self) -> None:
        session = completed_session()
        for message in session["messages"][:3]:
            message["arrivalsMs"] = []
        budget = BUDGET
        metrics = compute_metrics(session, budget)
        self.assertFalse(metrics["passes"]["delivery"])
        self.assertEqual(metrics["deliveryRatio"], 0.7)

    def test_high_latency_fails_target(self) -> None:
        session = completed_session()
        session["messages"][0]["arrivalsMs"] = [
            session["messages"][0]["sentAtMs"] + 90_000
        ]
        budget = BUDGET
        self.assertFalse(compute_metrics(session, budget)["passes"]["latency"])

    def test_temp_file_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "session.json"
            path.write_text(json.dumps(completed_session()), encoding="utf-8")
            self.assertEqual(validate_schema(load(path), 6), [])


if __name__ == "__main__":
    unittest.main()
