# AND-19 Battery budget and performance

## Budget (target, not a measurement)

The default profile has a **target of at most 3.0 percent battery drain per hour
with the screen off** on each reference phone. This is a budget to be verified,
not a reported measurement: no phone was attached to this repository, so all
rows in the results table below are `NOT_MEASURED`. The machine-readable source
of the budget is [`battery-budget.json`](battery-budget.json); the script
`android/scripts/battery/compute_budget.py` compares a completed run against it.

| Profile | Scan window / interval | Advertise interval | Sync dwell | Max continuous wake | Target drain / h |
|---|---|---|---|---|---|
| `balanced` (default) | 4000 ms / 12000 ms | 1000 ms | 3000 ms | 30000 ms | 3.0 % |
| `low_power` | 2000 ms / 20000 ms | 3000 ms | 1500 ms | 15000 ms | 2.0 % |
| `performance` | 4000 ms / 8000 ms | 1000 ms | 5000 ms | 60000 ms | 5.0 % |

Profile values are initial engineering bounds, not values tuned from Battery
Historian runs. They must be replaced in the same change that publishes real
measurements. `BatteryProfiles.validate()` enforces the bounds below so no
profile can silently grow into an unbounded scan loop or wake lock. The Kotlin
object is a compile-time contract for those Android-side bounds, not a settings
store: runtime settings are mapped onto crate configs by the Rust core (APP-11),
and Kotlin reads through UniFFI.

* `scanWindowMs` and `advertiseIntervalMs` are positive; `scanWindowMs <= scanIntervalMs`.
* `syncDwellMs <= maxContinuousWakeMs <= 60000 ms`.
* Scan duty cycle is at most 0.5 (the phone must be free to sleep).
* `low_power` is at least as conservative as `balanced` on every radio parameter.

## Wake-lock policy

There is no `WAKE_LOCK` permission in `AndroidManifest.xml` and no partial wake
lock in this module. A foreground service (AND-10) with a visible notification
is the only mechanism that keeps the mesh alive. Every sync operation is bounded
by `maxContinuousWakeMs`; when it ends, the loop returns to the scan/advertise
windows above and the phone is free to suspend. No alarm wakes the app while the
screen is off. `test_compute_budget.py` checks that the manifest stays free of
the permission.

## Battery Historian runs

Runs of 1, 8 and 24 hours are required; the 8-hour run is the MVP acceptance
scenario 8. Use one physical reference phone at a time, screen off for the
whole run, with the default profile and no charger.

```bash
ANDROID_BATTERY_EVIDENCE_DIR=/home/Gaurav/tmp/battery-evidence \
  android/scripts/battery/run-soak.sh --hours 8 --out /home/Gaurav/tmp/battery-evidence/pixel-8h
```

The script resets batterystats, records the device model and screen-off
confirmation, sleeps for the requested window and captures `adb bugreport`.
Analyse the bugreport with Battery Historian, then write a metrics file:

```json
{"startBattery": 82, "endBattery": 78, "hours": 8, "screenOnMinutes": 0}
```

```bash
python3 android/scripts/battery/compute_budget.py /home/Gaurav/tmp/battery-evidence/pixel-8h/metrics.json
```

`screenOnMinutes` above zero invalidates the run: it is not a screen-off
measurement. `compute_budget.py` rejects it and exits non-zero.

## Results

| Device | Android | Hours | Screen off | Start | End | Drain / h | Within 3.0 % budget | Evidence |
|---|---|---|---|---|---|---|---|---|
| reference Pixel | `NOT_MEASURED` | 1 | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | none |
| reference Pixel | `NOT_MEASURED` | 8 | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | none |
| reference Pixel | `NOT_MEASURED` | 24 | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | none |
| reference Samsung | `NOT_MEASURED` | 1 | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | none |
| reference Samsung | `NOT_MEASURED` | 8 | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | none |
| reference Samsung | `NOT_MEASURED` | 24 | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | none |

## Regression gate

The repeatable gate is the unit test plus the budget checker: changing a
profile or the target requires `android/docs/battery-budget.json`, the Kotlin
constants and this table to be updated together, and a test fails if they
drift. Once measurements exist, add the completed metrics file for each run to
the evidence directory and run `compute_budget.py` on it in the field-test
report (AND-23). Changes that increase drain must be accompanied by a new run
on the same reference phones; the target is not relaxed to make a run pass.

## Blocker

`Done when` requires profiles tuned from measurements, runs on reference phones
and a demonstrated budget. Scaffolding, profiles, wake-lock policy, the
repeatable checker and the run procedure exist here. No Android SDK, device or
Battery Historian run was available, so measured values are not claimed.
