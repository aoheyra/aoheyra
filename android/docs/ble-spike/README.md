# AND-02 BLE feasibility spike

The spike measures Android BLE limits on physical hardware before the M2 BLE
modules choose defaults. It does not change protocol bytes and does not use
the production app: a standalone test app drives the same GATT layout and
advertising format as aoNetwork's native contract.

- [Measurement protocol](protocol.md)
- [Results table](results.md)

No device was attached to the environment where this document was written.
The results table therefore records `NOT MEASURED` and the Done-when line is
not met. Do not treat the current defaults as validated until the table is
filled and the NET-16 adjustment table is resolved.
