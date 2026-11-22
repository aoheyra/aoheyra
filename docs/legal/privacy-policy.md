# Privacy policy draft (APP-18)

Status: draft for review. This describes the MVP design as pinned at
`v0.1.0`; a feature that changes any statement below must update this file in
the same change.

## What stays on the device

- The identity seed and secret keys, wrapped by a Keystore-held key where the
  device provides one. They are never sent to a server and never included in
  diagnostics exports.
- Contacts and their trust state, messages and delivery status, settings,
  carried mesh packets and location-sharing session rules, in the encrypted
  SQLite database. `aoheyra` is the only owner of this data; Android has no
  second database.
- Precise coordinates, except inside a sharing session whose precision the
  user chose. "Stop sharing" ends the session immediately and publishes the
  end to the selected contacts.
- Crash and diagnostics data until the user explicitly exports a bundle.
  Bundles contain counts and allowlisted device facts, never message text,
  keys, contact lists or coordinates.

There is no account, no server, no analytics and no crash reporting that
leaves the device. The map works offline; Google Play Services is not needed
for QR, maps or location.

## What leaves the device

Only mesh traffic created by an explicit user action, and only over Bluetooth
LE to nearby phones:

- Sealed chat messages and location updates, readable only by the chosen
  recipients under the current crypto suite. Location precision follows the
  user's session setting; relays see opaque bytes.
- Signed public-channel messages the user posts to a channel they joined.
  These are not encrypted and are visible to anyone forwarding or listening
  on that channel.
- Presence, node-directory, receipt and routing packet metadata. Relay
  metadata (origin, destination, size and timing) is deliberately visible so
  the mesh can rate-limit spam; this is the accepted MVP trade-off.
- An optional user-exported diagnostics bundle and an optional encrypted
  identity/contact backup. Neither is transmitted by the app.

## Known limits

- No forward secrecy against recipient key compromise in sealed messages.
- A recipient can forward location it lawfully received; delivery does not
  give the sender post-delivery control.
- Any relay can drop or delay traffic; delivery is best effort.
- First contact is trusted on first use until the user compares safety
  numbers out of band.
- Radio observers can see traffic patterns even when content is sealed.

These limits come from the aoIdents threat model (IDN-19) and the routing
metadata decision (MSH-02); they are stated here rather than hidden.
