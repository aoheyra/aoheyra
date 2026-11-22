# AND-21 Notifications, accessibility and i18n scaffolding

## Notification channels

Channels are created by `NotificationChannels.create(context)` when the
foreground service starts (AND-10). No channel posts a notification by itself,
and no notification SDK or Google Play Services dependency is added. The
`POST_NOTIFICATIONS` permission flow belongs to AND-03; this issue only defines
the channels that the service and location session will use.

| Channel id | Importance | Purpose | Strings |
|---|---|---|---|
| `mesh_service` | LOW | Persistent foreground-service notification; explains that the mesh is relaying | `channel_mesh_service_*` |
| `messages` | DEFAULT | Delivered message alerts; never contains message text | `channel_messages_*` |
| `location_sharing` | HIGH | Persistent indicator while a precise location session is active; one-tap stop | `channel_location_sharing_*` |

Notification content must never include message text, keys or coordinates. On
API 33+ the user can deny `POST_NOTIFICATIONS`; the service still runs, and the
in-app indicator remains accurate.

## Accessibility

* All user-facing text comes from `res/values/strings.xml`; no UI string is
  hard-coded in Kotlin. `check_resources.py` enforces this for the scaffold.
* Compose text sizes use `sp` (`16.sp` body, `14.sp` supporting text), so the
  platform font scale, including large text, is honoured. Never substitute `dp`.
* `AoHeyraTheme` follows `isSystemInDarkTheme()` with Material 3 light and dark
  colour schemes; `values-night/themes.xml` covers the pre-Compose window.
* Every non-text element that conveys meaning needs a content description from
  `cd_*` strings. Text composables already expose their text as semantics and
  must not be given a competing description.
* Touch targets stay at least 48 dp; the list rows in the OEM help screen use
  padding rather than fixed heights so they grow with font size.

## Translation workflow

1. Add or change text only in `res/values/strings.xml`. Give every user-facing
   string a translator comment that states the context.
2. Mark product names and protocol identifiers `translatable="false"`. The
   checker rejects any other non-translatable string.
3. Do not commit machine translations. A language is added as a complete
   `res/values-<locale>/strings.xml` with the same names and no placeholder
   text; partial translations are not shipped.
4. Freeze strings before a beta build. Run `python3 -m unittest discover -s
   android/scripts/a11y` and `./gradlew lintDebug`; `MissingTranslation` and
   hardcoded-text lint errors are release blockers, not warnings.

## Verification status

`check_resources.py` and its unit tests run locally without an SDK and pass.
`lintDebug` and an Accessibility Scanner pass on core screens require the
Android SDK and a physical device; neither was available here, so the `Done
when` condition is not claimed as met.
