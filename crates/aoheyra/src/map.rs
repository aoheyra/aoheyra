//! One bounded stream of offline-map items (APP-13).
//!
//! The map does not query several app screens. A host feeds it three kinds
//! of facts: the local node, contact positions from APP-12, and verified
//! directory entries from aoMesh (MSH-20). `poll` returns deterministic
//! change events; `snapshot` renders the current items with an age and a
//! staleness level. `topology` gives the debug overlay neighbour and mesh
//! counts without exposing hello bytes, tags or keys.
//!
//! Nodes are aoIdents `NodeId` values, the same type aoMesh carries in its
//! directory and neighbour tables. Directory location bytes are decoded by
//! APP-07's payload codec; the map never guesses a wire layout.

use ao_idents::NodeId;
use ao_mesh::{DirectoryEntry, NeighborTable, NodeDirectory};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::payload::{AppPayload, LocationUpdate};

/// Maximum number of items with the default config.
pub const DEFAULT_MAX_ITEMS: usize = 256;
/// An entry is fresh below this relative age with the default config.
pub const DEFAULT_STALE_AFTER_MS: u64 = 30_000;
/// An entry is removed at this relative age with the default config.
pub const DEFAULT_REMOVE_AFTER_MS: u64 = 60_000;
/// Maximum queued change events with the default config.
pub const DEFAULT_MAX_CHANGES: usize = 64;

/// Staleness shown by the map. Removal is a change event, not a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    /// Recently updated.
    Fresh,
    /// Still displayed, but marked as old.
    Stale,
}

/// How an item is presented: self, a contact, or a mesh-only node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapKind {
    /// The local node.
    SelfNode,
    /// A contact from aoIdents. Contacts take precedence over mesh nodes.
    Contact,
    /// A node known only through the mesh directory.
    MeshNode,
}

/// One renderable map item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapItem {
    /// Presentation role of this item.
    pub kind: MapKind,
    /// Node the item describes.
    pub node: NodeId,
    /// Last position, if one has been supplied.
    pub location: Option<LocationUpdate>,
    /// Relative age of the item's last update.
    pub age_ms: u64,
    /// Display staleness.
    pub staleness: Staleness,
    /// Estimated hop distance from a directory observation.
    pub hops: Option<u8>,
    /// Neighbour through which the node was learned.
    pub via: Option<NodeId>,
}

/// A typed observation from the node directory bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryFact {
    /// Node the fact describes.
    pub node: NodeId,
    /// Estimated hop distance.
    pub hops: u8,
    /// Neighbour through which the fact was learned.
    pub via: NodeId,
    /// APP-07 position decoded from the directory announcement.
    pub location: Option<LocationUpdate>,
}

/// Kinds of change emitted by the map stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapChangeKind {
    /// A new item appeared.
    Added,
    /// A visible field changed.
    Updated,
    /// The item crossed the stale threshold.
    Stale,
    /// The item was removed at the removal threshold or capacity pressure.
    Removed,
}

/// One coalesced change. Only ids and kinds travel; the UI calls `snapshot`
/// for details, as with aoMesh's directory events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapChange {
    /// Local time of the change.
    pub at_ms: u64,
    /// What happened.
    pub kind: MapChangeKind,
    /// Node affected.
    pub node: NodeId,
}

/// A neighbour row for the debug overlay. No hello bytes or tags are copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeighbourView {
    /// Local link identifier.
    pub link: u64,
    /// Verified mesh identity, absent for a failed introduction.
    pub node: Option<NodeId>,
    /// Relative age of the last received packet.
    pub last_rx_age_ms: Option<u64>,
    /// Structurally valid packets received.
    pub received_packets: u64,
    /// Structurally invalid packets received.
    pub malformed_packets: u64,
    /// Pipeline refusals charged to this link.
    pub rejected_packets: u64,
}

/// A mesh and neighbour topology snapshot for the debug overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologySnapshot {
    /// Effective local time of the snapshot.
    pub at_ms: u64,
    /// Neighbours in link-id order.
    pub neighbours: Vec<NeighbourView>,
    /// Live directory entries at `at_ms`.
    pub mesh_nodes: usize,
    /// Stored directory entries awaiting reclamation.
    pub stored_directory_entries: usize,
}

/// Typed errors for map inputs and configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// A config bound was zero or out of order.
    InvalidConfig,
    /// More contacts were supplied than the item bound allows.
    TooManyContacts {
        /// Offered count.
        len: usize,
        /// Configured maximum.
        max: usize,
    },
    /// Directory location bytes were not an APP-07 location payload.
    InvalidLocationPayload,
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid map config"),
            Self::TooManyContacts { len, max } => {
                write!(f, "{len} contacts exceed the map item bound {max}")
            }
            Self::InvalidLocationPayload => {
                write!(
                    f,
                    "directory location bytes are not an APP-07 location payload"
                )
            }
        }
    }
}

impl std::error::Error for MapError {}

/// Bounds and relative lifetimes for [`MapService`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapConfig {
    /// Hard bound on map items, including the self item.
    pub max_items: usize,
    /// Relative age at which an item is marked stale.
    pub stale_after_ms: u64,
    /// Relative age at which an item is removed; must exceed staleness.
    pub remove_after_ms: u64,
    /// Hard bound on queued change events.
    pub max_changes: usize,
}

impl MapConfig {
    /// Validates and builds a config without silent clamping.
    ///
    /// Returns [`MapError::InvalidConfig`] if any bound is zero or the
    /// removal threshold is not later than the stale threshold.
    pub fn new(
        max_items: usize,
        stale_after_ms: u64,
        remove_after_ms: u64,
        max_changes: usize,
    ) -> Result<Self, MapError> {
        if max_items == 0
            || stale_after_ms == 0
            || remove_after_ms <= stale_after_ms
            || max_changes == 0
        {
            return Err(MapError::InvalidConfig);
        }
        Ok(Self {
            max_items,
            stale_after_ms,
            remove_after_ms,
            max_changes,
        })
    }
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            max_items: DEFAULT_MAX_ITEMS,
            stale_after_ms: DEFAULT_STALE_AFTER_MS,
            remove_after_ms: DEFAULT_REMOVE_AFTER_MS,
            max_changes: DEFAULT_MAX_CHANGES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    node: NodeId,
    location: Option<LocationUpdate>,
    updated_at_ms: u64,
    hops: Option<u8>,
    via: Option<NodeId>,
    staleness: Staleness,
    sequence: u64,
}

impl Record {
    fn age_at(&self, now: u64) -> u64 {
        now.saturating_sub(self.updated_at_ms)
    }
}

/// Deterministic, bounded owner of the map item stream.
#[derive(Debug)]
pub struct MapService {
    config: MapConfig,
    clock_ms: u64,
    self_node: Option<NodeId>,
    contacts: BTreeSet<NodeId>,
    records: BTreeMap<NodeId, Record>,
    changes: VecDeque<MapChange>,
    sequence: u64,
    dropped_changes: u64,
}

impl MapService {
    /// Creates an empty service.
    #[must_use]
    pub fn new(config: MapConfig) -> Self {
        Self {
            config,
            clock_ms: 0,
            self_node: None,
            contacts: BTreeSet::new(),
            records: BTreeMap::new(),
            changes: VecDeque::with_capacity(config.max_changes),
            sequence: 0,
            dropped_changes: 0,
        }
    }

    /// Configured item bound.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.config.max_items
    }

    /// Change events dropped by the bounded queue.
    #[must_use]
    pub const fn dropped_changes(&self) -> u64 {
        self.dropped_changes
    }

    /// Sets the local node. A previous self record is not deleted: it becomes
    /// a normal mesh item until it ages out.
    pub fn set_self(&mut self, now_ms: u64, node: NodeId) {
        let now = self.effective_now(now_ms);
        self.self_node = Some(node);
        let location = self.records.get(&node).and_then(|record| record.location);
        self.upsert(now, node, location, None, None);
    }

    /// Replaces the contact set. Contact presentation wins over mesh-node
    /// presentation for the same node.
    pub fn set_contacts(&mut self, now_ms: u64, nodes: &[NodeId]) -> Result<(), MapError> {
        if nodes.len() > self.config.max_items {
            return Err(MapError::TooManyContacts {
                len: nodes.len(),
                max: self.config.max_items,
            });
        }
        let now = self.effective_now(now_ms);
        let next: BTreeSet<NodeId> = nodes.iter().copied().collect();
        let added: Vec<NodeId> = next.difference(&self.contacts).copied().collect();
        for node in added {
            let before = self.kind_for(node);
            let after = if self.self_node == Some(node) {
                MapKind::SelfNode
            } else {
                MapKind::Contact
            };
            let sequence = self.next_sequence();
            if let Some(record) = self.records.get_mut(&node) {
                record.sequence = sequence;
            }
            if before != after {
                self.push_change(now, MapChangeKind::Updated, node);
            }
        }
        self.contacts = next;
        Ok(())
    }

    /// Records a position from APP-12's location session.
    pub fn set_location(&mut self, now_ms: u64, node: NodeId, location: LocationUpdate) {
        let now = self.effective_now(now_ms);
        self.upsert(now, node, Some(location), None, None);
    }

    /// Records a direct directory fact supplied by the presence bridge.
    pub fn observe_fact(&mut self, now_ms: u64, fact: DirectoryFact) {
        let now = self.effective_now(now_ms);
        self.upsert(
            now,
            fact.node,
            fact.location,
            Some(fact.hops),
            Some(fact.via),
        );
    }

    /// Records an aoMesh directory entry, decoding its opaque location bytes
    /// with APP-07's payload codec.
    ///
    /// A directory entry whose location bytes are not a location payload is
    /// rejected and not recorded.
    pub fn observe_directory_entry(
        &mut self,
        now_ms: u64,
        entry: &DirectoryEntry,
    ) -> Result<(), MapError> {
        let location = decode_location(entry.location.as_deref())?;
        self.observe_fact(
            now_ms,
            DirectoryFact {
                node: entry.node,
                hops: entry.hops,
                via: entry.via,
                location,
            },
        );
        Ok(())
    }

    /// Advances relative time, applying stale and removal transitions.
    pub fn poll(&mut self, now_ms: u64) -> Vec<MapChange> {
        self.refresh(now_ms);
        self.drain_changes()
    }

    /// Renders items in node-id order after applying time transitions. Change
    /// events remain queued for `poll` or `drain_changes`.
    pub fn snapshot(&mut self, now_ms: u64) -> Vec<MapItem> {
        let now = self.effective_now(now_ms);
        self.refresh_at(now);
        self.items_at(now)
    }

    /// Takes all queued changes without advancing time.
    pub fn drain_changes(&mut self) -> Vec<MapChange> {
        self.changes.drain(..).collect()
    }

    /// Renders neighbour and mesh counts without copying hello bytes, tags,
    /// peer keys or payloads.
    #[must_use]
    pub fn topology(
        &self,
        now_ms: u64,
        neighbors: &NeighborTable,
        directory: &NodeDirectory,
    ) -> TopologySnapshot {
        let now = self.clock_ms.max(now_ms);
        let neighbours = neighbors
            .iter()
            .map(|neighbor| NeighbourView {
                link: neighbor.link.raw(),
                node: neighbor.node_id,
                last_rx_age_ms: neighbor.last_rx_ms.map(|seen| now.saturating_sub(seen)),
                received_packets: neighbor.received_packets,
                malformed_packets: neighbor.malformed_packets,
                rejected_packets: neighbor.rejected_packets,
            })
            .collect();
        TopologySnapshot {
            at_ms: now,
            neighbours,
            mesh_nodes: directory.live_len(now),
            stored_directory_entries: directory.len(),
        }
    }

    fn refresh(&mut self, now_ms: u64) {
        let now = self.effective_now(now_ms);
        self.refresh_at(now);
    }

    fn refresh_at(&mut self, now: u64) {
        let stale_after = self.config.stale_after_ms;
        let remove_after = self.config.remove_after_ms;
        let mut removed = Vec::new();
        let mut stale = Vec::new();
        let mut refreshed = Vec::new();
        for (node, record) in &self.records {
            let age = record.age_at(now);
            if age >= remove_after {
                removed.push(*node);
            } else if age >= stale_after && record.staleness == Staleness::Fresh {
                stale.push(*node);
            } else if age < stale_after && record.staleness == Staleness::Stale {
                refreshed.push(*node);
            }
        }
        for node in stale {
            if let Some(record) = self.records.get_mut(&node) {
                record.staleness = Staleness::Stale;
            }
            self.push_change(now, MapChangeKind::Stale, node);
        }
        for node in refreshed {
            if let Some(record) = self.records.get_mut(&node) {
                record.staleness = Staleness::Fresh;
            }
            self.push_change(now, MapChangeKind::Updated, node);
        }
        for node in removed {
            self.records.remove(&node);
            self.push_change(now, MapChangeKind::Removed, node);
        }
    }

    fn items_at(&self, now: u64) -> Vec<MapItem> {
        self.records
            .iter()
            .map(|(node, record)| MapItem {
                kind: self.kind_for(*node),
                node: *node,
                location: record.location,
                age_ms: record.age_at(now),
                staleness: record.staleness,
                hops: record.hops,
                via: record.via,
            })
            .collect()
    }

    fn upsert(
        &mut self,
        now: u64,
        node: NodeId,
        location: Option<LocationUpdate>,
        hops: Option<u8>,
        via: Option<NodeId>,
    ) {
        let existed = self.records.contains_key(&node);
        let sequence = self.next_sequence();
        match self.records.get_mut(&node) {
            Some(record) => {
                record.location = location.or(record.location);
                record.updated_at_ms = now;
                record.hops = hops.or(record.hops);
                record.via = via.or(record.via);
                record.staleness = Staleness::Fresh;
                record.sequence = sequence;
            }
            None => {
                self.records.insert(
                    node,
                    Record {
                        node,
                        location,
                        updated_at_ms: now,
                        hops,
                        via,
                        staleness: Staleness::Fresh,
                        sequence,
                    },
                );
            }
        }
        self.evict_if_full(now);
        self.push_change(
            now,
            if existed {
                MapChangeKind::Updated
            } else {
                MapChangeKind::Added
            },
            node,
        );
    }

    fn evict_if_full(&mut self, now: u64) {
        while self.records.len() > self.config.max_items {
            let oldest = self
                .records
                .values()
                .min_by_key(|record| (record.updated_at_ms, record.sequence))
                .map(|record| record.node);
            match oldest {
                Some(node) => {
                    self.records.remove(&node);
                    self.push_change(now, MapChangeKind::Removed, node);
                }
                None => break,
            }
        }
    }

    fn kind_for(&self, node: NodeId) -> MapKind {
        if self.self_node == Some(node) {
            MapKind::SelfNode
        } else if self.contacts.contains(&node) {
            MapKind::Contact
        } else {
            MapKind::MeshNode
        }
    }

    fn effective_now(&mut self, now_ms: u64) -> u64 {
        self.clock_ms = self.clock_ms.max(now_ms);
        self.clock_ms
    }

    fn next_sequence(&mut self) -> u64 {
        let sequence = self.sequence;
        self.sequence = self.sequence.saturating_add(1);
        sequence
    }

    fn push_change(&mut self, at_ms: u64, kind: MapChangeKind, node: NodeId) {
        if self.changes.len() == self.config.max_changes {
            self.changes.pop_front();
            self.dropped_changes = self.dropped_changes.saturating_add(1);
        }
        self.changes.push_back(MapChange { at_ms, kind, node });
    }
}

/// Decodes the opaque directory location bytes with the APP-07 codec.
fn decode_location(bytes: Option<&[u8]>) -> Result<Option<LocationUpdate>, MapError> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    match AppPayload::decode(bytes) {
        Ok(AppPayload::Location(location)) => Ok(Some(location)),
        Ok(_) | Err(_) => Err(MapError::InvalidLocationPayload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::LocationPrecision;
    use ao_mesh::{DirectoryConfig, MeshConfig, MeshEngine};
    use rand_core::RngCore;

    #[derive(Debug)]
    struct FixedRng(u64);

    impl RngCore for FixedRng {
        fn next_u32(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            (self.0 >> 32) as u32
        }

        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            self.0
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
            }
        }
    }

    fn node(byte: u8) -> NodeId {
        NodeId::from_bytes([byte; 16])
    }

    fn sample(accuracy: u16, timestamp_s: u32) -> LocationUpdate {
        LocationUpdate::new(
            LocationPrecision::Precise,
            timestamp_s,
            12_345_678,
            -45_000_000,
            accuracy,
            None,
        )
        .expect("fixed sample is valid")
    }

    fn directory_entry(id: NodeId, location: Option<Vec<u8>>) -> DirectoryEntry {
        DirectoryEntry {
            node: id,
            last_seen_ms: 1_000,
            local_seen_ms: 1_000,
            expires_at_ms: 2_000,
            hops: 3,
            via: node(9),
            caps: 0,
            seq: 1,
            card_hash: [0; 8],
            location,
        }
    }

    #[test]
    fn config_rejects_zero_and_inverted_bounds() {
        assert_eq!(MapConfig::new(0, 10, 20, 1), Err(MapError::InvalidConfig));
        assert_eq!(MapConfig::new(1, 20, 20, 1), Err(MapError::InvalidConfig));
        assert_eq!(MapConfig::new(1, 20, 10, 1), Err(MapError::InvalidConfig));
        assert_eq!(MapConfig::new(1, 10, 20, 0), Err(MapError::InvalidConfig));
    }

    #[test]
    fn items_update_immediately_then_become_stale_and_are_removed() {
        let config = MapConfig::new(8, 10, 20, 16).expect("valid config");
        let mut service = MapService::new(config);
        let self_id = node(1);
        let contact = node(2);
        service.set_self(1_000, self_id);
        service
            .set_contacts(1_000, &[contact])
            .expect("one contact fits");
        service.set_location(1_000, contact, sample(25, 1_000));

        let items = service.snapshot(1_000);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, MapKind::SelfNode);
        assert_eq!(items[1].kind, MapKind::Contact);
        assert!(items.iter().all(|item| item.staleness == Staleness::Fresh));
        let changes = service.drain_changes();
        assert!(
            changes
                .iter()
                .any(|change| change.node == contact && change.kind == MapChangeKind::Added)
        );

        service.set_location(1_001, contact, sample(12, 1_001));
        let items = service.snapshot(1_001);
        let updated = items
            .iter()
            .find(|item| item.node == contact)
            .expect("contact item exists");
        assert_eq!(updated.age_ms, 0);
        assert_eq!(updated.location.expect("position").accuracy_m(), Some(12));

        let items = service.snapshot(1_011);
        let stale = items
            .iter()
            .find(|item| item.node == contact)
            .expect("stale item is still shown");
        assert_eq!(stale.staleness, Staleness::Stale);
        let changes = service.poll(1_011);
        assert!(
            changes
                .iter()
                .any(|change| change.node == contact && change.kind == MapChangeKind::Stale)
        );

        let items = service.snapshot(1_021);
        assert!(items.iter().all(|item| item.node != contact));
        let changes = service.poll(1_021);
        assert!(
            changes
                .iter()
                .any(|change| change.node == contact && change.kind == MapChangeKind::Removed)
        );
    }

    #[test]
    fn directory_entries_use_the_app_07_location_codec() {
        let mut service = MapService::new(MapConfig::new(4, 10, 20, 8).expect("valid config"));
        let observed = node(7);
        let expected = sample(25, 1_000);
        let bytes = AppPayload::Location(expected)
            .encode()
            .expect("location encodes");
        service
            .observe_directory_entry(1_000, &directory_entry(observed, Some(bytes)))
            .expect("valid location payload");
        let items = service.snapshot(1_000);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, MapKind::MeshNode);
        assert_eq!(items[0].hops, Some(3));
        assert_eq!(items[0].via, Some(node(9)));
        assert_eq!(items[0].location, Some(expected));

        let invalid = node(8);
        assert_eq!(
            service.observe_directory_entry(1_001, &directory_entry(invalid, Some(vec![1, 2, 3]))),
            Err(MapError::InvalidLocationPayload)
        );
        assert!(
            service
                .snapshot(1_001)
                .iter()
                .all(|item| item.node != invalid)
        );
    }

    #[test]
    fn directory_facts_feed_the_same_stream() {
        let mut service = MapService::new(MapConfig::new(4, 10, 20, 8).expect("valid config"));
        let observed = node(7);
        let via = node(8);
        service.observe_fact(
            1_000,
            DirectoryFact {
                node: observed,
                hops: 3,
                via,
                location: Some(sample(25, 1_000)),
            },
        );
        let items = service.snapshot(1_000);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, MapKind::MeshNode);
        assert_eq!(items[0].hops, Some(3));
        assert_eq!(items[0].via, Some(via));
    }

    #[test]
    fn capacity_evicts_the_oldest_item_deterministically() {
        let mut service = MapService::new(MapConfig::new(2, 10, 20, 16).expect("valid config"));
        service.observe_fact(
            10,
            DirectoryFact {
                node: node(1),
                hops: 1,
                via: node(9),
                location: None,
            },
        );
        service.observe_fact(
            11,
            DirectoryFact {
                node: node(2),
                hops: 1,
                via: node(9),
                location: None,
            },
        );
        service.observe_fact(
            12,
            DirectoryFact {
                node: node(3),
                hops: 1,
                via: node(9),
                location: None,
            },
        );
        let items = service.snapshot(12);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.node != node(1)));
    }

    #[test]
    fn topology_snapshot_exposes_counts_not_hello_bytes() {
        let service = MapService::new(MapConfig::default());
        let config = MeshConfig {
            directory: DirectoryConfig {
                capacity: 4,
                stale_after_ms: 10,
                max_location_bytes: 8,
                max_change_events: 4,
            },
            ..MeshConfig::default()
        };
        let engine = MeshEngine::with_config(FixedRng(7), config);
        let topology = service.topology(42, engine.neighbors(), engine.directory());
        assert_eq!(topology.at_ms, 42);
        assert!(topology.neighbours.is_empty());
        assert_eq!(topology.mesh_nodes, 0);
        assert_eq!(topology.stored_directory_entries, 0);
    }
}
