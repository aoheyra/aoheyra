# Android permissions and onboarding (AND-03)

The app does not touch Bluetooth until the user has seen and granted the
permissions the current Android version requires. The first-run screen states
why each permission is needed, and every denial or later revocation produces an
explanation plus a path that works.

## Permission matrix

| API level | Runtime permissions asked for | Why |
|---|---|---|
| 26-30 (Android 8-11) | `ACCESS_FINE_LOCATION`, `ACCESS_COARSE_LOCATION` | BLE scanning needs location permission. `BLUETOOTH` and `BLUETOOTH_ADMIN` are normal permissions on these versions. |
| 31-32 (Android 12-12L) | `BLUETOOTH_SCAN`, `BLUETOOTH_ADVERTISE`, `BLUETOOTH_CONNECT` | Nearby-device permissions replace location for BLE scanning. The scan permission declares `neverForLocation` because a BLE beacon is only used to break ties between duplicate links, never as a location source. |
| 33+ (Android 13+) | the Android 12 set plus `POST_NOTIFICATIONS` | The ongoing mesh notification needs a runtime grant. |

The manifest caps legacy location and Bluetooth permissions at SDK 30 so they
are not requested on newer versions.

## Recovery paths

| Condition | Shown text | Action |
|---|---|---|
| Permission not asked yet | The reason for that capability, then a Continue button | Ask for that capability's permissions only |
| Denied, rationale available | Denied explanation and a Try again button | Ask again |
| Denied, no rationale (permanent block) | Explanation that Android will not prompt again | Open app settings |
| Bluetooth off or unreadable | Turn Bluetooth on | Open Bluetooth settings |
| Location services off or unreadable on API 26-30 | Turn location services on | Open location settings |
| Battery optimisation active | Explanation that the mesh may pause with the screen off | Open app settings (optional; never blocks the radio) |
| Settings activity missing | One bounded notice; permission cards stay visible | Use the system Settings app |

State is rebuilt from the OS in `onResume`, so revoking a permission in Android
settings brings the card straight back when the user returns. Nothing about
permissions is persisted in app data.

## Bounds and privacy

- At most three capability cards and three system cards are rendered; the
  screen never accumulates a history of prompts or results.
- A single nullable notice is the only transient message.
- No permission string, Android setting or OS result is logged.
- The app data directory is already excluded from backup in the manifest; no
  key, database or message content is written by this feature.
- Device verification of API 29, 31 and 34 recovery flows is recorded in the
  AND-03 PR and remains pending until a device or emulator with those API
  levels is available.
