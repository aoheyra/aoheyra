#!/usr/bin/env python3
"""AND-20: validate the OEM results table before it is used as evidence.

A row marked VERIFIED must contain real observations and a path to a note that
is present in the checkout. A pending row must not contain numbers: a matrix
that looks measured but is not is worse than an honest empty matrix.
"""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

HEADER = [
    "vendor",
    "device",
    "android_version",
    "connection_cap",
    "advertising_limit",
    "background_survival_8h",
    "workaround",
    "workaround_verified",
    "evidence_ref",
    "status",
]
VENDORS = {"pixel", "samsung", "xiaomi", "oneplus", "other"}
STATUSES = {"PENDING_DEVICE_RUN", "VERIFIED"}
WORKAROUNDS = {"common", "pixel", "samsung", "xiaomi", "oneplus", "other"}
NOT_MEASURED = "NOT_MEASURED"
MEASURED_FIELDS = ("connection_cap", "advertising_limit", "background_survival_8h")


def validate(path: Path, root: Path) -> list[str]:
    errors: list[str] = []
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames != HEADER:
            return [f"header mismatch: expected {HEADER}, got {reader.fieldnames}"]
        rows = list(reader)

    if {row["vendor"] for row in rows} != VENDORS:
        errors.append("matrix must have exactly one row per vendor key")

    for index, row in enumerate(rows, start=2):
        vendor = row["vendor"]
        status = row["status"]
        if status not in STATUSES:
            errors.append(f"row {index}: bad status {status!r}")
            continue
        workaround_ids = set(row["workaround"].split("+"))
        if "common" not in workaround_ids or not workaround_ids <= WORKAROUNDS:
            errors.append(f"row {index}: bad workaround ids {row['workaround']!r}")
        if vendor != "other" and vendor not in workaround_ids:
            errors.append(f"row {index}: missing vendor help step for {vendor}")
        if status == "PENDING_DEVICE_RUN":
            for field in MEASURED_FIELDS:
                if row[field] != NOT_MEASURED:
                    errors.append(f"row {index}: pending row has {field}={row[field]!r}")
            if row["evidence_ref"] != "NONE":
                errors.append(f"row {index}: pending row must not claim evidence")
            if row["workaround_verified"] != "NO":
                errors.append(f"row {index}: pending row cannot verify a workaround")
        else:
            for field in MEASURED_FIELDS:
                if row[field] == NOT_MEASURED or not row[field].strip():
                    errors.append(f"row {index}: verified row has empty {field}")
            if not re.fullmatch(r"[A-Za-z0-9._/-]+", row["evidence_ref"] or ""):
                errors.append(f"row {index}: evidence_ref must be a repository-relative path")
            elif not (root / row["evidence_ref"]).is_file():
                errors.append(f"row {index}: evidence file missing: {row['evidence_ref']}")
            if row["device"] == NOT_MEASURED or row["android_version"] == NOT_MEASURED:
                errors.append(f"row {index}: verified row needs a device and Android version")
            if row["workaround_verified"] not in {"YES", "NO"}:
                errors.append(f"row {index}: workaround_verified must be YES or NO")
    return errors


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: validate_oem_results.py <results.csv>", file=sys.stderr)
        return 2
    path = Path(argv[1])
    root = Path(__file__).resolve().parents[3]
    errors = validate(path, root)
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    print(f"{path}: {len(list(path.open(encoding='utf-8'))) - 1} rows valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
