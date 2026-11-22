# Foreground service (AND-10)

`ForegroundMeshService` owns the process lifetime of the BLE and Rust stack. It
declares only `FOREGROUND_SERVICE` and `FOREGROUND_SERVICE_CONNECTED_DEVICE`,
and uses the `connectedDevice` foreground type on Android 14 and above. Location
is not requested: it belongs to an explicit sharing session in AND-16.

The ongoing notification shows the mesh state (starting, running, stopping,
stopped or unavailable) and has a one-tap stop action. Its channel importance is
low so it stays visible without interrupting the user. The stop action records
an explicit-stop lifecycle flag, stops the runtime once and returns
`START_NOT_STICKY`. A normal start returns `START_STICKY`, so the system
recreates the service after an unexpected kill. Only the explicit-stop flag is
persisted; it is not application data and is cleared by `MeshServiceLauncher.start`.

Doze is handled by the connected-device type combined with the scanner's
service-UUID hardware filter and the advertiser's long restart window. No wake
lock, battery-optimization exemption or background location is requested. The
service does not stop the runtime in `onDestroy`: that would defeat restart
after a kill. An explicit stop already stopped it through its own action.

`MeshServiceController` is the pure, idempotent start/stop state machine.
Duplicate start or stop commands call the runtime at most once per transition,
and a failed transition restores the previous state for a retry. The runtime
binds its `MeshRuntimeLifecycle` through `MeshServiceRuntime` and calls
`MeshServiceLauncher` for start, stop and state changes.

## APP-03 blocker

PR #54 does not expose any Kotlin-to-Rust runtime lifecycle callback, so nothing
can call `MeshServiceLauncher` from the Rust side yet. The service and its
runtime-facing API are implemented, but "start and stop owned by the runtime"
cannot be demonstrated end to end until APP-03 issue #5 exports the required
seam. This PR does not invent a UniFFI method.

## Tests and evidence

JVM tests cover idempotent start, idempotent stop, retry after failed start and
stop, missing runtime and restart-policy selection. The one-hour screen-off soak
and kill/restart behaviour need physical devices and were not run locally: no
Android SDK, Gradle installation or phones are available.
