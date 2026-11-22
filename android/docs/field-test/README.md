# AND-23 Field-test plan

Simulators cannot validate corridors, stairwells, moving crowds or a deliberate
spammer. This directory contains the scripted sessions, the results form and the
metric definitions. Every number in a completed report comes from a diagnostics
bundle exported on the phones (APP-15); nothing is estimated here.

## Sessions

Use 5 to 10 physical phones with the release candidate. Every phone installs
the same build, has nearby-device and location permission where the scenario
needs it, and exports a diagnostics bundle at the end. Messages are synthetic
strings generated for the test; never use real contacts or real coordinates.

| Id | Venue | Phones | Duration | Script |
|---|---|---|---|---|
| `outdoor` | Open park or courtyard, no fixed infrastructure | 5-10 | 30 min | Start in one cluster; pair A-C out of range with B in between; walk slowly apart and back together; send one 1:1 message per pair every 2 min |
| `corridor` | Indoor corridor with walls between endpoints | 5-10 | 20 min | Place phones in a line at roughly 10 m intervals; A and C out of range; send direct, relay and store-and-forward traffic; move one phone out and back to trigger carry-forward |
| `multi_floor` | Two floors joined by a stairwell | 5-10 | 20 min | Two phones on each floor, two on the stairs; repeat the corridor traffic; record which floor each message crossed |
| `moving_crowd` | Busy indoor area, phones carried by walking people | 8-10 | 20 min | Walk random loops at different speeds for the whole session; send one message per phone per minute; record the maximum observed degree |
| `spammer` | Any of the above | 5-10 | 15 min | One phone sends the configured maximum of broadcast messages continuously; the others run the normal 1:1 script; record whether normal traffic still arrives |

Short runs at the end of M3 and M5: `outdoor` and `corridor`, 5 phones, 10 min
each, no spammer. The full M7 run covers all five scenarios with 8-10 phones.
Each short run writes a report using the same schema so results are comparable.

Before each run: note the build commit, Android versions, phone models, the
configured degree cap and battery profile. After each run: export each phone's
diagnostics bundle on the device (no network upload), record the bundle ids in
the session file, and file any defect with the session id and the bundle id.

## Metrics and targets

The targets below are acceptance targets, not measured results.

| Metric | Definition | Target |
|---|---|---|
| Delivery ratio | Messages with at least one recorded arrival divided by messages sent | >= 0.90 normal scenarios; >= 0.70 for normal traffic during `spammer` |
| Latency | First arrival time minus send time per message; report p50 and p95 | p95 <= 60 s normal; p95 <= 120 s `spammer` |
| Degree behaviour | Maximum neighbours observed by any phone | Never above the configured degree cap; graph stays connected at least once |
| Battery | Start minus end percent divided by elapsed hours, screen off where possible | Within the AND-19 budget (3.0 %/h default profile) |

`android/scripts/field-test/validate_session.py` computes these values from a
completed session file and prints a pass/fail line for each. It reads no
network and no personal data.

## Results form and report

* [`session-template.json`](session-template.json) is the machine-readable
  submission; [`results-schema.md`](results-schema.md) defines every field.
* `.github/ISSUE_TEMPLATE/field-test-result.md` is the GitHub issue form for
  the human-readable result and linked defects.
* [`report-template.md`](report-template.md) is the written report required by
  the issue's Done when line. Copy it into a new file under this directory for
  each run; do not overwrite a previous run's report.

## Example command

```bash
python3 android/scripts/field-test/validate_session.py \
  android/docs/field-test/session-template.json --degree-cap 6
```

Add `--budget android/docs/battery-budget.json` once the AND-19 budget file is
present in the branch being tested; without it the battery target is reported
but not scored. The committed template is `PLANNED` and therefore contains no
results. The
validator refuses to compute metrics from it until a device run has filled the
fields.
