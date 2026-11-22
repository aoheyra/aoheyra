# Security review checklist (APP-18)

Built from the accepted aoIdents threat model (IDN-19, tag `v0.1.0`) and the
routing metadata decision (MSH-02). A reviewer must replace `Pending` with a
date and evidence reference for every row before a beta tag is signed off.
No sign-off is claimed in this change.

| # | Check | Risk addressed | Owner | Evidence | Status |
|---|---|---|---|---|---|
| 1 | Message content is sealed to the recipient and opened only after sender signature and replay checks | Passive eavesdropper, malicious relay, impersonator | aoIdents / APP-09 | IDN-09, IDN-04, IDN-14 conformance tests | Pending |
| 2 | Symlink-free, domain-separated signatures cover every immutable field; hop fields stay unsigned | Forgery and tampering | aoMesh / aoIdents | MSH-02, MSH-04 codec and sign tests | Pending |
| 3 | Key substitution and changed keys raise typed warnings; TOFU never silently accepts a new key | Impersonator | aoIdents / APP-10 | IDN-03, IDN-08 contact-book tests | Pending |
| 4 | Optional hop handshake pins the card hash; failure installs no session and never falls back to cleartext | Impersonator, passive eavesdropper | aoIdents / aoMesh | IDN-13, IDN-14, MSH-26 integration tests | Pending |
| 5 | Per-link and per-origin budgets, replay bounds and suppression are enabled in shipped defaults | Spammer / Sybil, malicious relay | aoNetwork / aoMesh | NET-18, MSH-10, MSH-11 simulator rows | Pending |
| 6 | Identity seed and database key are Keystore-wrapped; copied app data is unusable; auto-backup excluded | Device thief | Android / APP-04 | AND-13, APP-04 key-store tests | Pending |
| 7 | Exported backups use Argon2id and XChaCha20-Poly1305; identity is verified after import | Device thief, corrupted backup | aoIdents / APP-16 | IDN-06 backup tests | Pending |
| 8 | Secrets are zeroized and redacted in `Debug`; logs and diagnostics never carry message text, keys or coordinates | All classes | aoIdents / APP-15 | IDN-02, APP-15 privacy tests | Pending |
| 9 | Location precision policy is enforced before sealing; sessions stop on time and on permission revocation | Malicious recipient, covert tracking | APP-12 / AND-16 | APP-07 payload-privacy tests, AND-16 permission tests | Pending |
| 10 | Diagnostics export and backup require a visible user action; no analytics or off-device crash reporting | Covert exfiltration | APP-15, APP-18 | This privacy draft and APP-15 bundle tests | Pending |
| 11 | Relay-visible metadata, no forward secrecy, TOFU and best-effort availability are documented in user-facing text | User misunderstanding of guarantees | APP-18 | `docs/legal/privacy-policy.md`, README privacy stance | Pending |
| 12 | Release builds keep UniFFI classes under R8; signing material stays out of the repository | Supply-chain and release integrity | AND-22 / APP-18 | `docs/release.md`, release build evidence | Pending |

## Explicitly accepted limits

The checklist verifies that the limits are stated and enforced where the
design claims protection; it does not convert them into guarantees:

- origin, destination, size and timing are visible to relays (MSH-02);
- sealed-message v1 has no forward secrecy against recipient key compromise;
- contact graph and traffic patterns are exposed to radio observers;
- a recipient can forward location after delivery;
- a relay can drop or delay traffic;
- first-contact impersonation needs out-of-band safety-number comparison;
- Sybil identities are bounded by budgets, not prevented;
- a stolen unlocked device stays valid until rotation and revocation spread;
- a compromised channel member can forge as other senders until rotation;
- cleartext links remain cleartext unless both peers negotiate hop encryption.
