//! Jan signed discovery records.
//!
//! Jan is the networking engine used by this app. A discovery record is a
//! user-chosen username plus a locally generated random ten-digit number. It
//! is carried in the existing signed `Presence` profile payload, so aoMesh
//! authenticates the announcing node without a new wire format and no central
//! server is involved.
//!
//! A record is broadcast only while the user has opted in. Receivers keep it
//! in a bounded directory keyed by the sender's 16-byte node id. The key-derived
//! node id remains authoritative; a repeated discovery number never merges two
//! identities.

use std::collections::{HashMap, VecDeque};

use rand_core::RngCore;

use crate::payload::{AppPayload, PayloadError, Profile};

/// Unique marker at the start of a Jan discovery bio.
pub const JAN_BIO_PREFIX: &str = "jan:v1:";

/// Length of a Jan discovery number in decimal digits.
pub const PHONE_DIGITS: usize = 10;

/// Default upper bound on discovery records kept by one device.
pub const DEFAULT_DIRECTORY_CAPACITY: usize = 256;

/// Upper bound accepted by [`NodeDirectory::new`].
pub const MAX_DIRECTORY_CAPACITY: usize = 4096;

/// A node identifier, opaque to this module.
pub type NodeId = [u8; 16];

/// Input rejected while creating or searching a Jan discovery record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    /// The username was empty, contained control characters or exceeded 32 bytes.
    InvalidUsername {
        /// Number of bytes in the supplied username.
        len: usize,
    },
    /// The number was not exactly ten ASCII digits.
    InvalidNumber {
        /// Number of bytes in the supplied number.
        len: usize,
    },
    /// A directory capacity was zero or larger than [`MAX_DIRECTORY_CAPACITY`].
    InvalidCapacity {
        /// The rejected capacity.
        capacity: usize,
    },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUsername { len } => {
                write!(f, "invalid Jan username of {len} bytes")
            }
            Self::InvalidNumber { len } => write!(f, "invalid Jan number of {len} bytes"),
            Self::InvalidCapacity { capacity } => {
                write!(f, "invalid Jan directory capacity {capacity}")
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// A signed, opt-in identity hint announced over Jan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryRecord {
    username: String,
    number: [u8; PHONE_DIGITS],
}

impl DiscoveryRecord {
    /// Validates a user-chosen username.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::InvalidUsername`] when the name is empty,
    /// contains a control character or is longer than the profile name cap.
    pub fn validate_username(username: &str) -> Result<(), DiscoveryError> {
        let len = username.len();
        if len == 0 || len > crate::payload::MAX_NAME_BYTES {
            return Err(DiscoveryError::InvalidUsername { len });
        }
        if username.chars().any(char::is_control) {
            return Err(DiscoveryError::InvalidUsername { len });
        }
        Ok(())
    }

    /// Converts a ten-digit decimal string into its canonical byte form.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::InvalidNumber`] unless every byte is an ASCII
    /// digit and the input is exactly [`PHONE_DIGITS`] bytes long.
    pub fn parse_number(number: &str) -> Result<[u8; PHONE_DIGITS], DiscoveryError> {
        let bytes = number.as_bytes();
        if bytes.len() != PHONE_DIGITS || !bytes.iter().all(u8::is_ascii_digit) {
            return Err(DiscoveryError::InvalidNumber { len: bytes.len() });
        }
        let mut out = [0u8; PHONE_DIGITS];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    /// Builds a validated record.
    ///
    /// # Errors
    ///
    /// Returns a [`DiscoveryError`] if the username or number is invalid.
    pub fn new(username: &str, number: &str) -> Result<Self, DiscoveryError> {
        Self::validate_username(username)?;
        Ok(Self {
            username: username.to_owned(),
            number: Self::parse_number(number)?,
        })
    }

    /// Generates a uniformly distributed ten-digit number from an injected RNG.
    ///
    /// Rejection sampling avoids the bias of reducing a byte modulo ten. The
    /// RNG is injected so the caller owns all randomness and tests are
    /// deterministic.
    #[must_use]
    pub fn random_number(rng: &mut impl RngCore) -> String {
        let mut out = String::with_capacity(PHONE_DIGITS);
        while out.len() < PHONE_DIGITS {
            let draw = (rng.next_u32() & 0xff) as u8;
            if draw < 250 {
                out.push(char::from(b'0' + (draw % 10)));
            }
        }
        out
    }

    /// The user-chosen username.
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// The ten-digit discovery number as text.
    #[must_use]
    pub fn number(&self) -> String {
        self.number.iter().map(|d| char::from(*d)).collect()
    }

    /// Encodes this record as the profile payload of a signed Presence packet.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError`] if a validated field cannot be encoded, which
    /// cannot happen for a constructed or decoded record.
    pub fn to_payload(&self) -> Result<AppPayload, PayloadError> {
        let bio = format!("{JAN_BIO_PREFIX}{}", self.number());
        Ok(AppPayload::Profile(Profile::new(
            &self.username,
            &bio,
            None,
        )?))
    }

    /// Extracts a Jan record from a decoded application payload.
    ///
    /// Returns `None` for ordinary profiles and for other payload classes.
    /// A malformed Jan bio is ignored instead of being treated as a peer record.
    #[must_use]
    pub fn from_payload(payload: &AppPayload) -> Option<Self> {
        let AppPayload::Profile(profile) = payload else {
            return None;
        };
        let bio = profile.bio();
        let encoded = bio.strip_prefix(JAN_BIO_PREFIX)?;
        let username = profile.display_name();
        if Self::validate_username(username).is_err() || encoded.len() != PHONE_DIGITS {
            return None;
        }
        let number = Self::parse_number(encoded).ok()?;
        Some(Self {
            username: username.to_owned(),
            number,
        })
    }

    /// Decodes raw payload bytes and extracts a Jan record, if present.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError`] when the bytes are not a valid app payload.
    pub fn decode_payload(bytes: &[u8]) -> Result<Option<Self>, PayloadError> {
        Ok(Self::from_payload(&AppPayload::decode(bytes)?))
    }
}

/// One directory entry: the authenticated sender and its signed hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    node_id: NodeId,
    record: DiscoveryRecord,
}

impl DirectoryEntry {
    /// The sender's key-derived node id.
    #[must_use]
    pub const fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// The signed discovery hint from that sender.
    #[must_use]
    pub const fn record(&self) -> &DiscoveryRecord {
        &self.record
    }
}

/// Bounded directory of recently seen Jan discovery records.
#[derive(Debug)]
pub struct NodeDirectory {
    capacity: usize,
    entries: HashMap<NodeId, DirectoryEntry>,
    order: VecDeque<NodeId>,
}

impl NodeDirectory {
    /// Creates a directory with an explicit bounded capacity.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::InvalidCapacity`] if `capacity` is zero or
    /// above [`MAX_DIRECTORY_CAPACITY`].
    pub fn new(capacity: usize) -> Result<Self, DiscoveryError> {
        if capacity == 0 || capacity > MAX_DIRECTORY_CAPACITY {
            return Err(DiscoveryError::InvalidCapacity { capacity });
        }
        Ok(Self {
            capacity,
            entries: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        })
    }

    /// Creates a directory with the tested default capacity.
    #[must_use]
    pub fn with_default_capacity() -> Self {
        // Construct directly: the default is a compile-time constant inside
        // the range checked by `new`, so there is no failure path here.
        Self {
            capacity: DEFAULT_DIRECTORY_CAPACITY,
            entries: HashMap::with_capacity(DEFAULT_DIRECTORY_CAPACITY),
            order: VecDeque::with_capacity(DEFAULT_DIRECTORY_CAPACITY),
        }
    }

    /// Configured maximum number of records.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of records currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the directory is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Inserts or refreshes a record, evicting the least recently inserted
    /// sender when the directory is full.
    pub fn insert(&mut self, node_id: NodeId, record: DiscoveryRecord) {
        if self.entries.remove(&node_id).is_some() {
            // A refresh moves the sender to the newest position. Keeping the
            // old position would let one node occupy two order slots and
            // prevent the oldest actual sender from being evicted.
            self.order.retain(|id| *id != node_id);
        } else if self.entries.len() == self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        self.order.push_back(node_id);
        self.entries
            .insert(node_id, DirectoryEntry { node_id, record });
    }

    /// Removes one sender's record.
    pub fn remove(&mut self, node_id: &NodeId) -> Option<DirectoryEntry> {
        let removed = self.entries.remove(node_id);
        if removed.is_some() {
            self.order.retain(|id| id != node_id);
        }
        removed
    }

    /// Returns one sender's record.
    #[must_use]
    pub fn get(&self, node_id: &NodeId) -> Option<&DirectoryEntry> {
        self.entries.get(node_id)
    }

    /// Returns every entry, oldest first.
    #[must_use]
    pub fn all(&self) -> Vec<DirectoryEntry> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id).cloned())
            .collect()
    }

    /// Searches by username, number, or both.
    ///
    /// A `None` field is unconstrained. Username matching is ASCII
    /// case-insensitive; number matching is exact. Supplying both requires both
    /// to match, which makes a colliding ten-digit number harmless.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] if a supplied username or number is invalid.
    pub fn search(
        &self,
        username: Option<&str>,
        number: Option<&str>,
    ) -> Result<Vec<DirectoryEntry>, DiscoveryError> {
        if let Some(name) = username {
            DiscoveryRecord::validate_username(name)?;
        }
        let wanted_number = match number {
            Some(value) => Some(DiscoveryRecord::parse_number(value)?),
            None => None,
        };
        Ok(self
            .all()
            .into_iter()
            .filter(|entry| {
                let name_ok = username
                    .is_none_or(|wanted| entry.record.username.eq_ignore_ascii_case(wanted));
                let number_ok = wanted_number.is_none_or(|wanted| entry.record.number == wanted);
                name_ok && number_ok
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::MAX_ENCODED_LEN;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn record() -> DiscoveryRecord {
        DiscoveryRecord::new("Asha", "0123456789").expect("test record is valid")
    }

    fn node(byte: u8) -> NodeId {
        [byte; 16]
    }

    #[test]
    fn profile_round_trip_keeps_both_fields() {
        let record = record();
        let payload = record.to_payload().expect("payload encodes");
        assert_eq!(
            payload.packet_class(),
            ao_mesh::packet::PacketClass::Presence
        );
        let bytes = payload.encode().expect("payload encodes");
        assert!(bytes.len() <= MAX_ENCODED_LEN);
        let decoded = DiscoveryRecord::decode_payload(&bytes)
            .expect("payload decodes")
            .expect("record is present");
        assert_eq!(decoded, record);
        assert_eq!(decoded.username(), "Asha");
        assert_eq!(decoded.number(), "0123456789");
    }

    #[test]
    fn ordinary_profile_is_not_a_discovery_record() {
        let profile = Profile::new("Asha", "just a bio", None).expect("profile is valid");
        let payload = AppPayload::Profile(profile);
        assert_eq!(DiscoveryRecord::from_payload(&payload), None);
        assert_eq!(
            DiscoveryRecord::from_payload(&AppPayload::Receipt(crate::payload::Receipt::new(
                [7; 16],
                crate::payload::ReceiptStatus::Delivered
            ))),
            None
        );
    }

    #[test]
    fn malformed_jan_bio_is_ignored() {
        let profile = Profile::new("Asha", "jan:v1:123", None).expect("profile is valid");
        assert_eq!(
            DiscoveryRecord::from_payload(&AppPayload::Profile(profile)),
            None
        );
        let profile = Profile::new("Asha", "jan:v1:123456789x", None).expect("profile is valid");
        assert_eq!(
            DiscoveryRecord::from_payload(&AppPayload::Profile(profile)),
            None
        );
    }

    #[test]
    fn validates_username_and_number() {
        assert_eq!(
            DiscoveryRecord::new("", "0123456789"),
            Err(DiscoveryError::InvalidUsername { len: 0 })
        );
        assert_eq!(
            DiscoveryRecord::new("bad\nname", "0123456789"),
            Err(DiscoveryError::InvalidUsername { len: 8 })
        );
        assert_eq!(
            DiscoveryRecord::new("Asha", "012345678"),
            Err(DiscoveryError::InvalidNumber { len: 9 })
        );
        assert_eq!(
            DiscoveryRecord::new("Asha", "012345678x"),
            Err(DiscoveryError::InvalidNumber { len: 10 })
        );
    }

    #[test]
    fn random_number_is_ten_digits_for_every_seeded_run() {
        let mut rng = StdRng::seed_from_u64(0x4a_41_4e);
        for _ in 0..128 {
            let number = DiscoveryRecord::random_number(&mut rng);
            assert_eq!(number.len(), PHONE_DIGITS);
            assert!(number.bytes().all(|byte| byte.is_ascii_digit()));
            assert!(DiscoveryRecord::new("Seeded", &number).is_ok());
        }
    }

    #[test]
    fn search_accepts_username_number_or_both() {
        let mut directory = NodeDirectory::with_default_capacity();
        directory.insert(node(1), record());
        directory.insert(
            node(2),
            DiscoveryRecord::new("asha", "9876543210").expect("test record is valid"),
        );

        let by_name = directory
            .search(Some("ASHA"), None)
            .expect("search input is valid");
        assert_eq!(by_name.len(), 2);

        let by_number = directory
            .search(None, Some("9876543210"))
            .expect("search input is valid");
        assert_eq!(by_number.len(), 1);
        assert_eq!(by_number[0].node_id, node(2));

        let both = directory
            .search(Some("asha"), Some("0123456789"))
            .expect("search input is valid");
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].node_id, node(1));

        let none = directory
            .search(Some("nobody"), Some("0000000000"))
            .expect("search input is valid");
        assert!(none.is_empty());
    }

    #[test]
    fn directory_is_bounded_and_refresh_does_not_evict() {
        let mut directory = NodeDirectory::new(2).expect("capacity is valid");
        directory.insert(node(1), record());
        directory.insert(node(2), record());
        directory.insert(node(1), record());
        assert_eq!(directory.len(), 2);
        assert!(directory.get(&node(1)).is_some());

        directory.insert(node(3), record());
        assert_eq!(directory.len(), 2);
        assert!(directory.get(&node(2)).is_none());
        assert!(directory.get(&node(3)).is_some());

        assert!(matches!(
            NodeDirectory::new(0),
            Err(DiscoveryError::InvalidCapacity { capacity: 0 })
        ));
    }

    #[test]
    fn remove_clears_index_and_search() {
        let mut directory = NodeDirectory::with_default_capacity();
        directory.insert(node(1), record());
        assert!(directory.remove(&node(1)).is_some());
        assert!(directory.is_empty());
        assert!(directory.all().is_empty());
    }
}
