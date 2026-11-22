# AND-23 session results schema

One JSON object per session. `status` gates the rest of the file: a `PLANNED`
session has `null` measurements and cannot be scored; a `COMPLETED` session must
contain all fields below. No field may contain phone numbers, contact names,
message text, keys or precise coordinates.

| Field | Type | Meaning |
|---|---|---|
| `sessionId` | string | `^(short-m[35]|full-m7)-[a-z0-9-]+$`, unique per run |
| `phase` | string | `M3_SHORT`, `M5_SHORT` or `M7_FULL` |
| `scenario` | string | `outdoor`, `corridor`, `multi_floor`, `moving_crowd` or `spammer` |
| `buildCommit` | string | Git commit of the installed build |
| `deviceCount` | integer | 5 to 10 for a full run, 5 for a short run |
| `devices` | array of objects | Each with `label`, `androidVersion` (integer API level) and `vendor` |
| `startedUtc`, `endedUtc` | string | ISO-8601 UTC timestamps; end after start |
| `diagnosticsBundleIds` | array of strings | One id per phone; no file paths or personal data |
| `degreeCapConfigured` | integer | Degree cap configured for the build; must match `--degree-cap` |
| `messages` | array of objects | Synthetic traffic: `id`, `sentBy`, `sentAtMs`, `receivedBy` (array of labels), `arrivalsMs` (one per receiver) |
| `degreeSamples` | array of integers | Maximum neighbour count sampled every minute |
| `battery` | object | `startPercent`, `endPercent`, `hours` from a screen-off run where possible |
| `defects` | array of strings | Issue URLs or numbers filed from the session; empty is allowed |
| `status` | string | `PLANNED` or `COMPLETED` |
| `notes` | string | Venue and deviations from the script; no personal data |

`validate_session.py` checks the schema, computes delivery ratio, p50/p95
latency, maximum degree and battery drain, and compares them with the targets in
`README.md` and the budget file. It never edits the session file.
