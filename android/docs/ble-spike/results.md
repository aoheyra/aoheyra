# Results

`NOT MEASURED` is an explicit state: no device was available and no value was
inferred from a simulator, an emulator or another phone. Fill one column per
physical phone before closing AND-02.

| Metric | Pixel-class phone 1 | Mid-range phone 2 | OEM phone 3 |
|---|---|---|---|
| Model | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Android API level | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Bluetooth controller | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Battery/OEM mode during test | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Highest stable simultaneous links | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| First failure status code | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Negotiated MTU | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Connect latency p50 / worst (ms) | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Status 133 count / 50 attempts | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Throughput delivered / 1,000 | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Throughput duplicates | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Notification stalls | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Screen-off scan callbacks after 30 min | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Screen-off advertising after 30 min | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Advertising/scan restart after Bluetooth toggle | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Process killed in background | NOT MEASURED | NOT MEASURED | NOT MEASURED |
| Restart delay after kill (s) | NOT MEASURED | NOT MEASURED | NOT MEASURED |

## NET-16 defaults to confirm

Current aoNetwork defaults are `max_links = 5`, requested ATT MTU 517 with a
514-byte maximum ATT value, and a 10 s discovery interval. No change is made
until the measured table exists.

| Setting | Current value | Measured value | Decision |
|---|---|---|---|
| `max_links` | 5 | NOT MEASURED | pending |
| requested MTU | 517 | NOT MEASURED | pending |
| maximum ATT value | 514 | NOT MEASURED | pending |
| `discovery_interval` | 10 s | NOT MEASURED | pending |
| minSdk | 26 | NOT MEASURED | pending |

Any change to those defaults belongs to NET-16 in aoNetwork, not to this
repository. This table records the evidence used for the request.
