#!/usr/bin/env python3
"""AND-20: tests for the results-table validator (stdlib only)."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_help_strings import check as check_help_strings
from validate_oem_results import HEADER, NOT_MEASURED, validate

REPO = Path(__file__).resolve().parents[3]
RESULTS = REPO / "android" / "docs" / "oem" / "results.csv"


class OemResultsTest(unittest.TestCase):
    def test_committed_matrix_is_honestly_pending(self) -> None:
        self.assertEqual(validate(RESULTS, REPO), [])

    def test_pending_row_rejects_invented_observation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "results.csv"
            rows = RESULTS.read_text(encoding="utf-8").splitlines()
            rows[1] = (
                "pixel,Pixel 7,14,8,120ms,ALIVE,common+pixel,NO,"
                "android/docs/oem/evidence/pixel.md,PENDING_DEVICE_RUN"
            )
            path.write_text("\n".join(rows) + "\n", encoding="utf-8")
            errors = validate(path, REPO)
            self.assertTrue(any("pending row has connection_cap" in e for e in errors))
            self.assertTrue(any("pending row must not claim evidence" in e for e in errors))

    def test_verified_row_requires_evidence_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "results.csv"
            rows = RESULTS.read_text(encoding="utf-8").splitlines()
            rows[1] = (
                "pixel,Pixel 7,14,8,120ms,ALIVE,common+pixel,YES,"
                "android/docs/oem/evidence/missing.md,VERIFIED"
            )
            path.write_text("\n".join(rows) + "\n", encoding="utf-8")
            errors = validate(path, REPO)
            self.assertTrue(any("evidence file missing" in e for e in errors))

    def test_verified_row_accepts_present_evidence(self) -> None:
        with tempfile.TemporaryDirectory(dir=REPO) as tmp:
            path = Path(tmp) / "results.csv"
            evidence = Path(tmp) / "evidence.md"
            evidence.write_text("measured by hand\n", encoding="utf-8")
            rows = RESULTS.read_text(encoding="utf-8").splitlines()
            rows[1] = (
                "pixel,Pixel 7,14,8,120ms,ALIVE,common+pixel,YES,"
                f"{evidence.relative_to(REPO)},VERIFIED"
            )
            path.write_text("\n".join(rows) + "\n", encoding="utf-8")
            self.assertEqual(validate(path, REPO), [])

    def test_help_screen_strings_are_declared(self) -> None:
        self.assertEqual(check_help_strings(REPO), [])

    def test_header_is_exact(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "results.csv"
            path.write_text(",".join(HEADER[:-1]) + "\n", encoding="utf-8")
            errors = validate(path, REPO)
            self.assertEqual(len(errors), 1)
            self.assertIn("header mismatch", errors[0])
            self.assertEqual(NOT_MEASURED, "NOT_MEASURED")


if __name__ == "__main__":
    unittest.main()
