//! Versioned, compact binary schema for application payloads (APP-07).
//!
//! aoMesh carries these bytes opaquely and only chooses a
//! [`PacketClass`](ao_mesh::packet::PacketClass) for
//! them; no sibling crate learns what a text, location, profile or receipt
//! contains. The codec is deliberately hand-rolled: the layout is fixed, all
//! integers are big-endian, and every input is checked before any variable
//! storage is allocated. There are no I/O, clock, RNG or third-party codec
//! dependencies here.
//!
//! # Wire layout
//!
//! Every payload starts with a one-byte version ([`crate::payload::PAYLOAD_VERSION`]) and a
//! one-byte kind. Unknown versions and kinds are rejected with a typed error;
//! decoding consumes the input exactly, so trailing bytes are rejected too.
//!
//! ```text
//! Text       ver | kind=1 | text_len:u16 BE | UTF-8 text (1..=1024 bytes)
//! Location   ver | kind=2 | precision_flags | timestamp_s:u32 BE
//!                 | [latitude_e6:i32 BE | longitude_e6:i32 BE
//!                    | accuracy_m:u16 BE | heading_tenths:u16 BE]
//! Profile    ver | kind=3 | flags | name_len:u8 | name | bio_len:u8 | bio
//!                 | [avatar_hash:8 bytes]
//! Receipt    ver | kind=4 | status:u8 | message_id:16 bytes
//! ```
//!
//! In `precision_flags`, bits 0..1 hold
//! [`LocationPrecision`](crate::payload::LocationPrecision) (0 precise,
//! 1 about 100 m, 2 off) and bit 7 signals a heading. All other bits must be
//! zero. `Off` encodes no coordinates, accuracy or heading at all: its total
//! length is [`crate::payload::OFF_ENCODED_LEN`] bytes. A full precise
//! location with heading is [`crate::payload::MAX_LOCATION_ENCODED_LEN`] bytes.
//!
//! Coordinates are signed 1e-6 degrees, the same unit Android locations use.
//! [`crate::payload::LocationPrecision::Approx100m`] rounds them onto a 0.001 degree grid
//! (about 111 m at the equator) inside the constructor, so neither the
//! in-memory value nor the encoded bytes ever hold the precise input. Decoding
//! rejects an `Approx100m` payload whose coordinates are off-grid instead of
//! silently treating it as precise.
//!
//! # Packet classes
//!
//! [`crate::payload::AppPayload::packet_class`] maps each variant onto the real class from
//! `ao_mesh::packet::PacketClass`: text and receipts are directed 1:1 traffic
//! (including the acknowledgement back to the sender), locations are
//! latest-wins telemetry, and profiles are public, trickle-refreshed node
//! state. Every encoded payload fits inside
//! [`crate::payload::MAX_ENCODED_LEN`], the aoMesh
//! packet payload cap.

use std::fmt;

use ao_mesh::packet::PacketClass;

/// The only application payload version this codec produces or accepts.
pub const PAYLOAD_VERSION: u8 = 1;

/// Hard cap on an encoded application payload.
///
/// This is the aoMesh [`MAX_PAYLOAD`](ao_mesh::packet::MAX_PAYLOAD) cap: an
/// encoded payload must fit in one mesh packet without fragmentation.
pub const MAX_ENCODED_LEN: usize = ao_mesh::packet::MAX_PAYLOAD;

/// Maximum UTF-8 bytes in a [`TextMessage`].
pub const MAX_TEXT_BYTES: usize = 1024;

/// Maximum encoded length of a [`TextMessage`]: version, kind, length and text.
pub const MAX_TEXT_ENCODED_LEN: usize = 4 + MAX_TEXT_BYTES;

/// Maximum UTF-8 bytes in a profile display name.
pub const MAX_NAME_BYTES: usize = 32;

/// Maximum UTF-8 bytes in a profile bio.
pub const MAX_BIO_BYTES: usize = 160;

/// Maximum encoded length of a [`Profile`], including an avatar hash.
pub const MAX_PROFILE_ENCODED_LEN: usize = 4 + MAX_NAME_BYTES + 1 + MAX_BIO_BYTES + 8;

/// Encoded length of a [`Receipt`].
pub const RECEIPT_ENCODED_LEN: usize = 19;

/// Encoded length of a location with precision [`LocationPrecision::Off`].
pub const OFF_ENCODED_LEN: usize = 7;

/// Encoded length of a precise location without a heading.
pub const PRECISE_ENCODED_LEN: usize = 17;

/// Encoded length of a precise location with a heading.
pub const PRECISE_WITH_HEADING_ENCODED_LEN: usize = 19;

/// Maximum encoded length of a [`LocationUpdate`].
pub const MAX_LOCATION_ENCODED_LEN: usize = PRECISE_WITH_HEADING_ENCODED_LEN;

/// Grid size in 1e-6 degrees for [`LocationPrecision::Approx100m`].
///
/// 1000 units is 0.001 degrees, about 111 m of latitude at the equator. The
/// rounding error is at most half a grid cell in either axis.
pub const APPROX_100M_GRID_E6: i32 = 1000;

/// Accuracy reported for an `Approx100m` update, in metres.
///
/// It is at least the rounding bound of the grid and is never lowered by the
/// caller's more optimistic accuracy value.
pub const APPROX_100M_ACCURACY_M: u16 = 150;

/// Largest accepted latitude in 1e-6 degrees (90 degrees north).
pub const MAX_LATITUDE_E6: i32 = 90_000_000;

/// Largest accepted longitude in 1e-6 degrees (180 degrees east or west).
pub const MAX_LONGITUDE_E6: i32 = 180_000_000;

const KIND_TEXT: u8 = 1;
const KIND_LOCATION: u8 = 2;
const KIND_PROFILE: u8 = 3;
const KIND_RECEIPT: u8 = 4;

const PRECISION_MASK: u8 = 0b0000_0011;
const HEADING_FLAG: u8 = 0b1000_0000;
const AVATAR_FLAG: u8 = 0b0000_0001;

const TEXT_HEADER_LEN: usize = 4;
const LOCATION_PREFIX_LEN: usize = 7;
const COORDS_LEN: usize = 8;
const ACCURACY_LEN: usize = 2;
const HEADING_LEN: usize = 2;
const MESSAGE_ID_LEN: usize = 16;
const AVATAR_HASH_LEN: usize = 8;

const RECEIPT_DELIVERED: u8 = 1;
const RECEIPT_READ: u8 = 2;

const _: () = assert!(MAX_TEXT_ENCODED_LEN <= MAX_ENCODED_LEN);
const _: () = assert!(MAX_PROFILE_ENCODED_LEN <= MAX_ENCODED_LEN);
const _: () = assert!(MAX_LOCATION_ENCODED_LEN <= MAX_ENCODED_LEN);
const _: () = assert!(RECEIPT_ENCODED_LEN <= MAX_ENCODED_LEN);
const _: () = assert!(MAX_LOCATION_ENCODED_LEN <= 20);

/// A typed failure to construct or decode an [`AppPayload`].
///
/// Errors never contain message text, coordinates, names or any other payload
/// content; only lengths, wire tags and field names are reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// Input (or a requested text) is longer than the hard cap.
    TooLarge {
        /// Length that was supplied.
        len: usize,
        /// Largest accepted length.
        max: usize,
    },
    /// The input ends before a required field.
    Truncated {
        /// First byte offset that does not exist.
        need: usize,
        /// Bytes actually present.
        have: usize,
    },
    /// The input has bytes after the end of the payload.
    Trailing {
        /// Exact encoded length of this payload kind.
        expected: usize,
        /// Number of bytes supplied.
        actual: usize,
    },
    /// The version byte is not [`PAYLOAD_VERSION`].
    UnsupportedVersion(u8),
    /// The kind byte is not one of the four defined variants.
    UnknownKind(u8),
    /// The location precision bits are reserved.
    UnknownPrecision(u8),
    /// A reserved flag bit is set, or heading is combined with `Off`.
    InvalidFlags(u8),
    /// A text, name or bio field is not valid UTF-8.
    InvalidUtf8,
    /// A text message would be empty.
    EmptyText,
    /// Text is longer than [`MAX_TEXT_BYTES`].
    TextTooLong {
        /// UTF-8 length that was supplied.
        len: usize,
        /// Largest accepted length.
        max: usize,
    },
    /// Display name is longer than [`MAX_NAME_BYTES`].
    NameTooLong {
        /// UTF-8 length that was supplied.
        len: usize,
        /// Largest accepted length.
        max: usize,
    },
    /// Bio is longer than [`MAX_BIO_BYTES`].
    BioTooLong {
        /// UTF-8 length that was supplied.
        len: usize,
        /// Largest accepted length.
        max: usize,
    },
    /// Latitude is outside the range ±[`MAX_LATITUDE_E6`].
    InvalidLatitude,
    /// Longitude is outside the range ±[`MAX_LONGITUDE_E6`].
    InvalidLongitude,
    /// Accuracy is zero; a precise update must state a positive bound.
    InvalidAccuracy,
    /// An approximate location claims accuracy finer than its grid supports.
    AccuracyTooOptimistic,
    /// Heading is greater than 3600 tenths of a degree.
    InvalidHeading,
    /// An `Approx100m` latitude is not on the [`APPROX_100M_GRID_E6`] grid.
    UnquantizedLatitude,
    /// An `Approx100m` longitude is not on the [`APPROX_100M_GRID_E6`] grid.
    UnquantizedLongitude,
    /// The receipt status byte is reserved.
    UnknownReceiptStatus(u8),
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { len, max } => write!(f, "payload length {len} exceeds cap {max}"),
            Self::Truncated { need, have } => {
                write!(f, "payload ends at {have} bytes, needs {need}")
            }
            Self::Trailing { expected, actual } => {
                write!(f, "payload is {actual} bytes, expected exactly {expected}")
            }
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported app payload version {version}")
            }
            Self::UnknownKind(kind) => write!(f, "unknown app payload kind {kind}"),
            Self::UnknownPrecision(bits) => write!(f, "reserved location precision bits {bits}"),
            Self::InvalidFlags(flags) => write!(f, "invalid app payload flags {flags}"),
            Self::InvalidUtf8 => f.write_str("payload field is not valid UTF-8"),
            Self::EmptyText => f.write_str("text message is empty"),
            Self::TextTooLong { len, max } => write!(f, "text length {len} exceeds cap {max}"),
            Self::NameTooLong { len, max } => {
                write!(f, "profile name length {len} exceeds cap {max}")
            }
            Self::BioTooLong { len, max } => {
                write!(f, "profile bio length {len} exceeds cap {max}")
            }
            Self::InvalidLatitude => f.write_str("latitude outside valid range"),
            Self::InvalidLongitude => f.write_str("longitude outside valid range"),
            Self::InvalidAccuracy => f.write_str("accuracy must be positive"),
            Self::AccuracyTooOptimistic => {
                f.write_str("approximate accuracy is finer than its grid")
            }
            Self::InvalidHeading => f.write_str("heading outside valid range"),
            Self::UnquantizedLatitude => f.write_str("approximate latitude is not quantized"),
            Self::UnquantizedLongitude => f.write_str("approximate longitude is not quantized"),
            Self::UnknownReceiptStatus(status) => {
                write!(f, "unknown receipt status {status}")
            }
        }
    }
}

impl std::error::Error for PayloadError {}

/// Precision the sharer chose for a [`LocationUpdate`].
///
/// The value is encoded in the low two bits of the location flags byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LocationPrecision {
    /// Send the coordinates as supplied, quantized only to 1e-6 degrees.
    Precise = 0,
    /// Round coordinates onto a roughly 100 m grid before they leave the device.
    Approx100m = 1,
    /// Send no coordinates, accuracy or heading at all.
    Off = 2,
}

/// Application-level delivery status carried by a [`Receipt`].
///
/// This is the status shown in chat; it is distinct from aoMesh's internal
/// delivery-receipt gossip, which never sees an application payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReceiptStatus {
    /// The message reached the intended device.
    Delivered = 1,
    /// The recipient has seen the message.
    Read = 2,
}

/// A short text message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMessage {
    text: String,
}

impl TextMessage {
    /// Validates and wraps UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError::EmptyText`] for empty input and
    /// [`PayloadError::TextTooLong`] beyond [`MAX_TEXT_BYTES`].
    pub fn new(text: &str) -> Result<Self, PayloadError> {
        let len = text.len();
        if len == 0 {
            return Err(PayloadError::EmptyText);
        }
        if len > MAX_TEXT_BYTES {
            return Err(PayloadError::TextTooLong {
                len,
                max: MAX_TEXT_BYTES,
            });
        }
        Ok(Self {
            text: text.to_owned(),
        })
    }

    /// The message text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A location update at the precision chosen by the sharer.
///
/// Constructing this value applies the precision policy: `Approx100m` rounds
/// the coordinates immediately and `Off` discards them entirely. All accessors
/// for coordinate data return `None` when the precision is `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocationUpdate {
    precision: LocationPrecision,
    timestamp_s: u32,
    latitude_e6: i32,
    longitude_e6: i32,
    accuracy_m: u16,
    heading_tenths: Option<u16>,
}

impl LocationUpdate {
    /// Constructs a location update, enforcing the chosen privacy policy.
    ///
    /// Coordinates use 1e-6 degrees, accuracy is in metres and heading is in
    /// tenths of a degree (0..=3600). For [`LocationPrecision::Off`] the
    /// coordinates, accuracy and heading are validated and then discarded.
    ///
    /// # Errors
    ///
    /// Returns a typed error for out-of-range coordinates, zero accuracy on a
    /// non-`Off` update or an out-of-range heading.
    pub fn new(
        precision: LocationPrecision,
        timestamp_s: u32,
        latitude_e6: i32,
        longitude_e6: i32,
        accuracy_m: u16,
        heading_tenths: Option<u16>,
    ) -> Result<Self, PayloadError> {
        validate_coordinates(latitude_e6, longitude_e6)?;
        if let Some(heading) = heading_tenths
            && heading > 3600
        {
            return Err(PayloadError::InvalidHeading);
        }
        match precision {
            LocationPrecision::Precise => {
                if accuracy_m == 0 {
                    return Err(PayloadError::InvalidAccuracy);
                }
                Ok(Self {
                    precision,
                    timestamp_s,
                    latitude_e6,
                    longitude_e6,
                    accuracy_m,
                    heading_tenths,
                })
            }
            LocationPrecision::Approx100m => {
                if accuracy_m == 0 {
                    return Err(PayloadError::InvalidAccuracy);
                }
                Ok(Self {
                    precision,
                    timestamp_s,
                    latitude_e6: quantize_e6(latitude_e6),
                    longitude_e6: quantize_e6(longitude_e6),
                    accuracy_m: accuracy_m.max(APPROX_100M_ACCURACY_M),
                    heading_tenths,
                })
            }
            LocationPrecision::Off => Ok(Self {
                precision,
                timestamp_s,
                latitude_e6: 0,
                longitude_e6: 0,
                accuracy_m: 0,
                heading_tenths: None,
            }),
        }
    }

    /// Precision this update was constructed with.
    #[must_use]
    pub const fn precision(&self) -> LocationPrecision {
        self.precision
    }

    /// Unix timestamp in whole seconds.
    #[must_use]
    pub const fn timestamp_s(&self) -> u32 {
        self.timestamp_s
    }

    /// Latitude in 1e-6 degrees, or `None` when sharing is off.
    #[must_use]
    pub const fn latitude_e6(&self) -> Option<i32> {
        match self.precision {
            LocationPrecision::Off => None,
            LocationPrecision::Precise | LocationPrecision::Approx100m => Some(self.latitude_e6),
        }
    }

    /// Longitude in 1e-6 degrees, or `None` when sharing is off.
    #[must_use]
    pub const fn longitude_e6(&self) -> Option<i32> {
        match self.precision {
            LocationPrecision::Off => None,
            LocationPrecision::Precise | LocationPrecision::Approx100m => Some(self.longitude_e6),
        }
    }

    /// Accuracy in metres, or `None` when sharing is off.
    #[must_use]
    pub const fn accuracy_m(&self) -> Option<u16> {
        match self.precision {
            LocationPrecision::Off => None,
            LocationPrecision::Precise | LocationPrecision::Approx100m => Some(self.accuracy_m),
        }
    }

    /// Heading in tenths of a degree, if one was supplied and sharing is on.
    #[must_use]
    pub const fn heading_tenths(&self) -> Option<u16> {
        self.heading_tenths
    }
}

/// Public profile announced for a node.
///
/// The display name and bio are user-chosen text; the optional avatar hash is
/// an opaque application identifier, never an encryption key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    display_name: String,
    bio: String,
    avatar_hash: Option<[u8; AVATAR_HASH_LEN]>,
}

impl Profile {
    /// Validates and wraps profile fields.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError::NameTooLong`] or [`PayloadError::BioTooLong`]
    /// when a field exceeds its byte cap.
    pub fn new(
        display_name: &str,
        bio: &str,
        avatar_hash: Option<[u8; 8]>,
    ) -> Result<Self, PayloadError> {
        if display_name.len() > MAX_NAME_BYTES {
            return Err(PayloadError::NameTooLong {
                len: display_name.len(),
                max: MAX_NAME_BYTES,
            });
        }
        if bio.len() > MAX_BIO_BYTES {
            return Err(PayloadError::BioTooLong {
                len: bio.len(),
                max: MAX_BIO_BYTES,
            });
        }
        Ok(Self {
            display_name: display_name.to_owned(),
            bio: bio.to_owned(),
            avatar_hash,
        })
    }

    /// The display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The free-form bio.
    #[must_use]
    pub fn bio(&self) -> &str {
        &self.bio
    }

    /// The optional opaque avatar hash.
    #[must_use]
    pub fn avatar_hash(&self) -> Option<&[u8; AVATAR_HASH_LEN]> {
        self.avatar_hash.as_ref()
    }
}

/// An application-level acknowledgement for a previously received message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receipt {
    message_id: [u8; MESSAGE_ID_LEN],
    status: ReceiptStatus,
}

impl Receipt {
    /// Wraps the 16-byte identifier of the message being acknowledged.
    #[must_use]
    pub const fn new(message_id: [u8; MESSAGE_ID_LEN], status: ReceiptStatus) -> Self {
        Self { message_id, status }
    }

    /// The acknowledged message identifier.
    #[must_use]
    pub const fn message_id(&self) -> &[u8; MESSAGE_ID_LEN] {
        &self.message_id
    }

    /// The acknowledged delivery status.
    #[must_use]
    pub const fn status(&self) -> ReceiptStatus {
        self.status
    }
}

/// A versioned application payload carried opaquely by aoMesh.
///
/// Every instance has already passed the constructor's validation, so
/// [`Self::encode`] produces a canonical byte string within the per-variant
/// caps. Decode is strict: unknown tags, reserved bits, non-canonical lengths,
/// trailing bytes and invalid UTF-8 are all rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppPayload {
    /// A user-visible text message.
    Text(TextMessage),
    /// A location update with an explicit precision policy.
    Location(LocationUpdate),
    /// A public node profile.
    Profile(Profile),
    /// An acknowledgement of a received message.
    Receipt(Receipt),
}

impl AppPayload {
    /// Decodes a payload from bytes.
    ///
    /// The overall size, version and kind are checked before any variant is
    /// parsed, and variable-length fields are bounds-checked before UTF-8
    /// validation or allocation.
    ///
    /// # Errors
    ///
    /// Returns the matching [`PayloadError`] for oversize, truncated,
    /// trailing, unknown-tag, malformed-field or invalid-UTF-8 input.
    pub fn decode(bytes: &[u8]) -> Result<Self, PayloadError> {
        if bytes.len() > MAX_ENCODED_LEN {
            return Err(PayloadError::TooLarge {
                len: bytes.len(),
                max: MAX_ENCODED_LEN,
            });
        }
        let header = take(bytes, 0, 2)?;
        if header[0] != PAYLOAD_VERSION {
            return Err(PayloadError::UnsupportedVersion(header[0]));
        }
        match header[1] {
            KIND_TEXT => decode_text(bytes),
            KIND_LOCATION => decode_location(bytes),
            KIND_PROFILE => decode_profile(bytes),
            KIND_RECEIPT => decode_receipt(bytes),
            other => Err(PayloadError::UnknownKind(other)),
        }
    }

    /// Encodes this payload into a fresh byte vector.
    ///
    /// # Errors
    ///
    /// Returns a typed length error if an instance somehow holds a field
    /// larger than its variant cap. Constructed and decoded values cannot
    /// reach this branch.
    pub fn encode(&self) -> Result<Vec<u8>, PayloadError> {
        match self {
            Self::Text(text) => encode_text(text),
            Self::Location(location) => Ok(encode_location(location)),
            Self::Profile(profile) => encode_profile(profile),
            Self::Receipt(receipt) => Ok(encode_receipt(receipt)),
        }
    }

    /// The aoMesh packet class this payload should be submitted under.
    #[must_use]
    pub const fn packet_class(&self) -> PacketClass {
        match self {
            Self::Text(_) | Self::Receipt(_) => PacketClass::Direct,
            Self::Location(_) => PacketClass::Telemetry,
            Self::Profile(_) => PacketClass::Presence,
        }
    }

    /// Exact number of bytes [`Self::encode`] will produce.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        match self {
            Self::Text(text) => TEXT_HEADER_LEN + text.text.len(),
            Self::Location(location) => encoded_location_len(location),
            Self::Profile(profile) => {
                4 + profile.display_name.len()
                    + 1
                    + profile.bio.len()
                    + if profile.avatar_hash.is_some() {
                        AVATAR_HASH_LEN
                    } else {
                        0
                    }
            }
            Self::Receipt(_) => RECEIPT_ENCODED_LEN,
        }
    }
}

/// Returns `input[offset..offset + len]`, checking the range without panicking.
fn take(input: &[u8], offset: usize, len: usize) -> Result<&[u8], PayloadError> {
    let end = match offset.checked_add(len) {
        Some(end) => end,
        None => {
            return Err(PayloadError::Truncated {
                need: usize::MAX,
                have: input.len(),
            });
        }
    };
    if end > input.len() {
        return Err(PayloadError::Truncated {
            need: end,
            have: input.len(),
        });
    }
    Ok(&input[offset..end])
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, PayloadError> {
    let bytes = take(input, offset, 2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_i32(input: &[u8], offset: usize) -> Result<i32, PayloadError> {
    let bytes = take(input, offset, 4)?;
    Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, PayloadError> {
    let bytes = take(input, offset, 4)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_utf8(input: &[u8], offset: usize, len: usize) -> Result<&str, PayloadError> {
    let bytes = take(input, offset, len)?;
    std::str::from_utf8(bytes).map_err(|_| PayloadError::InvalidUtf8)
}

fn exact_len(len: usize, expected: usize) -> Result<(), PayloadError> {
    match len.cmp(&expected) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => Err(PayloadError::Truncated {
            need: expected,
            have: len,
        }),
        std::cmp::Ordering::Greater => Err(PayloadError::Trailing {
            expected,
            actual: len,
        }),
    }
}

fn validate_coordinates(latitude_e6: i32, longitude_e6: i32) -> Result<(), PayloadError> {
    if !(-MAX_LATITUDE_E6..=MAX_LATITUDE_E6).contains(&latitude_e6) {
        return Err(PayloadError::InvalidLatitude);
    }
    if !(-MAX_LONGITUDE_E6..=MAX_LONGITUDE_E6).contains(&longitude_e6) {
        return Err(PayloadError::InvalidLongitude);
    }
    Ok(())
}

/// Rounds to the nearest `APPROX_100M_GRID_E6` cell, halfway points upward.
fn quantize_e6(value: i32) -> i32 {
    let cells = value.div_euclid(APPROX_100M_GRID_E6);
    let remainder = value.rem_euclid(APPROX_100M_GRID_E6);
    let rounded = if remainder >= APPROX_100M_GRID_E6 / 2 {
        cells + 1
    } else {
        cells
    };
    // Input bounds make |rounded| <= 180_000, so this product stays in i32.
    rounded * APPROX_100M_GRID_E6
}

fn encode_text(text: &TextMessage) -> Result<Vec<u8>, PayloadError> {
    let bytes = text.text.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| PayloadError::TextTooLong {
        len: bytes.len(),
        max: MAX_TEXT_BYTES,
    })?;
    let mut out = Vec::with_capacity(TEXT_HEADER_LEN + bytes.len());
    out.push(PAYLOAD_VERSION);
    out.push(KIND_TEXT);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(out)
}

fn decode_text(bytes: &[u8]) -> Result<AppPayload, PayloadError> {
    let len = usize::from(read_u16(bytes, 2)?);
    if len > MAX_TEXT_BYTES {
        return Err(PayloadError::TextTooLong {
            len,
            max: MAX_TEXT_BYTES,
        });
    }
    let expected = TEXT_HEADER_LEN
        .checked_add(len)
        .ok_or(PayloadError::TooLarge {
            len: usize::MAX,
            max: MAX_ENCODED_LEN,
        })?;
    exact_len(bytes.len(), expected)?;
    let text = read_utf8(bytes, TEXT_HEADER_LEN, len)?;
    if text.is_empty() {
        return Err(PayloadError::EmptyText);
    }
    Ok(AppPayload::Text(TextMessage {
        text: text.to_owned(),
    }))
}

fn encoded_location_len(location: &LocationUpdate) -> usize {
    match location.precision {
        LocationPrecision::Off => OFF_ENCODED_LEN,
        LocationPrecision::Precise | LocationPrecision::Approx100m => {
            LOCATION_PREFIX_LEN
                + COORDS_LEN
                + ACCURACY_LEN
                + if location.heading_tenths.is_some() {
                    HEADING_LEN
                } else {
                    0
                }
        }
    }
}

fn encode_location(location: &LocationUpdate) -> Vec<u8> {
    let mut out = Vec::with_capacity(encoded_location_len(location));
    out.push(PAYLOAD_VERSION);
    out.push(KIND_LOCATION);
    let mut flags = location.precision as u8 & PRECISION_MASK;
    if location.heading_tenths.is_some() && location.precision != LocationPrecision::Off {
        flags |= HEADING_FLAG;
    }
    out.push(flags);
    out.extend_from_slice(&location.timestamp_s.to_be_bytes());
    if location.precision == LocationPrecision::Off {
        return out;
    }
    // Constructor guarantees Some(..) for every non-Off precision. The match
    // keeps the codec total even if an invariant is ever broken.
    let (latitude, longitude, accuracy) = match (
        location.latitude_e6(),
        location.longitude_e6(),
        location.accuracy_m(),
    ) {
        (Some(lat), Some(lon), Some(acc)) => (lat, lon, acc),
        _ => (0, 0, APPROX_100M_ACCURACY_M),
    };
    out.extend_from_slice(&latitude.to_be_bytes());
    out.extend_from_slice(&longitude.to_be_bytes());
    out.extend_from_slice(&accuracy.to_be_bytes());
    if let Some(heading) = location.heading_tenths {
        out.extend_from_slice(&heading.to_be_bytes());
    }
    out
}

fn decode_location(bytes: &[u8]) -> Result<AppPayload, PayloadError> {
    take(bytes, 0, LOCATION_PREFIX_LEN)?;
    let flags = bytes[2];
    if flags & !(PRECISION_MASK | HEADING_FLAG) != 0 {
        return Err(PayloadError::InvalidFlags(flags));
    }
    let precision = match flags & PRECISION_MASK {
        0 => LocationPrecision::Precise,
        1 => LocationPrecision::Approx100m,
        2 => LocationPrecision::Off,
        other => return Err(PayloadError::UnknownPrecision(other)),
    };
    let has_heading = flags & HEADING_FLAG != 0;
    if has_heading && precision == LocationPrecision::Off {
        return Err(PayloadError::InvalidFlags(flags));
    }
    let timestamp_s = read_u32(bytes, 3)?;

    if precision == LocationPrecision::Off {
        exact_len(bytes.len(), OFF_ENCODED_LEN)?;
        return Ok(AppPayload::Location(LocationUpdate {
            precision,
            timestamp_s,
            latitude_e6: 0,
            longitude_e6: 0,
            accuracy_m: 0,
            heading_tenths: None,
        }));
    }

    let expected =
        LOCATION_PREFIX_LEN + COORDS_LEN + ACCURACY_LEN + if has_heading { HEADING_LEN } else { 0 };
    exact_len(bytes.len(), expected)?;
    let latitude_e6 = read_i32(bytes, LOCATION_PREFIX_LEN)?;
    let longitude_e6 = read_i32(bytes, LOCATION_PREFIX_LEN + 4)?;
    let accuracy_m = read_u16(bytes, LOCATION_PREFIX_LEN + COORDS_LEN)?;
    let heading_tenths = if has_heading {
        Some(read_u16(
            bytes,
            LOCATION_PREFIX_LEN + COORDS_LEN + ACCURACY_LEN,
        )?)
    } else {
        None
    };

    if precision == LocationPrecision::Approx100m {
        if latitude_e6.rem_euclid(APPROX_100M_GRID_E6) != 0 {
            return Err(PayloadError::UnquantizedLatitude);
        }
        if longitude_e6.rem_euclid(APPROX_100M_GRID_E6) != 0 {
            return Err(PayloadError::UnquantizedLongitude);
        }
        if accuracy_m < APPROX_100M_ACCURACY_M {
            return Err(PayloadError::AccuracyTooOptimistic);
        }
    }
    validate_coordinates(latitude_e6, longitude_e6)?;
    if accuracy_m == 0 {
        return Err(PayloadError::InvalidAccuracy);
    }
    if let Some(heading) = heading_tenths
        && heading > 3600
    {
        return Err(PayloadError::InvalidHeading);
    }
    Ok(AppPayload::Location(LocationUpdate {
        precision,
        timestamp_s,
        latitude_e6,
        longitude_e6,
        accuracy_m,
        heading_tenths,
    }))
}

fn encode_profile(profile: &Profile) -> Result<Vec<u8>, PayloadError> {
    let name = profile.display_name.as_bytes();
    let bio = profile.bio.as_bytes();
    let name_len = u8::try_from(name.len()).map_err(|_| PayloadError::NameTooLong {
        len: name.len(),
        max: MAX_NAME_BYTES,
    })?;
    let bio_len = u8::try_from(bio.len()).map_err(|_| PayloadError::BioTooLong {
        len: bio.len(),
        max: MAX_BIO_BYTES,
    })?;
    let mut out = Vec::with_capacity(4 + name.len() + 1 + bio.len() + AVATAR_HASH_LEN);
    out.push(PAYLOAD_VERSION);
    out.push(KIND_PROFILE);
    out.push(if profile.avatar_hash.is_some() {
        AVATAR_FLAG
    } else {
        0
    });
    out.push(name_len);
    out.extend_from_slice(name);
    out.push(bio_len);
    out.extend_from_slice(bio);
    if let Some(hash) = profile.avatar_hash {
        out.extend_from_slice(&hash);
    }
    Ok(out)
}

fn decode_profile(bytes: &[u8]) -> Result<AppPayload, PayloadError> {
    take(bytes, 0, 4)?;
    let flags = bytes[2];
    if flags & !AVATAR_FLAG != 0 {
        return Err(PayloadError::InvalidFlags(flags));
    }
    let name_len = usize::from(bytes[3]);
    if name_len > MAX_NAME_BYTES {
        return Err(PayloadError::NameTooLong {
            len: name_len,
            max: MAX_NAME_BYTES,
        });
    }
    let display_name = read_utf8(bytes, 4, name_len)?;

    let bio_len_offset = 4usize
        .checked_add(name_len)
        .ok_or(PayloadError::InvalidUtf8)?;
    let bio_len = usize::from(*bytes.get(bio_len_offset).ok_or(PayloadError::Truncated {
        need: bio_len_offset + 1,
        have: bytes.len(),
    })?);
    if bio_len > MAX_BIO_BYTES {
        return Err(PayloadError::BioTooLong {
            len: bio_len,
            max: MAX_BIO_BYTES,
        });
    }
    let bio_offset = bio_len_offset
        .checked_add(1)
        .ok_or(PayloadError::InvalidUtf8)?;
    let bio = read_utf8(bytes, bio_offset, bio_len)?;

    let avatar_offset = bio_offset
        .checked_add(bio_len)
        .ok_or(PayloadError::InvalidUtf8)?;
    let has_avatar = flags & AVATAR_FLAG != 0;
    let expected = if has_avatar {
        avatar_offset.checked_add(AVATAR_HASH_LEN)
    } else {
        Some(avatar_offset)
    }
    .ok_or(PayloadError::InvalidUtf8)?;
    exact_len(bytes.len(), expected)?;

    let avatar_hash = if has_avatar {
        let raw = take(bytes, avatar_offset, AVATAR_HASH_LEN)?;
        let mut hash = [0u8; AVATAR_HASH_LEN];
        // `take` guarantees exactly AVATAR_HASH_LEN bytes.
        hash.copy_from_slice(raw);
        Some(hash)
    } else {
        None
    };
    Ok(AppPayload::Profile(Profile {
        display_name: display_name.to_owned(),
        bio: bio.to_owned(),
        avatar_hash,
    }))
}

fn encode_receipt(receipt: &Receipt) -> Vec<u8> {
    let status = match receipt.status {
        ReceiptStatus::Delivered => RECEIPT_DELIVERED,
        ReceiptStatus::Read => RECEIPT_READ,
    };
    let mut out = Vec::with_capacity(RECEIPT_ENCODED_LEN);
    out.push(PAYLOAD_VERSION);
    out.push(KIND_RECEIPT);
    out.push(status);
    out.extend_from_slice(&receipt.message_id);
    out
}

fn decode_receipt(bytes: &[u8]) -> Result<AppPayload, PayloadError> {
    take(bytes, 0, 3)?;
    let status = match bytes[2] {
        RECEIPT_DELIVERED => ReceiptStatus::Delivered,
        RECEIPT_READ => ReceiptStatus::Read,
        other => return Err(PayloadError::UnknownReceiptStatus(other)),
    };
    exact_len(bytes.len(), RECEIPT_ENCODED_LEN)?;
    let raw = take(bytes, 3, MESSAGE_ID_LEN)?;
    let mut message_id = [0u8; MESSAGE_ID_LEN];
    // `take` guarantees exactly MESSAGE_ID_LEN bytes.
    message_id.copy_from_slice(raw);
    Ok(AppPayload::Receipt(Receipt { message_id, status }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAT_E6: i32 = 19_076_090;
    const LON_E6: i32 = 72_877_700;
    const TS: u32 = 1_766_399_999;

    fn round_trip(payload: &AppPayload) -> Result<(), PayloadError> {
        let bytes = payload.encode()?;
        assert_eq!(bytes.len(), payload.encoded_len());
        assert_eq!(AppPayload::decode(&bytes)?, *payload);
        Ok(())
    }

    #[test]
    fn round_trip_text() -> Result<(), PayloadError> {
        round_trip(&AppPayload::Text(TextMessage::new("hello mesh")?))?;
        round_trip(&AppPayload::Text(TextMessage::new("नमस्ते 🌍")?))?;
        let longest = "a".repeat(MAX_TEXT_BYTES);
        round_trip(&AppPayload::Text(TextMessage::new(&longest)?))
    }

    #[test]
    fn round_trip_location_precise_with_heading() -> Result<(), PayloadError> {
        let update = LocationUpdate::new(
            LocationPrecision::Precise,
            TS,
            LAT_E6,
            LON_E6,
            12,
            Some(2745),
        )?;
        let payload = AppPayload::Location(update);
        round_trip(&payload)?;
        assert_eq!(payload.encode()?.len(), MAX_LOCATION_ENCODED_LEN);
        Ok(())
    }

    #[test]
    fn round_trip_location_precise_without_heading() -> Result<(), PayloadError> {
        let update = LocationUpdate::new(LocationPrecision::Precise, TS, LAT_E6, LON_E6, 20, None)?;
        let payload = AppPayload::Location(update);
        round_trip(&payload)?;
        assert_eq!(payload.encode()?.len(), PRECISE_ENCODED_LEN);
        Ok(())
    }

    #[test]
    fn round_trip_location_approx_100m() -> Result<(), PayloadError> {
        let update = LocationUpdate::new(
            LocationPrecision::Approx100m,
            TS,
            LAT_E6,
            LON_E6,
            5,
            Some(900),
        )?;
        let payload = AppPayload::Location(update);
        round_trip(&payload)?;
        let decoded = AppPayload::decode(&payload.encode()?)?;
        let AppPayload::Location(decoded) = decoded else {
            return Err(PayloadError::UnknownKind(KIND_LOCATION));
        };
        let lat = decoded.latitude_e6().ok_or(PayloadError::InvalidLatitude)?;
        let lon = decoded
            .longitude_e6()
            .ok_or(PayloadError::InvalidLongitude)?;
        assert_eq!(lat % APPROX_100M_GRID_E6, 0);
        assert_eq!(lon % APPROX_100M_GRID_E6, 0);
        assert_eq!(decoded.accuracy_m(), Some(APPROX_100M_ACCURACY_M.max(5)));
        Ok(())
    }

    #[test]
    fn round_trip_location_off() -> Result<(), PayloadError> {
        let update = LocationUpdate::new(LocationPrecision::Off, TS, LAT_E6, LON_E6, 5, Some(900))?;
        let payload = AppPayload::Location(update);
        round_trip(&payload)?;
        assert_eq!(payload.encode()?.len(), OFF_ENCODED_LEN);
        Ok(())
    }

    #[test]
    fn round_trip_profile() -> Result<(), PayloadError> {
        round_trip(&AppPayload::Profile(Profile::new("Asha", "", None)?))?;
        round_trip(&AppPayload::Profile(Profile::new(
            "Asha",
            "mesh only, no internet",
            Some([7; 8]),
        )?))
    }

    #[test]
    fn round_trip_receipt() -> Result<(), PayloadError> {
        round_trip(&AppPayload::Receipt(Receipt::new(
            [0xA5; 16],
            ReceiptStatus::Delivered,
        )))?;
        round_trip(&AppPayload::Receipt(Receipt::new(
            [1; 16],
            ReceiptStatus::Read,
        )))
    }

    #[test]
    fn location_max_size_is_nineteen_bytes() -> Result<(), PayloadError> {
        let update = LocationUpdate::new(
            LocationPrecision::Precise,
            TS,
            MAX_LATITUDE_E6,
            MAX_LONGITUDE_E6,
            u16::MAX,
            Some(3600),
        )?;
        let bytes = AppPayload::Location(update).encode()?;
        assert_eq!(bytes.len(), 19);
        assert!(bytes.len() <= 20);
        assert!(bytes.len() <= MAX_ENCODED_LEN);
        Ok(())
    }

    #[test]
    fn per_variant_caps_fit_inside_mesh_payload() -> Result<(), PayloadError> {
        let text = "x".repeat(MAX_TEXT_BYTES);
        let name = "n".repeat(MAX_NAME_BYTES);
        let bio = "b".repeat(MAX_BIO_BYTES);
        let largest = [
            AppPayload::Text(TextMessage::new(&text)?),
            AppPayload::Profile(Profile::new(&name, &bio, Some([0; 8]))?),
            AppPayload::Location(LocationUpdate::new(
                LocationPrecision::Precise,
                TS,
                LAT_E6,
                LON_E6,
                1,
                Some(1),
            )?),
            AppPayload::Receipt(Receipt::new([0; 16], ReceiptStatus::Read)),
        ];
        for payload in &largest {
            let len = payload.encode()?.len();
            assert!(len <= MAX_ENCODED_LEN);
        }
        Ok(())
    }

    #[test]
    fn approx_100m_never_encodes_precise_coordinates() -> Result<(), PayloadError> {
        let precise = AppPayload::Location(LocationUpdate::new(
            LocationPrecision::Precise,
            TS,
            LAT_E6,
            LON_E6,
            3,
            None,
        )?)
        .encode()?;
        let approx = AppPayload::Location(LocationUpdate::new(
            LocationPrecision::Approx100m,
            TS,
            LAT_E6,
            LON_E6,
            3,
            None,
        )?)
        .encode()?;

        let precise_lat = LAT_E6.to_be_bytes();
        let precise_lon = LON_E6.to_be_bytes();
        assert!(contains_slice(&precise[7..11], &precise_lat));
        assert!(contains_slice(&precise[11..15], &precise_lon));
        assert!(!contains_slice(&approx, &precise_lat));
        assert!(!contains_slice(&approx, &precise_lon));

        let AppPayload::Location(decoded) = AppPayload::decode(&approx)? else {
            return Err(PayloadError::UnknownKind(KIND_LOCATION));
        };
        assert_ne!(decoded.latitude_e6(), Some(LAT_E6));
        assert_ne!(decoded.longitude_e6(), Some(LON_E6));
        assert_eq!(
            decoded.latitude_e6().ok_or(PayloadError::InvalidLatitude)?,
            quantize_e6(LAT_E6)
        );
        Ok(())
    }

    #[test]
    fn off_never_encodes_coordinates() -> Result<(), PayloadError> {
        let precise = AppPayload::Location(LocationUpdate::new(
            LocationPrecision::Precise,
            TS,
            LAT_E6,
            LON_E6,
            3,
            None,
        )?)
        .encode()?;
        let off_update =
            LocationUpdate::new(LocationPrecision::Off, TS, LAT_E6, LON_E6, 3, Some(42))?;
        assert_eq!(off_update.latitude_e6(), None);
        assert_eq!(off_update.longitude_e6(), None);
        assert_eq!(off_update.accuracy_m(), None);
        assert_eq!(off_update.heading_tenths(), None);

        let off = AppPayload::Location(off_update).encode()?;
        assert_eq!(off.len(), OFF_ENCODED_LEN);
        assert!(!contains_slice(&off, &LAT_E6.to_be_bytes()));
        assert!(!contains_slice(&off, &LON_E6.to_be_bytes()));
        assert!(!contains_slice(&off, &precise[7..11]));
        assert!(!contains_slice(&off, &precise[11..15]));

        let AppPayload::Location(decoded) = AppPayload::decode(&off)? else {
            return Err(PayloadError::UnknownKind(KIND_LOCATION));
        };
        assert_eq!(decoded.latitude_e6(), None);
        assert_eq!(decoded.longitude_e6(), None);
        Ok(())
    }

    #[test]
    fn malformed_truncated_input_is_rejected() -> Result<(), PayloadError> {
        let text = AppPayload::Text(TextMessage::new("hi")?).encode()?;
        let needs = [2, 2, 4, 4, 6, 6];
        for (prefix, need) in (0..text.len()).zip(needs) {
            assert_eq!(
                AppPayload::decode(&text[..prefix]),
                Err(PayloadError::Truncated { need, have: prefix })
            );
        }

        let location = AppPayload::Location(LocationUpdate::new(
            LocationPrecision::Precise,
            TS,
            LAT_E6,
            LON_E6,
            3,
            Some(1),
        )?)
        .encode()?;
        assert_eq!(
            AppPayload::decode(&location[..16]),
            Err(PayloadError::Truncated { need: 19, have: 16 })
        );

        let profile = AppPayload::Profile(Profile::new("n", "", None)?).encode()?;
        assert_eq!(
            AppPayload::decode(&profile[..2]),
            Err(PayloadError::Truncated { need: 4, have: 2 })
        );
        let receipt = AppPayload::Receipt(Receipt::new([0; 16], ReceiptStatus::Read)).encode()?;
        assert_eq!(
            AppPayload::decode(&receipt[..2]),
            Err(PayloadError::Truncated { need: 3, have: 2 })
        );
        Ok(())
    }

    #[test]
    fn malformed_trailing_input_is_rejected() -> Result<(), PayloadError> {
        let mut bytes = AppPayload::Text(TextMessage::new("hi")?).encode()?;
        bytes.push(0);
        assert_eq!(
            AppPayload::decode(&bytes),
            Err(PayloadError::Trailing {
                expected: 6,
                actual: 7
            })
        );

        let mut off = vec![PAYLOAD_VERSION, KIND_LOCATION, 2, 0, 0, 0, 0];
        off.push(0);
        assert_eq!(
            AppPayload::decode(&off),
            Err(PayloadError::Trailing {
                expected: OFF_ENCODED_LEN,
                actual: 8
            })
        );
        Ok(())
    }

    #[test]
    fn malformed_version_and_kind_are_rejected() {
        assert_eq!(
            AppPayload::decode(&[2, KIND_TEXT, 0, 1, b'x']),
            Err(PayloadError::UnsupportedVersion(2))
        );
        assert_eq!(
            AppPayload::decode(&[PAYLOAD_VERSION, 9, 0, 1, b'x']),
            Err(PayloadError::UnknownKind(9))
        );
    }

    #[test]
    fn malformed_flags_and_status_are_rejected() {
        let mut location = vec![PAYLOAD_VERSION, KIND_LOCATION, 0];
        location.extend_from_slice(&TS.to_be_bytes());
        location.extend_from_slice(&[0; 12]);
        location[2] = 0b0100_0000;
        assert_eq!(
            AppPayload::decode(&location),
            Err(PayloadError::InvalidFlags(0b0100_0000))
        );
        location[2] = 3;
        assert_eq!(
            AppPayload::decode(&location),
            Err(PayloadError::UnknownPrecision(3))
        );

        let mut off = vec![PAYLOAD_VERSION, KIND_LOCATION, 2 | HEADING_FLAG];
        off.extend_from_slice(&TS.to_be_bytes());
        assert_eq!(
            AppPayload::decode(&off),
            Err(PayloadError::InvalidFlags(2 | HEADING_FLAG))
        );

        let mut profile = vec![PAYLOAD_VERSION, KIND_PROFILE, 0b0000_0010, 0, 0];
        assert_eq!(
            AppPayload::decode(&profile),
            Err(PayloadError::InvalidFlags(0b0000_0010))
        );
        profile[2] = AVATAR_FLAG;
        assert_eq!(
            AppPayload::decode(&profile),
            Err(PayloadError::Truncated { need: 13, have: 5 })
        );

        let mut receipt = vec![PAYLOAD_VERSION, KIND_RECEIPT, 0];
        receipt.extend_from_slice(&[0; 16]);
        assert_eq!(
            AppPayload::decode(&receipt),
            Err(PayloadError::UnknownReceiptStatus(0))
        );
    }

    #[test]
    fn malformed_oversize_input_is_rejected() -> Result<(), PayloadError> {
        let huge = vec![0u8; MAX_ENCODED_LEN + 1];
        assert_eq!(
            AppPayload::decode(&huge),
            Err(PayloadError::TooLarge {
                len: MAX_ENCODED_LEN + 1,
                max: MAX_ENCODED_LEN
            })
        );

        let mut text = vec![PAYLOAD_VERSION, KIND_TEXT];
        let oversize_len = (MAX_TEXT_BYTES + 1) as u16;
        text.extend_from_slice(&oversize_len.to_be_bytes());
        text.extend_from_slice(&vec![b'x'; MAX_TEXT_BYTES + 1]);
        assert_eq!(
            AppPayload::decode(&text),
            Err(PayloadError::TextTooLong {
                len: MAX_TEXT_BYTES + 1,
                max: MAX_TEXT_BYTES
            })
        );
        Ok(())
    }

    #[test]
    fn malformed_invalid_utf8_is_rejected() -> Result<(), PayloadError> {
        let mut text = vec![PAYLOAD_VERSION, KIND_TEXT, 0, 1, 0xFF];
        assert_eq!(AppPayload::decode(&text), Err(PayloadError::InvalidUtf8));

        let profile = vec![PAYLOAD_VERSION, KIND_PROFILE, 0, 2, 0xC3, 0x28, 0];
        assert_eq!(AppPayload::decode(&profile), Err(PayloadError::InvalidUtf8));
        text[4] = b'x';
        assert!(AppPayload::decode(&text).is_ok());
        Ok(())
    }

    #[test]
    fn unquantized_approximate_coordinates_are_rejected() -> Result<(), PayloadError> {
        let mut bytes = vec![PAYLOAD_VERSION, KIND_LOCATION, 1];
        bytes.extend_from_slice(&TS.to_be_bytes());
        bytes.extend_from_slice(&LAT_E6.to_be_bytes());
        bytes.extend_from_slice(&LON_E6.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        assert_eq!(
            AppPayload::decode(&bytes),
            Err(PayloadError::UnquantizedLatitude)
        );

        let quantized_lat = quantize_e6(LAT_E6);
        bytes[7..11].copy_from_slice(&quantized_lat.to_be_bytes());
        assert_eq!(
            AppPayload::decode(&bytes),
            Err(PayloadError::UnquantizedLongitude)
        );

        let quantized_lon = quantize_e6(LON_E6);
        bytes[11..15].copy_from_slice(&quantized_lon.to_be_bytes());
        assert_eq!(
            AppPayload::decode(&bytes),
            Err(PayloadError::AccuracyTooOptimistic)
        );
        bytes[15..17].copy_from_slice(&APPROX_100M_ACCURACY_M.to_be_bytes());
        assert!(AppPayload::decode(&bytes).is_ok());
        Ok(())
    }

    #[test]
    fn invalid_constructor_inputs_are_rejected() {
        assert_eq!(
            LocationUpdate::new(
                LocationPrecision::Precise,
                TS,
                MAX_LATITUDE_E6 + 1,
                0,
                1,
                None
            ),
            Err(PayloadError::InvalidLatitude)
        );
        assert_eq!(
            LocationUpdate::new(
                LocationPrecision::Precise,
                TS,
                0,
                -(MAX_LONGITUDE_E6 + 1),
                1,
                None
            ),
            Err(PayloadError::InvalidLongitude)
        );
        assert_eq!(
            LocationUpdate::new(LocationPrecision::Precise, TS, 0, 0, 0, None),
            Err(PayloadError::InvalidAccuracy)
        );
        assert_eq!(
            LocationUpdate::new(LocationPrecision::Precise, TS, 0, 0, 1, Some(3601)),
            Err(PayloadError::InvalidHeading)
        );
        assert_eq!(TextMessage::new(""), Err(PayloadError::EmptyText));
        assert_eq!(
            Profile::new(&"n".repeat(MAX_NAME_BYTES + 1), "", None),
            Err(PayloadError::NameTooLong {
                len: MAX_NAME_BYTES + 1,
                max: MAX_NAME_BYTES
            })
        );
        assert_eq!(
            Profile::new("", &"b".repeat(MAX_BIO_BYTES + 1), None),
            Err(PayloadError::BioTooLong {
                len: MAX_BIO_BYTES + 1,
                max: MAX_BIO_BYTES
            })
        );
    }

    #[test]
    fn packet_class_mapping_uses_real_mesh_classes() -> Result<(), PayloadError> {
        assert_eq!(
            AppPayload::Text(TextMessage::new("hi")?).packet_class(),
            PacketClass::Direct
        );
        assert_eq!(
            AppPayload::Location(LocationUpdate::new(
                LocationPrecision::Off,
                TS,
                0,
                0,
                0,
                None
            )?)
            .packet_class(),
            PacketClass::Telemetry
        );
        assert_eq!(
            AppPayload::Profile(Profile::new("n", "", None)?).packet_class(),
            PacketClass::Presence
        );
        assert_eq!(
            AppPayload::Receipt(Receipt::new([0; 16], ReceiptStatus::Delivered)).packet_class(),
            PacketClass::Direct
        );
        Ok(())
    }

    fn contains_slice(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }
}
