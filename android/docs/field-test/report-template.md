# Field report: <session id>

Fill one file per run; keep earlier reports. Copy values from
`validate_session.py` output, not from memory.

| Field | Value |
|---|---|
| Session id | |
| Phase and scenario | |
| Build commit | |
| Phones and Android versions | |
| Venue and conditions | |
| Diagnostics bundle ids | |
| Degree cap configured | |

## Metrics from the diagnostics bundles

| Metric | Value | Target | Pass |
|---|---|---|---|
| Messages sent / delivered | | | |
| Delivery ratio | | >= 0.90 (>= 0.70 under spammer) | |
| Latency p50 / p95 | | p95 <= 60 s (120 s under spammer) | |
| Maximum degree observed | | <= configured cap | |
| Battery drain / h | | <= AND-19 budget | |

## Observations

* Corridor or stairwell failures, including the phone positions:
* Spammer effect on normal traffic:
* Permission revocations, OEM kills or restarts:

## Defects filed

| Issue | Summary | Bundle id |
|---|---|---|
| | | |

## Status

`NOT_RUN` until a physical session has been completed and every cell above has
evidence. Do not publish inferred numbers.
