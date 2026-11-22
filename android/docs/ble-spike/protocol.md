# Measurement protocol

Use at least three physical phones from different vendors and Android API
levels. Emulators are not evidence for radio behaviour. Record the exact
model, Android version, Bluetooth controller and OEM battery setting for each
run. Repeat every measurement five times and record the median and worst
value; keep the raw log on the device until the results table is reviewed.

The standalone app must use the same values as the native contract:

- primary service `4c5b0001-108f-4105-a600-5a82e0fd7470`;
- RX `4c5b0002-108f-4105-a600-5a82e0fd7470`, write without response;
- TX `4c5b0003-108f-4105-a600-5a82e0fd7470`, notify, with CCCD;
- service UUID in the advertising data, a 16-byte test beacon in
  manufacturer data with company id `0xFFFF` in the scan response;
- `autoConnect = false`, no pairing, no device name, legacy advertising.

## Procedures

1. **Links and MTU.** Phone A advertises and hosts the GATT server; phone B
   scans and connects as central. Request MTU 517. Record the negotiated MTU
   and the largest ATT value accepted without fragmentation. Add phones until
   a connect fails; record the highest stable simultaneous link count and the
   status code of the first failure.
2. **Connect latency.** Time from `connectGatt` to the CCCD write callback.
   Record 50 attempts, including attempts after a failure, and count status
   133 separately.
3. **Throughput.** With one link up, send 1,000 ATT values of `mtu - 3` bytes
   from central to peripheral and back. Wait for each send callback before
   the next value. Record delivered values, duplicates, elapsed time and the
   observed stall count.
4. **Screen off.** Turn the screen off with the app in the foreground and
   log advertising and scan callbacks for 30 minutes. Record the last
   callback time, the number of `onScanFailed`/`onStartFailure` events and
   whether the service was killed by the OEM.
5. **Bluetooth toggles.** Turn Bluetooth off and on five times. Record
   whether advertising and scanning restart and whether any stale GATT
   handle is reused.
6. **Background kill.** Start the foreground service, put the app in the
   background, then kill the process. Record the restart delay and whether
   links and discovery resume.

Stop a run when a handle leak, a stuck GATT operation or a permission failure
is observed; record the stopping condition instead of continuing.

## Interpretation

The stable simultaneous link count feeds NET-16 `max_links`; the negotiated
MTU feeds the advertised MTU assumption and the 514-byte chunk ceiling. A
phone that cannot sustain the current defaults is recorded as a finding, not
removed from the table. Google Play Services is not used in any procedure.
