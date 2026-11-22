# AND-20 OEM compatibility matrix

This is the recording contract for physical-device testing. No values here are
measured yet: this repository has no attached phones, so every observation is
`NOT_MEASURED`. Do not replace a cell with a number or a vendor claim without an
evidence reference from the phone used. The validator enforces that rule:

```bash
python3 android/scripts/oem/validate_oem_results.py android/docs/oem/results.csv
```

Results live in [`results.csv`](results.csv), one row per tested build/device.
The same vendors feed the onboarding help screen implemented in
`app/src/main/kotlin/com/aoheyra/app/onboarding/OemHelp.kt`; each row's
`workaround` column names the help step that must be verified on that device.

## Measurement contract

| Column | Meaning | How to obtain it |
|---|---|---|
| `vendor` | Stable key used by `OemVendor.from()` | `pixel`, `samsung`, `xiaomi`, `oneplus`, `other` |
| `device` | Model name as printed by the phone | `adb shell getprop ro.product.model` |
| `android_version` | Android release | `adb shell getprop ro.build.version.release` |
| `connection_cap` | Maximum simultaneous BLE connections observed, not the advertised maximum | Connect peers one at a time with the AND-04/08 stress harness; record the count at which a new link is rejected |
| `advertising_limit` | Observed advertising behaviour (interval floor and success/failure of connectable advertising with the screen off) | Start the AND-05 advertiser, log `onStartFailure`, capture `adb shell dumpsys bluetooth_manager` |
| `background_survival_8h` | Whether the mesh keeps relaying for eight hours with the screen off | AND-10 soak run; report `ALIVE` or `KILLED` plus the elapsed time in the evidence reference |
| `workaround` | The onboarding step id shown for that vendor (`common`, `samsung`, `xiaomi`, `oneplus`, `pixel`) | The step ids are defined in `OemHelp.kt` |
| `workaround_verified` | `YES` only when the step was followed on the listed device and restored relaying | On-device checkbox in the field sheet |
| `evidence_ref` | Path to the redacted evidence table or note; `NONE` while pending | `android/docs/oem/evidence/<vendor>-<model>-<date>.md` |
| `status` | `PENDING_DEVICE_RUN` or `VERIFIED` | Set to `VERIFIED` only when all four observations have evidence |

`capture-oem.sh` collects the raw device dumps into a directory **outside**
version control. Raw `dumpsys` output can contain identifiers and app state;
commit only the distilled table and a short note referencing it.

```bash
ANDROID_OEM_EVIDENCE_DIR=/home/Gaurav/tmp/oem-evidence \
  android/scripts/oem/capture-oem.sh <device-label>
```

## Matrix

| Vendor | Device | Android | Connection cap | Advertising limits | 8 h background survival | Help step | Status |
|---|---|---|---|---|---|---|---|
| Pixel (reference) | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `common`, `pixel` | PENDING_DEVICE_RUN |
| Samsung | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `common`, `samsung` | PENDING_DEVICE_RUN |
| Xiaomi | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `common`, `xiaomi` | PENDING_DEVICE_RUN |
| Oppo or OnePlus | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `common`, `oneplus` | PENDING_DEVICE_RUN |
| Older device (API 26-28) | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `NOT_MEASURED` | `common`, `other` | PENDING_DEVICE_RUN |

## Onboarding help content

The help screen is generic about what is observed and specific about where the
user should look. It never asserts that a setting exists on a device that has
not been verified: each vendor step says "check" and the matrix records whether
the step actually worked. The common steps (nearby-device permission and an
explanation of the battery trade-off) are shown for every vendor.

| Step id | Shown for | User action |
|---|---|---|
| `common` | all vendors | Explain that background relaying needs nearby-device permission and battery optimisations allowed; the user decides the trade-off |
| `pixel` | Pixel | Check the app's battery optimisation entry after an OS update |
| `samsung` | Samsung | Check Sleeping apps / sleeping apps list |
| `xiaomi` | Xiaomi | Check Autostart and background activity for aoheyra |
| `oneplus` | Oppo, OnePlus | Check background activity and aggressive battery settings |
| `other` | older/unknown devices | Check the OS-specific background-data and battery settings |

## Blocker

`Done when` requires a table of results and a per-vendor help screen. The help
screen and the results contract exist here; the results themselves cannot be
produced without five physical phones and an eight-hour soak per device. This
PR therefore leaves every measured cell as `NOT_MEASURED` and reports the
blocker instead of inventing observations.
