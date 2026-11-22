//! Data lifecycle management (APP-16).
//!
//! The workspace already owns one SQLCipher SQLite store, so this module owns
//! the policy and typed seams, not a second persistence implementation. The
//! SQLite store and Android Keystore bridge are the intended implementers of
//! [`crate::lifecycle::LifecycleStore`] and
//! [`crate::lifecycle::IdentityVault`]; this service applies retention, panic
//! wipe and backup/restore policy through those seams.
//!
//! Identity and contacts use the real aoIdents seams: passphrase-encrypted
//! [`ao_idents::export_backup`]/[`ao_idents::import_backup`] for the seed and a
//! self-addressed [`ao_idents::SealedMessage`] for the bounded contact list.
//! No new cryptography is implemented here. See
//! [`docs/adr/0015-data-lifecycle.md`](../../docs/adr/0015-data-lifecycle.md).

use ao_idents::{IdentError, Identity, NodeId, SealedMessage, export_backup, import_backup};
use rand_core06::{CryptoRng, RngCore};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

/// Default retention entry cap.
pub const DEFAULT_MAX_ENTRIES: usize = 10_000;
/// Default maximum age of a retained message, seven days.
pub const DEFAULT_MAX_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Default maximum contacts in a backup.
pub const MAX_CONTACTS: usize = 256;
/// Maximum bytes in one contact label.
pub const MAX_CONTACT_LABEL_LEN: usize = 128;
/// Maximum bytes in an identity backup blob. The v1 aoIdents blob is fixed
/// at 114 bytes; the cap only bounds parsing of an imported file.
pub const MAX_IDENTITY_BACKUP_LEN: usize = 512;
/// Maximum bytes in the sealed contacts blob.
pub const MAX_CONTACT_BLOB_LEN: usize = 128 * 1024;
/// Maximum size of a parsed backup bundle.
pub const MAX_BACKUP_LEN: usize = MAX_IDENTITY_BACKUP_LEN + MAX_CONTACT_BLOB_LEN + 16;

const BACKUP_VERSION: u8 = 1;
const CONTACTS_VERSION: u8 = 1;
const CONTACT_MSG_ID: [u8; 16] = *b"aoheyra/cbak/v1!";

/// Typed lifecycle failures.
#[derive(Debug)]
pub enum LifecycleError {
    /// A retention bound was zero.
    InvalidConfig,
    /// A backup passphrase was empty.
    EmptyPassphrase,
    /// A bundle or one of its parts exceeded its hard cap.
    PayloadTooLarge {
        /// Offered length.
        len: usize,
        /// Applicable cap.
        max: usize,
    },
    /// The bundle framing was truncated or malformed.
    MalformedBackupBundle,
    /// The bundle used an unknown version.
    UnsupportedBackupVersion(u8),
    /// The contact plaintext was truncated or malformed.
    MalformedContacts,
    /// The contact plaintext used an unknown version.
    UnsupportedContactsVersion(u8),
    /// Too many contacts were offered.
    TooManyContacts {
        /// Offered count.
        len: usize,
        /// Hard cap.
        max: usize,
    },
    /// A contact label exceeded its cap.
    ContactLabelTooLong {
        /// Offered length.
        len: usize,
        /// Hard cap.
        max: usize,
    },
    /// One node id appeared twice in the contact list.
    DuplicateContact,
    /// Extra bytes followed the contact list.
    TrailingContactBytes {
        /// Number of trailing bytes.
        remaining: usize,
    },
    /// The store refused retention.
    StoreRetentionFailed,
    /// The store refused a wipe.
    StoreWipeFailed,
    /// The store refused vacuum.
    StoreVacuumFailed,
    /// The keystore refused identity replacement.
    IdentityReplaceFailed,
    /// The keystore refused identity restore.
    IdentityRestoreFailed,
    /// A contacts blob was sealed by a different identity.
    WrongBackupSender,
    /// aoIdents refused a cryptographic operation.
    Ident(IdentError),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid lifecycle config"),
            Self::EmptyPassphrase => write!(f, "backup passphrase is empty"),
            Self::PayloadTooLarge { len, max } => {
                write!(f, "lifecycle payload {len} exceeds cap {max}")
            }
            Self::MalformedBackupBundle => write!(f, "malformed backup bundle"),
            Self::UnsupportedBackupVersion(version) => {
                write!(f, "unsupported backup version {version}")
            }
            Self::MalformedContacts => write!(f, "malformed contacts backup"),
            Self::UnsupportedContactsVersion(version) => {
                write!(f, "unsupported contacts version {version}")
            }
            Self::TooManyContacts { len, max } => {
                write!(f, "contacts backup has {len} entries, cap is {max}")
            }
            Self::ContactLabelTooLong { len, max } => {
                write!(f, "contact label is {len} bytes, cap is {max}")
            }
            Self::DuplicateContact => write!(f, "duplicate contact in backup"),
            Self::TrailingContactBytes { remaining } => {
                write!(f, "contacts backup has {remaining} trailing bytes")
            }
            Self::StoreRetentionFailed => write!(f, "store refused retention"),
            Self::StoreWipeFailed => write!(f, "store refused wipe"),
            Self::StoreVacuumFailed => write!(f, "store refused vacuum"),
            Self::IdentityReplaceFailed => write!(f, "keystore refused fresh identity"),
            Self::IdentityRestoreFailed => write!(f, "keystore refused identity restore"),
            Self::WrongBackupSender => write!(f, "contacts backup has a different sender"),
            Self::Ident(error) => write!(f, "identity operation failed: {error}"),
        }
    }
}

impl Error for LifecycleError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Ident(error) => Some(error),
            _ => None,
        }
    }
}

impl From<IdentError> for LifecycleError {
    fn from(error: IdentError) -> Self {
        Self::Ident(error)
    }
}

/// Bounded retention settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Maximum age of a retained message, measured with injected time.
    pub max_age_ms: u64,
    /// Maximum number of messages to retain.
    pub max_entries: usize,
}

impl RetentionPolicy {
    /// Validates a policy without silent clamping.
    pub fn new(max_age_ms: u64, max_entries: usize) -> Result<Self, LifecycleError> {
        if max_entries == 0 {
            return Err(LifecycleError::InvalidConfig);
        }
        Ok(Self {
            max_age_ms,
            max_entries,
        })
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_ms: DEFAULT_MAX_AGE_MS,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }
}

/// What retention actually removed, for a redacted report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionReport {
    /// Messages removed by age.
    pub expired: u64,
    /// Messages removed by the entry cap.
    pub over_cap: u64,
    /// Bytes reported reclaimed by the store.
    pub bytes_reclaimed: u64,
}

/// What a panic wipe completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WipeReport {
    /// The database was wiped.
    pub store_wiped: bool,
    /// A fresh identity replaced the old keystore entry.
    pub identity_replaced: bool,
    /// The store was vacuumed after the wipe.
    pub vacuumed: bool,
}

/// The app-owned database seam. APP-04 supplies the real implementation.
pub trait LifecycleStore {
    /// Applies retention and reports what was removed.
    fn retain(
        &mut self,
        policy: &RetentionPolicy,
        now_ms: u64,
    ) -> Result<RetentionReport, LifecycleError>;

    /// Removes all rows and zeroes any in-memory plaintext copies.
    fn wipe(&mut self) -> Result<(), LifecycleError>;

    /// Reclaims storage after deletion.
    fn vacuum(&mut self) -> Result<(), LifecycleError>;
}

/// The app-owned keystore seam. Android Keystore supplies the real one.
pub trait IdentityVault {
    /// The current identity used for backup and contact sealing.
    fn identity(&self) -> &Identity;

    /// Destroys the old key material and installs a fresh seeded identity.
    fn replace_with_fresh_identity<R>(&mut self, rng: &mut R) -> Result<Identity, LifecycleError>
    where
        R: RngCore + CryptoRng;

    /// Installs an identity restored from a verified backup.
    fn restore_identity(&mut self, identity: Identity) -> Result<(), LifecycleError>;
}

/// One backup contact. `Debug` never prints the label.
#[derive(Clone, PartialEq, Eq)]
pub struct ContactRecord {
    /// Opaque verified node id.
    pub node_id: NodeId,
    /// Opaque app label; never interpreted or logged here.
    pub label: Vec<u8>,
}

impl ContactRecord {
    /// Validates one record before it enters a backup.
    pub fn new(node_id: NodeId, label: Vec<u8>) -> Result<Self, LifecycleError> {
        if label.len() > MAX_CONTACT_LABEL_LEN {
            return Err(LifecycleError::ContactLabelTooLong {
                len: label.len(),
                max: MAX_CONTACT_LABEL_LEN,
            });
        }
        Ok(Self { node_id, label })
    }
}

impl fmt::Debug for ContactRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContactRecord")
            .field("node_id", &self.node_id)
            .field("label_len", &self.label.len())
            .finish()
    }
}

/// An encrypted backup bundle: aoIdents identity blob plus sealed contacts.
#[derive(Clone, PartialEq, Eq)]
pub struct BackupBundle {
    /// Passphrase-encrypted identity seed from aoIdents, as opaque bytes.
    pub identity: Vec<u8>,
    /// Self-sealed, bounded contact list.
    pub contacts: Vec<u8>,
}

impl fmt::Debug for BackupBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackupBundle")
            .field("identity_len", &self.identity.len())
            .field("contacts_len", &self.contacts.len())
            .finish()
    }
}

impl BackupBundle {
    /// Frames the bundle with a version and explicit part lengths.
    pub fn to_bytes(&self) -> Result<Vec<u8>, LifecycleError> {
        let identity = &self.identity;
        if identity.len() > MAX_IDENTITY_BACKUP_LEN {
            return Err(LifecycleError::PayloadTooLarge {
                len: identity.len(),
                max: MAX_IDENTITY_BACKUP_LEN,
            });
        }
        if self.contacts.len() > MAX_CONTACT_BLOB_LEN {
            return Err(LifecycleError::PayloadTooLarge {
                len: self.contacts.len(),
                max: MAX_CONTACT_BLOB_LEN,
            });
        }
        let len = 1 + 4 + identity.len() + 4 + self.contacts.len();
        if len > MAX_BACKUP_LEN {
            return Err(LifecycleError::PayloadTooLarge {
                len,
                max: MAX_BACKUP_LEN,
            });
        }
        let mut out = Vec::with_capacity(len);
        out.push(BACKUP_VERSION);
        out.extend_from_slice(&(identity.len() as u32).to_be_bytes());
        out.extend_from_slice(identity);
        out.extend_from_slice(&(self.contacts.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.contacts);
        Ok(out)
    }

    /// Parses a framed bundle with every length capped before allocation.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LifecycleError> {
        if bytes.len() > MAX_BACKUP_LEN {
            return Err(LifecycleError::PayloadTooLarge {
                len: bytes.len(),
                max: MAX_BACKUP_LEN,
            });
        }
        if bytes.len() < 9 {
            return Err(LifecycleError::MalformedBackupBundle);
        }
        if bytes[0] != BACKUP_VERSION {
            return Err(LifecycleError::UnsupportedBackupVersion(bytes[0]));
        }
        let identity_len =
            usize::try_from(u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]))
                .map_err(|_| LifecycleError::MalformedBackupBundle)?;
        if identity_len > MAX_IDENTITY_BACKUP_LEN {
            return Err(LifecycleError::PayloadTooLarge {
                len: identity_len,
                max: MAX_IDENTITY_BACKUP_LEN,
            });
        }
        let identity_end = 5usize
            .checked_add(identity_len)
            .ok_or(LifecycleError::MalformedBackupBundle)?;
        if bytes.len() < identity_end + 4 {
            return Err(LifecycleError::MalformedBackupBundle);
        }
        let contacts_len = usize::try_from(u32::from_be_bytes([
            bytes[identity_end],
            bytes[identity_end + 1],
            bytes[identity_end + 2],
            bytes[identity_end + 3],
        ]))
        .map_err(|_| LifecycleError::MalformedBackupBundle)?;
        if contacts_len > MAX_CONTACT_BLOB_LEN {
            return Err(LifecycleError::PayloadTooLarge {
                len: contacts_len,
                max: MAX_CONTACT_BLOB_LEN,
            });
        }
        let contacts_end = identity_end
            .checked_add(4)
            .and_then(|start| start.checked_add(contacts_len))
            .ok_or(LifecycleError::MalformedBackupBundle)?;
        if bytes.len() != contacts_end {
            return Err(LifecycleError::MalformedBackupBundle);
        }
        Ok(Self {
            identity: bytes[5..identity_end].to_vec(),
            contacts: bytes[identity_end + 4..contacts_end].to_vec(),
        })
    }
}

/// What a restore installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    /// Contacts opened from the self-sealed blob.
    pub contacts: Vec<ContactRecord>,
    /// The node id of the restored identity.
    pub node_id: NodeId,
}

/// Retention, wipe and backup policy over injected seams.
#[derive(Debug)]
pub struct LifecycleService<S, V> {
    store: S,
    vault: V,
    retention: RetentionPolicy,
}

impl<S, V> LifecycleService<S, V>
where
    S: LifecycleStore,
    V: IdentityVault,
{
    /// Builds a service with a validated retention policy.
    pub fn new(store: S, vault: V, retention: RetentionPolicy) -> Self {
        Self {
            store,
            vault,
            retention,
        }
    }

    /// The currently configured policy.
    #[must_use]
    pub const fn retention(&self) -> &RetentionPolicy {
        &self.retention
    }

    /// Replaces the policy after validation by the caller.
    pub fn set_retention(&mut self, retention: RetentionPolicy) {
        self.retention = retention;
    }

    /// Applies the current policy using injected time.
    pub fn apply_retention(&mut self, now_ms: u64) -> Result<RetentionReport, LifecycleError> {
        self.store.retain(&self.retention, now_ms)
    }

    /// Wipes data, replaces the identity and vacuums the store.
    ///
    /// The store is wiped first: if keystore replacement fails, no old data
    /// remains readable. A partial failure is an error, never a success.
    pub fn panic_wipe<R>(&mut self, rng: &mut R) -> Result<WipeReport, LifecycleError>
    where
        R: RngCore + CryptoRng,
    {
        self.store.wipe()?;
        self.vault.replace_with_fresh_identity(rng)?;
        self.store.vacuum()?;
        Ok(WipeReport {
            store_wiped: true,
            identity_replaced: true,
            vacuumed: true,
        })
    }

    /// Exports the identity and a self-sealed contacts list.
    pub fn backup<R>(
        &self,
        contacts: &[ContactRecord],
        passphrase: &str,
        rng: &mut R,
    ) -> Result<BackupBundle, LifecycleError>
    where
        R: RngCore + CryptoRng,
    {
        if passphrase.is_empty() {
            return Err(LifecycleError::EmptyPassphrase);
        }
        let identity = self.vault.identity();
        let identity_blob = export_backup(identity, passphrase, rng)?;
        let plaintext = encode_contacts(contacts)?;
        let sealed = SealedMessage::seal(
            identity,
            identity.public(),
            &CONTACT_MSG_ID,
            &plaintext,
            rng,
        )?;
        let contacts = sealed.encode();
        if contacts.len() > MAX_CONTACT_BLOB_LEN {
            return Err(LifecycleError::PayloadTooLarge {
                len: contacts.len(),
                max: MAX_CONTACT_BLOB_LEN,
            });
        }
        Ok(BackupBundle {
            identity: identity_blob.as_bytes().to_vec(),
            contacts,
        })
    }

    /// Verifies a bundle, restores the identity, then opens the contacts.
    ///
    /// Contacts are opened only after the identity is known, but they are
    /// fully verified before the vault is changed, so a bad bundle leaves
    /// the current identity untouched.
    pub fn restore(
        &mut self,
        bundle: &BackupBundle,
        passphrase: &str,
    ) -> Result<RestoreReport, LifecycleError> {
        if passphrase.is_empty() {
            return Err(LifecycleError::EmptyPassphrase);
        }
        let identity = import_backup(&bundle.identity, passphrase)?;
        let sealed = SealedMessage::decode(&bundle.contacts)?;
        let opened = sealed.open(&identity, &CONTACT_MSG_ID)?;
        if opened.sender() != identity.public() {
            return Err(LifecycleError::WrongBackupSender);
        }
        let contacts = decode_contacts(opened.plaintext())?;
        let node_id = identity.node_id();
        self.vault.restore_identity(identity)?;
        Ok(RestoreReport { contacts, node_id })
    }
}

fn encode_contacts(contacts: &[ContactRecord]) -> Result<Vec<u8>, LifecycleError> {
    if contacts.len() > MAX_CONTACTS {
        return Err(LifecycleError::TooManyContacts {
            len: contacts.len(),
            max: MAX_CONTACTS,
        });
    }
    let mut out = Vec::new();
    out.push(CONTACTS_VERSION);
    out.extend_from_slice(&(contacts.len() as u16).to_be_bytes());
    for contact in contacts {
        out.extend_from_slice(contact.node_id.as_bytes());
        out.extend_from_slice(&(contact.label.len() as u16).to_be_bytes());
        out.extend_from_slice(&contact.label);
    }
    Ok(out)
}

fn decode_contacts(bytes: &[u8]) -> Result<Vec<ContactRecord>, LifecycleError> {
    if bytes.len() < 3 {
        return Err(LifecycleError::MalformedContacts);
    }
    if bytes[0] != CONTACTS_VERSION {
        return Err(LifecycleError::UnsupportedContactsVersion(bytes[0]));
    }
    let count = usize::from(u16::from_be_bytes([bytes[1], bytes[2]]));
    if count > MAX_CONTACTS {
        return Err(LifecycleError::TooManyContacts {
            len: count,
            max: MAX_CONTACTS,
        });
    }
    let mut seen = BTreeSet::new();
    let mut contacts = Vec::with_capacity(count);
    let mut offset = 3usize;
    for _ in 0..count {
        let node_end = offset
            .checked_add(16)
            .ok_or(LifecycleError::MalformedContacts)?;
        let label_len_end = node_end
            .checked_add(2)
            .ok_or(LifecycleError::MalformedContacts)?;
        if bytes.len() < label_len_end {
            return Err(LifecycleError::MalformedContacts);
        }
        let mut id_bytes = [0u8; 16];
        id_bytes.copy_from_slice(&bytes[offset..node_end]);
        let node_id = NodeId::from_bytes(id_bytes);
        if !seen.insert(node_id) {
            return Err(LifecycleError::DuplicateContact);
        }
        let label_len = usize::from(u16::from_be_bytes([bytes[node_end], bytes[node_end + 1]]));
        if label_len > MAX_CONTACT_LABEL_LEN {
            return Err(LifecycleError::ContactLabelTooLong {
                len: label_len,
                max: MAX_CONTACT_LABEL_LEN,
            });
        }
        let label_end = label_len_end
            .checked_add(label_len)
            .ok_or(LifecycleError::MalformedContacts)?;
        if bytes.len() < label_end {
            return Err(LifecycleError::MalformedContacts);
        }
        contacts.push(ContactRecord {
            node_id,
            label: bytes[label_len_end..label_end].to_vec(),
        });
        offset = label_end;
    }
    if offset != bytes.len() {
        return Err(LifecycleError::TrailingContactBytes {
            remaining: bytes.len() - offset,
        });
    }
    Ok(contacts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    #[derive(Debug)]
    struct TestRng(u64);

    impl RngCore for TestRng {
        fn next_u32(&mut self) -> u32 {
            self.next_u64() as u32
        }

        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            self.0
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for byte in dest {
                *byte = self.next_u64() as u8;
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core06::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    impl CryptoRng for TestRng {}

    #[derive(Clone)]
    struct MemoryStore {
        rows: Vec<(u64, Zeroizing<Vec<u8>>)>,
        vacuumed: bool,
    }

    impl MemoryStore {
        fn with_rows(rows: &[&str]) -> Self {
            Self {
                rows: rows
                    .iter()
                    .enumerate()
                    .map(|(index, text)| {
                        (index as u64 * 100, Zeroizing::new(text.as_bytes().to_vec()))
                    })
                    .collect(),
                vacuumed: false,
            }
        }
    }

    impl LifecycleStore for MemoryStore {
        fn retain(
            &mut self,
            policy: &RetentionPolicy,
            now_ms: u64,
        ) -> Result<RetentionReport, LifecycleError> {
            let before = self.rows.len();
            self.rows
                .retain(|(at, _)| now_ms.saturating_sub(*at) <= policy.max_age_ms);
            let expired = before.saturating_sub(self.rows.len()) as u64;
            let over_cap = self.rows.len().saturating_sub(policy.max_entries) as u64;
            if over_cap > 0 {
                self.rows.truncate(policy.max_entries);
            }
            Ok(RetentionReport {
                expired,
                over_cap,
                bytes_reclaimed: 0,
            })
        }

        fn wipe(&mut self) -> Result<(), LifecycleError> {
            self.rows.clear();
            Ok(())
        }

        fn vacuum(&mut self) -> Result<(), LifecycleError> {
            self.vacuumed = true;
            Ok(())
        }
    }

    struct MemoryVault {
        identity: Identity,
        seed: Zeroizing<[u8; 32]>,
    }

    impl MemoryVault {
        fn new(seed: [u8; 32]) -> Self {
            Self {
                identity: Identity::from_seed(seed),
                seed: Zeroizing::new(seed),
            }
        }
    }

    impl IdentityVault for MemoryVault {
        fn identity(&self) -> &Identity {
            &self.identity
        }

        fn replace_with_fresh_identity<R>(
            &mut self,
            rng: &mut R,
        ) -> Result<Identity, LifecycleError>
        where
            R: RngCore + CryptoRng,
        {
            let mut seed = Zeroizing::new([0u8; 32]);
            rng.fill_bytes(&mut *seed);
            self.identity = Identity::from_seed(*seed);
            self.seed = seed;
            Ok(Identity::from_seed(*self.seed))
        }

        fn restore_identity(&mut self, identity: Identity) -> Result<(), LifecycleError> {
            let mut seed = Zeroizing::new([0u8; 32]);
            // The restored seed is not exposed by aoIdents, so the vault
            // fixture records a derived marker instead. The real keystore
            // owns the authoritative restored key.
            seed[..16].copy_from_slice(identity.node_id().as_bytes());
            self.seed = seed;
            self.identity = identity;
            Ok(())
        }
    }

    fn node(byte: u8) -> NodeId {
        NodeId::from_bytes([byte; 16])
    }

    fn contacts() -> Vec<ContactRecord> {
        vec![
            ContactRecord::new(node(2), b"home".to_vec()).expect("valid label"),
            ContactRecord::new(node(3), b"work".to_vec()).expect("valid label"),
        ]
    }

    #[test]
    fn retention_policy_rejects_zero_cap() {
        assert!(matches!(
            RetentionPolicy::new(1, 0),
            Err(LifecycleError::InvalidConfig)
        ));
        assert!(RetentionPolicy::new(1, 1).is_ok());
    }

    #[test]
    fn retention_uses_injected_time() -> Result<(), LifecycleError> {
        let store = MemoryStore::with_rows(&["old", "new"]);
        let mut service = LifecycleService::new(
            store,
            MemoryVault::new([1; 32]),
            RetentionPolicy::new(50, 1)?,
        );
        let report = service.apply_retention(100)?;
        assert_eq!(report.expired, 1);
        assert_eq!(report.over_cap, 0);
        assert_eq!(service.store.rows.len(), 1);
        Ok(())
    }

    #[test]
    fn panic_wipe_restarts_with_a_fresh_identity() -> Result<(), LifecycleError> {
        let store = MemoryStore::with_rows(&["OLD-MESSAGE-MARKER"]);
        let vault = MemoryVault::new([9; 32]);
        let old_identity = vault.identity.node_id();
        let mut service = LifecycleService::new(store, vault, RetentionPolicy::default());
        let report = service.panic_wipe(&mut TestRng(42))?;
        assert!(report.store_wiped && report.identity_replaced && report.vacuumed);

        let persisted_seed = *service.vault.seed;
        let restarted = LifecycleService::new(
            MemoryStore::with_rows(&[]),
            MemoryVault::new(persisted_seed),
            RetentionPolicy::default(),
        );
        assert_ne!(restarted.vault.identity.node_id(), old_identity);
        assert!(restarted.store.rows.is_empty());
        assert!(!restarted.store.vacuumed);
        Ok(())
    }

    #[test]
    fn backup_and_restore_round_trip() -> Result<(), LifecycleError> {
        let service = LifecycleService::new(
            MemoryStore::with_rows(&[]),
            MemoryVault::new([5; 32]),
            RetentionPolicy::default(),
        );
        let original = service.vault.identity.node_id();
        let bundle = service.backup(&contacts(), "correct horse", &mut TestRng(11))?;
        let bytes = bundle.to_bytes()?;
        assert_eq!(BackupBundle::from_bytes(&bytes)?, bundle);

        let mut restored = LifecycleService::new(
            MemoryStore::with_rows(&[]),
            MemoryVault::new([7; 32]),
            RetentionPolicy::default(),
        );
        let report = restored.restore(&BackupBundle::from_bytes(&bytes)?, "correct horse")?;
        assert_eq!(report.node_id, original);
        assert_eq!(report.contacts, contacts());
        assert_eq!(restored.vault.identity.node_id(), original);
        Ok(())
    }

    #[test]
    fn restore_rejects_wrong_passphrase_and_empty_passphrase() -> Result<(), LifecycleError> {
        let mut service = LifecycleService::new(
            MemoryStore::with_rows(&[]),
            MemoryVault::new([5; 32]),
            RetentionPolicy::default(),
        );
        let bundle = service.backup(&contacts(), "correct horse", &mut TestRng(12))?;
        assert!(matches!(
            service.restore(&bundle, ""),
            Err(LifecycleError::EmptyPassphrase)
        ));
        assert!(matches!(
            service.restore(&bundle, "wrong"),
            Err(LifecycleError::Ident(IdentError::InvalidPassphrase))
        ));
        Ok(())
    }

    #[test]
    fn backup_refuses_empty_passphrase_and_oversized_contacts() -> Result<(), LifecycleError> {
        let service = LifecycleService::new(
            MemoryStore::with_rows(&[]),
            MemoryVault::new([5; 32]),
            RetentionPolicy::default(),
        );
        assert!(matches!(
            service.backup(&contacts(), "", &mut TestRng(13)),
            Err(LifecycleError::EmptyPassphrase)
        ));
        let too_many = vec![ContactRecord::new(node(8), Vec::new())?; MAX_CONTACTS + 1];
        assert!(matches!(
            service.backup(&too_many, "pass", &mut TestRng(13)),
            Err(LifecycleError::TooManyContacts { len, max: MAX_CONTACTS }) if len == MAX_CONTACTS + 1
        ));
        assert!(matches!(
            ContactRecord::new(node(8), vec![0; MAX_CONTACT_LABEL_LEN + 1]),
            Err(LifecycleError::ContactLabelTooLong { .. })
        ));
        Ok(())
    }

    #[test]
    fn malformed_bundle_framing_is_typed() {
        assert!(matches!(
            BackupBundle::from_bytes(&[]),
            Err(LifecycleError::MalformedBackupBundle)
        ));
        let mut bytes = vec![BACKUP_VERSION, 0, 0, 0, 0, 0, 0, 0, 0];
        bytes[0] = 9;
        assert!(matches!(
            BackupBundle::from_bytes(&bytes),
            Err(LifecycleError::UnsupportedBackupVersion(9))
        ));
    }

    #[test]
    fn contacts_codec_rejects_duplicates_and_trailing_bytes() -> Result<(), LifecycleError> {
        let mut bytes = encode_contacts(&contacts())?;
        bytes.push(0);
        assert!(matches!(
            decode_contacts(&bytes),
            Err(LifecycleError::TrailingContactBytes { remaining: 1 })
        ));
        let duplicate = vec![contacts()[0].clone(), contacts()[0].clone()];
        assert!(matches!(
            encode_contacts(&duplicate).and_then(|bytes| decode_contacts(&bytes)),
            Err(LifecycleError::DuplicateContact)
        ));
        Ok(())
    }
}
