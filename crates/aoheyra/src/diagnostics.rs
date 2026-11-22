//! Privacy-preserving diagnostics bundle (APP-15).
//!
//! A bundle is a deterministic, versioned byte archive of allowlisted numeric
//! state: redacted structured log events, a link table without peer keys,
//! network counters, mesh counters and device class. It is built from the
//! real observability snapshots published by aoNetwork (NET-23) and aoMesh
//! (MSH-30).
//!
//! Nothing here reads the system clock, a file, a log sink or a message body.
//! `now_ms` is injected, archive bytes are returned to the caller, and every
//! field is copied from a closed enum or a fixed-width integer. There is no
//! free-form string path from a packet, a neighbour or a log record into the
//! archive, so message text, keys and coordinates cannot leak by construction.

use ao_mesh::MESH_SNAPSHOT_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::time::Duration;

/// Magic prefix of a diagnostics archive.
pub const ARCHIVE_MAGIC: &[u8; 8] = b"AOHDIAG1";
/// Archive framing version. Bump when the framing or JSON schema changes.
pub const ARCHIVE_VERSION: u8 = 1;
/// Hard cap on one archive. The bounded tables keep generated bundles well
/// below it; it also bounds work when a reviewer opens an untrusted file.
pub const MAX_ARCHIVE_LEN: usize = 1024 * 1024;
/// Default maximum number of structured log events kept in memory.
pub const DEFAULT_MAX_EVENTS: usize = 256;
/// Default maximum number of link rows in a bundle.
pub const DEFAULT_MAX_LINKS: usize = 64;

const FRAMING_LEN: usize = ARCHIVE_MAGIC.len() + 1 + 8;

/// Typed diagnostics failures.
#[derive(Debug)]
pub enum DiagnosticsError {
    /// A supplied bound was zero or above its hard cap.
    InvalidConfig,
    /// The serialized payload exceeded [`MAX_ARCHIVE_LEN`].
    ArchiveTooLarge {
        /// Actual payload length.
        len: usize,
        /// Hard cap.
        max: usize,
    },
    /// The archive did not start with [`ARCHIVE_MAGIC`].
    BadMagic,
    /// The archive used an unknown framing version.
    UnsupportedArchiveVersion(u8),
    /// The archive ended before a required field.
    Truncated {
        /// Minimum bytes needed.
        need: usize,
        /// Bytes present.
        have: usize,
    },
    /// The declared payload length was invalid.
    InvalidLength,
    /// JSON encoding or decoding failed.
    Json(serde_json::Error),
}

impl fmt::Display for DiagnosticsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid diagnostics config"),
            Self::ArchiveTooLarge { len, max } => {
                write!(f, "diagnostics payload {len} exceeds cap {max}")
            }
            Self::BadMagic => write!(f, "not an aoheyra diagnostics archive"),
            Self::UnsupportedArchiveVersion(version) => {
                write!(f, "unsupported diagnostics archive version {version}")
            }
            Self::Truncated { need, have } => {
                write!(f, "diagnostics archive needs {need} bytes, has {have}")
            }
            Self::InvalidLength => write!(f, "invalid diagnostics payload length"),
            Self::Json(error) => write!(f, "diagnostics JSON: {error}"),
        }
    }
}

impl Error for DiagnosticsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

/// Bounded sizes for a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsConfig {
    /// Maximum number of structured events retained for a bundle.
    pub max_events: usize,
    /// Maximum number of link rows emitted for a bundle.
    pub max_links: usize,
}

impl DiagnosticsConfig {
    /// Validates both bounds without silent clamping.
    pub fn new(max_events: usize, max_links: usize) -> Result<Self, DiagnosticsError> {
        if max_events == 0 || max_links == 0 || max_links > usize::from(u16::MAX) {
            return Err(DiagnosticsError::InvalidConfig);
        }
        Ok(Self {
            max_events,
            max_links,
        })
    }
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            max_events: DEFAULT_MAX_EVENTS,
            max_links: DEFAULT_MAX_LINKS,
        }
    }
}

/// Closed set of log event kinds. No event carries text or bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventCode {
    /// A link reached `Ready`.
    LinkUp,
    /// A link ended, for any reason.
    LinkDown,
    /// A packet or frame was dropped by policy.
    PacketDropped,
    /// A packet was delivered locally.
    PacketDelivered,
    /// A mesh sync exchange completed.
    SyncCompleted,
    /// A mesh sync exchange failed validation.
    SyncFailed,
    /// A queue or cache reached its configured bound.
    Backpressure,
    /// A neighbour penalty was applied.
    PenaltyApplied,
}

/// One redacted structured log event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEvent {
    /// Allowlisted event kind.
    pub code: EventCode,
    /// Injected local time in milliseconds.
    pub at_ms: u64,
    /// Number of times this event occurred at this time.
    pub count: u64,
    /// Ordinal in the bundle's link table, never a peer key.
    pub link_index: Option<u16>,
}

/// A bounded ring of structured log events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticLog {
    events: VecDeque<LogEvent>,
    max_events: usize,
}

impl DiagnosticLog {
    /// Creates an empty log with a validated bound.
    pub fn new(max_events: usize) -> Result<Self, DiagnosticsError> {
        if max_events == 0 {
            return Err(DiagnosticsError::InvalidConfig);
        }
        Ok(Self {
            events: VecDeque::with_capacity(max_events.min(DEFAULT_MAX_EVENTS)),
            max_events,
        })
    }

    /// Records one event, dropping the oldest entry when full.
    pub fn record(&mut self, code: EventCode, at_ms: u64, count: u64, link_index: Option<u16>) {
        if self.events.len() == self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(LogEvent {
            code,
            at_ms,
            count,
            link_index,
        });
    }

    /// Recorded events in arrival order.
    pub fn events(&self) -> impl Iterator<Item = &LogEvent> {
        self.events.iter()
    }
}

impl Default for DiagnosticLog {
    fn default() -> Self {
        Self {
            events: VecDeque::with_capacity(DEFAULT_MAX_EVENTS),
            max_events: DEFAULT_MAX_EVENTS,
        }
    }
}

/// Device class only: no identifier, model, serial or advertising name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformCode {
    /// The Android app.
    Android,
}

/// CPU class only, for interpreting battery and latency numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchitectureCode {
    /// 64-bit ARM.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// 32-bit ARM.
    #[serde(rename = "armv7")]
    Armv7,
    /// 64-bit x86, used by the emulator.
    #[serde(rename = "x86_64")]
    X8664,
    /// A class this bundle version does not name.
    #[serde(rename = "other")]
    Other,
}

/// Allowlisted device information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Crate version, from `Cargo.toml` rather than the device.
    pub app_version: String,
    /// Platform class.
    pub platform: PlatformCode,
    /// Android SDK level supplied by the caller, if known.
    pub sdk_version: Option<u32>,
    /// CPU class.
    pub architecture: ArchitectureCode,
}

impl DeviceInfo {
    /// Builds Android device information with no free-form fields.
    #[must_use]
    pub fn android(sdk_version: Option<u32>, architecture: ArchitectureCode) -> Self {
        Self {
            app_version: crate::VERSION.to_owned(),
            platform: PlatformCode::Android,
            sdk_version,
            architecture,
        }
    }
}

/// Discovery mode code from aoNetwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryCode {
    /// Below the stable target degree.
    Aggressive,
    /// At or above the stable target degree.
    Normal,
    /// Low power or idle.
    Quiet,
}

/// Link role code; the peer's `PeerKey` is deliberately not carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkRoleCode {
    /// This node opened the link.
    Initiator,
    /// This node accepted the link.
    Responder,
}

/// Link class code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkClassCode {
    /// Inside the stable target.
    Stable,
    /// The exploration slot.
    Explore,
    /// A temporary opportunistic slot.
    Opportunistic,
    /// A class added after this bundle schema.
    #[serde(rename = "other")]
    Other,
}

/// Link lifecycle code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStateCode {
    /// Waiting for the transport to connect.
    Connecting,
    /// Negotiating MTU.
    Negotiating,
    /// Waiting for a valid peer hello.
    Handshaking,
    /// Data may flow.
    Ready,
    /// Waiting for transport confirmation of close.
    Closing,
    /// Ended.
    Closed,
}

/// One link row: numeric counters and closed codes only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRow {
    /// Ordinal in the snapshot's deterministic link order.
    pub index: u16,
    /// Which side opened the link.
    pub role: LinkRoleCode,
    /// Slot class.
    pub class: LinkClassCode,
    /// Lifecycle state.
    pub state: LinkStateCode,
    /// Injected time the handshake finished, in milliseconds.
    pub up_ms: u64,
    /// Moving-average RSSI in dBm, if the radio reported one.
    pub rssi_dbm: Option<i16>,
    /// Moving-average round trip in microseconds, if known.
    pub rtt_us: Option<u64>,
    /// Bytes written.
    pub tx_bytes: u64,
    /// Bytes received.
    pub rx_bytes: u64,
    /// Received frames discarded without closing the link.
    pub dropped_frames: u64,
    /// Bytes waiting in the send queue.
    pub queued_bytes: u64,
    /// Queue frames dropped past expiry.
    pub queue_expired: u64,
    /// Queue frames evicted under pressure.
    pub queue_evicted: u64,
    /// Queue frames replaced by a newer coalesced value.
    pub queue_coalesced: u64,
}

/// Cumulative aoNetwork counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkCounters {
    /// Links that finished their handshake.
    pub links_up: u64,
    /// Links closed normally by this side.
    pub links_down_closed: u64,
    /// Links closed by a peer `Bye`.
    pub links_down_bye: u64,
    /// Links lost to a failed handshake.
    pub links_down_handshake_failed: u64,
    /// Links lost to keepalive timeout.
    pub links_down_keepalive_timeout: u64,
    /// Links closed for protocol violations.
    pub links_down_protocol_violation: u64,
    /// Links closed for abuse.
    pub links_down_abuse_detected: u64,
    /// Links closed by the degree cap.
    pub links_down_degree_cap: u64,
    /// Links closed as duplicates.
    pub links_down_duplicate: u64,
    /// Links lost with the transport.
    pub links_down_transport_lost: u64,
    /// Connection attempts that never connected.
    pub connects_failed: u64,
    /// Queue frames dropped past expiry.
    pub queue_drops_expired: u64,
    /// Queue frames evicted under pressure.
    pub queue_drops_evicted: u64,
    /// Queue frames coalesced in place.
    pub queue_drops_coalesced: u64,
    /// Sends refused because a queue was full.
    pub queue_full: u64,
    /// Strikes recorded against peers.
    pub strikes: u64,
    /// Peers blacklisted.
    pub blacklisted: u64,
}

/// Redacted view of `ao_network::Snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkReport {
    /// Injected time the snapshot was taken, in milliseconds.
    pub at_ms: u64,
    /// Discovery mode code.
    pub mode: DiscoveryCode,
    /// Link rows emitted.
    pub link_count: u16,
    /// Links omitted because they exceeded the configured cap.
    pub links_omitted: u32,
    /// Bounded link table.
    pub links: Vec<LinkRow>,
    /// Known radio peers, without their keys.
    pub peers: u32,
    /// Advisory link hints held for peers.
    pub hints: u32,
    /// Peers blocked at snapshot time.
    pub blocked: u32,
    /// Cumulative link counters.
    pub counters: NetworkCounters,
}

/// Redacted view of `ao_mesh::MeshSnapshot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshCounters {
    /// Schema version reported by aoMesh.
    pub schema_version: u8,
    /// Whether this bundle version understands that schema.
    pub recognized: bool,
    /// Duplicate verdicts from the forwarding dedupe cache.
    pub duplicate_packets: u64,
    /// Flood relays cancelled by suppression.
    pub suppressed_relays: u64,
    /// Store ids known to the mesh.
    pub store_entries: u32,
    /// Encoded sync bytes sent.
    pub sync_bytes_sent: u64,
    /// Encoded sync bytes accepted.
    pub sync_bytes_received: u64,
    /// Neighbour penalty applications.
    pub neighbour_penalty_events: u64,
    /// Sum of current neighbour penalty scores.
    pub neighbour_penalty_total: u64,
    /// Largest current neighbour penalty score.
    pub neighbour_penalty_max: u32,
    /// Neighbours with a non-zero score.
    pub penalised_neighbours: u32,
    /// Current seen-cache recordings.
    pub seen_records: u32,
    /// Current pending suppression windows.
    pub suppression_pending: u32,
}

/// The complete archive content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsBundle {
    /// JSON schema version, equal to [`ARCHIVE_VERSION`].
    pub schema_version: u8,
    /// Injected local generation time in milliseconds.
    pub generated_at_ms: u64,
    /// Allowlisted device class.
    pub device: DeviceInfo,
    /// Redacted network snapshot.
    pub network: NetworkReport,
    /// Redacted mesh counters.
    pub mesh: MeshCounters,
    /// Redacted structured events in arrival order.
    pub events: Vec<LogEvent>,
}

impl DiagnosticsBundle {
    /// Builds a bundle from sibling snapshots and a redacted event log.
    #[must_use]
    pub fn new(
        config: &DiagnosticsConfig,
        generated_at_ms: u64,
        device: DeviceInfo,
        network: &ao_network::Snapshot,
        mesh: &ao_mesh::MeshSnapshot,
        log: &DiagnosticLog,
    ) -> Self {
        Self {
            schema_version: ARCHIVE_VERSION,
            generated_at_ms,
            device,
            network: NetworkReport::from_snapshot(config.max_links, network),
            mesh: MeshCounters::from_snapshot(mesh),
            events: log.events().cloned().collect(),
        }
    }

    /// Serializes the bundle as deterministic pretty JSON.
    pub fn to_json(&self) -> Result<String, DiagnosticsError> {
        serde_json::to_string_pretty(self).map_err(DiagnosticsError::Json)
    }

    /// Frames the JSON as a deterministic archive byte stream.
    pub fn to_archive(&self) -> Result<Vec<u8>, DiagnosticsError> {
        let payload = serde_json::to_vec(self).map_err(DiagnosticsError::Json)?;
        if payload.len() > MAX_ARCHIVE_LEN {
            return Err(DiagnosticsError::ArchiveTooLarge {
                len: payload.len(),
                max: MAX_ARCHIVE_LEN,
            });
        }
        let mut out = Vec::with_capacity(FRAMING_LEN + payload.len());
        out.extend_from_slice(ARCHIVE_MAGIC);
        out.push(ARCHIVE_VERSION);
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    /// Parses a framed archive. This validates framing only; callers must
    /// still treat an imported file as untrusted input.
    pub fn from_archive(bytes: &[u8]) -> Result<Self, DiagnosticsError> {
        if bytes.len() < ARCHIVE_MAGIC.len() {
            return Err(DiagnosticsError::Truncated {
                need: ARCHIVE_MAGIC.len(),
                have: bytes.len(),
            });
        }
        if &bytes[..ARCHIVE_MAGIC.len()] != ARCHIVE_MAGIC {
            return Err(DiagnosticsError::BadMagic);
        }
        if bytes.len() < ARCHIVE_MAGIC.len() + 1 {
            return Err(DiagnosticsError::Truncated {
                need: ARCHIVE_MAGIC.len() + 1,
                have: bytes.len(),
            });
        }
        let version = bytes[ARCHIVE_MAGIC.len()];
        if version != ARCHIVE_VERSION {
            return Err(DiagnosticsError::UnsupportedArchiveVersion(version));
        }
        if bytes.len() < FRAMING_LEN {
            return Err(DiagnosticsError::Truncated {
                need: FRAMING_LEN,
                have: bytes.len(),
            });
        }
        let len_start = ARCHIVE_MAGIC.len() + 1;
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&bytes[len_start..FRAMING_LEN]);
        let payload_len = u64::from_le_bytes(len_bytes);
        if payload_len > MAX_ARCHIVE_LEN as u64 {
            return Err(DiagnosticsError::InvalidLength);
        }
        let payload_len =
            usize::try_from(payload_len).map_err(|_| DiagnosticsError::InvalidLength)?;
        let end = FRAMING_LEN
            .checked_add(payload_len)
            .ok_or(DiagnosticsError::InvalidLength)?;
        if bytes.len() < end {
            return Err(DiagnosticsError::Truncated {
                need: end,
                have: bytes.len(),
            });
        }
        serde_json::from_slice(&bytes[FRAMING_LEN..end]).map_err(DiagnosticsError::Json)
    }
}

impl NetworkReport {
    /// Copies only allowlisted fields from a real aoNetwork snapshot.
    #[must_use]
    pub fn from_snapshot(max_links: usize, snapshot: &ao_network::Snapshot) -> Self {
        let total = snapshot.links.len();
        let mut links = Vec::with_capacity(total.min(max_links));
        for (ordinal, link) in snapshot.links.iter().take(max_links).enumerate() {
            let metrics = &link.metrics;
            let queue = &metrics.queue_drops;
            links.push(LinkRow {
                index: u16::try_from(ordinal).unwrap_or(u16::MAX),
                role: match link.role {
                    ao_network::LinkRole::Initiator => LinkRoleCode::Initiator,
                    ao_network::LinkRole::Responder => LinkRoleCode::Responder,
                },
                class: match link.class {
                    ao_network::LinkClass::Stable => LinkClassCode::Stable,
                    ao_network::LinkClass::Explore => LinkClassCode::Explore,
                    ao_network::LinkClass::Opportunistic => LinkClassCode::Opportunistic,
                    _ => LinkClassCode::Other,
                },
                state: match link.state {
                    ao_network::LinkState::Connecting => LinkStateCode::Connecting,
                    ao_network::LinkState::Negotiating => LinkStateCode::Negotiating,
                    ao_network::LinkState::Handshaking => LinkStateCode::Handshaking,
                    ao_network::LinkState::Ready => LinkStateCode::Ready,
                    ao_network::LinkState::Closing => LinkStateCode::Closing,
                    ao_network::LinkState::Closed => LinkStateCode::Closed,
                },
                up_ms: duration_ms(link.up_at),
                rssi_dbm: metrics.rssi,
                rtt_us: metrics.rtt.map(duration_micros),
                tx_bytes: metrics.tx_bytes,
                rx_bytes: metrics.rx_bytes,
                dropped_frames: metrics.dropped_frames,
                queued_bytes: saturating_u64(link.queued_bytes),
                queue_expired: queue.expired,
                queue_evicted: queue.evicted,
                queue_coalesced: queue.coalesced,
            });
        }
        let counters = &snapshot.counters;
        let down = &counters.links_down;
        Self {
            at_ms: duration_ms(snapshot.at),
            mode: match snapshot.mode {
                ao_network::DiscoveryMode::Aggressive => DiscoveryCode::Aggressive,
                ao_network::DiscoveryMode::Normal => DiscoveryCode::Normal,
                ao_network::DiscoveryMode::Quiet => DiscoveryCode::Quiet,
            },
            link_count: u16::try_from(links.len()).unwrap_or(u16::MAX),
            links_omitted: saturating_u32(total.saturating_sub(links.len())),
            links,
            peers: saturating_u32(snapshot.peers),
            hints: saturating_u32(snapshot.hints),
            blocked: saturating_u32(snapshot.blocked),
            counters: NetworkCounters {
                links_up: counters.links_up,
                links_down_closed: down.closed,
                links_down_bye: down.bye,
                links_down_handshake_failed: down.handshake_failed,
                links_down_keepalive_timeout: down.keepalive_timeout,
                links_down_protocol_violation: down.protocol_violation,
                links_down_abuse_detected: down.abuse_detected,
                links_down_degree_cap: down.degree_cap,
                links_down_duplicate: down.duplicate,
                links_down_transport_lost: down.transport_lost,
                connects_failed: counters.connects_failed,
                queue_drops_expired: counters.queue_drops.expired,
                queue_drops_evicted: counters.queue_drops.evicted,
                queue_drops_coalesced: counters.queue_drops.coalesced,
                queue_full: counters.queue_full,
                strikes: counters.strikes,
                blacklisted: counters.blacklisted,
            },
        }
    }
}

impl MeshCounters {
    /// Copies a mesh snapshot, refusing to guess an unfamiliar schema.
    #[must_use]
    pub fn from_snapshot(snapshot: &ao_mesh::MeshSnapshot) -> Self {
        if snapshot.version != MESH_SNAPSHOT_VERSION {
            return Self {
                schema_version: snapshot.version,
                recognized: false,
                duplicate_packets: 0,
                suppressed_relays: 0,
                store_entries: 0,
                sync_bytes_sent: 0,
                sync_bytes_received: 0,
                neighbour_penalty_events: 0,
                neighbour_penalty_total: 0,
                neighbour_penalty_max: 0,
                penalised_neighbours: 0,
                seen_records: 0,
                suppression_pending: 0,
            };
        }
        Self {
            schema_version: snapshot.version,
            recognized: true,
            duplicate_packets: snapshot.duplicate_packets,
            suppressed_relays: snapshot.suppressed_relays,
            store_entries: snapshot.store_entries,
            sync_bytes_sent: snapshot.sync_bytes_sent,
            sync_bytes_received: snapshot.sync_bytes_received,
            neighbour_penalty_events: snapshot.neighbour_penalty_events,
            neighbour_penalty_total: snapshot.neighbour_penalty_total,
            neighbour_penalty_max: snapshot.neighbour_penalty_max,
            penalised_neighbours: snapshot.penalised_neighbours,
            seen_records: snapshot.seen_records,
            suppression_pending: snapshot.suppression_pending,
        }
    }
}

fn duration_ms(value: Duration) -> u64 {
    u64::try_from(value.as_millis()).unwrap_or(u64::MAX)
}

fn duration_micros(value: Duration) -> u64 {
    u64::try_from(value.as_micros()).unwrap_or(u64::MAX)
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ao_mesh::MeshEngine;
    use ao_network::{
        AppHello, BeaconFlags, LocalTag, Network, NetworkConfig, SimConfig, SimNode, SimWorld,
    };
    use rand_core::RngCore;
    use std::task::{Context, Poll, Waker};

    const SECRET_MARKER: &str = "TOP-SECRET-MESSAGE-MARKER-7f3a2c";

    #[derive(Debug)]
    struct FixedRng(u64);

    impl RngCore for FixedRng {
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
    }

    fn config() -> DiagnosticsConfig {
        DiagnosticsConfig::new(8, 4).expect("valid config")
    }

    fn mesh_snapshot() -> ao_mesh::MeshSnapshot {
        MeshEngine::new(FixedRng(7)).observability_snapshot()
    }

    fn linked_network_snapshot() -> Result<ao_network::Snapshot, Box<dyn Error>> {
        let world = SimWorld::new(SimConfig {
            seed: 7,
            ..SimConfig::default()
        });
        let radio_a = world.add_node(SimNode::default())?;
        let radio_b = world.add_node(SimNode::default())?;
        let mut a = Network::new(NetworkConfig::default(), radio_a);
        let mut b = Network::new(NetworkConfig::default(), radio_b);
        let hello = AppHello::new(SECRET_MARKER.as_bytes().to_vec())?;
        let accepting = BeaconFlags {
            accepting: true,
            ..BeaconFlags::default()
        };
        a.set_local_hello(LocalTag::new([1; 16]), hello.clone());
        b.set_local_hello(LocalTag::new([2; 16]), hello);
        a.set_beacon([1; 8], [0; 4], accepting);
        b.set_beacon([2; 8], [0; 4], accepting);
        world.set_in_range(a.transport().node(), b.transport().node(), true);

        let mut context = Context::from_waker(Waker::noop());
        while world.now() < Duration::from_secs(3) {
            world.advance(Duration::from_millis(10));
            for node in [&mut a, &mut b] {
                while let Poll::Ready(_event) = node.poll_event(&mut context, world.now()) {}
            }
            if a.links().len() == 1 && b.links().len() == 1 {
                break;
            }
        }
        Ok(a.snapshot(world.now()))
    }

    fn peer_hex(snapshot: &ao_network::Snapshot, index: usize) -> String {
        let link = &snapshot.links[index];
        let bytes = link.peer.as_bytes();
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(out, "{byte:02x}").expect("writing to a String cannot fail");
        }
        out
    }

    #[test]
    fn config_and_log_bounds_reject_zero() {
        assert!(matches!(
            DiagnosticsConfig::new(0, 1),
            Err(DiagnosticsError::InvalidConfig)
        ));
        assert!(matches!(
            DiagnosticsConfig::new(1, 0),
            Err(DiagnosticsError::InvalidConfig)
        ));
        assert!(matches!(
            DiagnosticLog::new(0),
            Err(DiagnosticsError::InvalidConfig)
        ));
    }

    #[test]
    fn log_is_bounded_and_keeps_arrival_order() {
        let mut log = DiagnosticLog::new(2).expect("valid bound");
        log.record(EventCode::LinkUp, 1, 1, Some(0));
        log.record(EventCode::PacketDropped, 2, 3, None);
        log.record(EventCode::SyncCompleted, 3, 1, None);
        let codes: Vec<EventCode> = log.events().map(|event| event.code).collect();
        assert_eq!(
            codes,
            vec![EventCode::PacketDropped, EventCode::SyncCompleted]
        );
        assert_eq!(log.events().next().map(|event| event.count), Some(3));
    }

    #[test]
    fn mesh_schema_is_not_guessed_when_unknown() {
        let mut snapshot = mesh_snapshot();
        assert!(MeshCounters::from_snapshot(&snapshot).recognized);
        snapshot.version = mesh_snapshot().version.wrapping_add(1);
        let counters = MeshCounters::from_snapshot(&snapshot);
        assert!(!counters.recognized);
        assert_eq!(counters.schema_version, snapshot.version);
        assert_eq!(counters.duplicate_packets, 0);
    }

    #[test]
    fn bundle_is_deterministic_and_round_trips() -> Result<(), Box<dyn Error>> {
        let network = linked_network_snapshot()?;
        let config = config();
        let mut log = DiagnosticLog::new(config.max_events)?;
        log.record(EventCode::LinkUp, 42, 1, Some(0));
        log.record(EventCode::PacketDelivered, 43, 2, None);
        let bundle = DiagnosticsBundle::new(
            &config,
            44,
            DeviceInfo::android(Some(34), ArchitectureCode::Aarch64),
            &network,
            &mesh_snapshot(),
            &log,
        );
        let archive = bundle.to_archive()?;
        assert_eq!(archive, bundle.to_archive()?);
        assert_eq!(DiagnosticsBundle::from_archive(&archive)?, bundle);
        assert!(bundle.to_json()?.contains("\"schema_version\": 1"));
        Ok(())
    }

    #[test]
    fn sample_bundle_has_no_message_text_or_peer_key() -> Result<(), Box<dyn Error>> {
        let network = linked_network_snapshot()?;
        assert_eq!(network.links.len(), 1, "the privacy test needs a real link");
        let key = peer_hex(&network, 0);
        let config = DiagnosticsConfig::new(8, 4)?;
        let mut log = DiagnosticLog::new(config.max_events)?;
        log.record(EventCode::LinkUp, 1, 1, Some(0));
        let bundle = DiagnosticsBundle::new(
            &config,
            1,
            DeviceInfo::android(Some(34), ArchitectureCode::Aarch64),
            &network,
            &mesh_snapshot(),
            &log,
        );
        let archive = bundle.to_archive()?;
        let text = String::from_utf8_lossy(&archive);
        assert!(!text.contains(SECRET_MARKER), "message text leaked");
        assert!(!text.contains(&key), "peer key leaked");
        assert!(!text.contains("\"peer\""), "peer field leaked");
        assert!(!text.contains("peer_key"), "peer key field leaked");
        assert!(!text.contains("link_id"), "raw link id leaked");
        Ok(())
    }

    #[test]
    fn link_table_cap_is_explicit() -> Result<(), Box<dyn Error>> {
        let network = linked_network_snapshot()?;
        let config = DiagnosticsConfig::new(8, 1)?;
        let bundle = DiagnosticsBundle::new(
            &config,
            1,
            DeviceInfo::android(None, ArchitectureCode::Other),
            &network,
            &mesh_snapshot(),
            &DiagnosticLog::default(),
        );
        assert_eq!(bundle.network.link_count, 1);
        assert_eq!(bundle.network.links.len(), 1);
        assert_eq!(bundle.network.links_omitted, 0);
        Ok(())
    }

    #[test]
    fn archive_rejects_bad_framing() -> Result<(), Box<dyn Error>> {
        let config = config();
        let bundle = DiagnosticsBundle::new(
            &config,
            1,
            DeviceInfo::android(Some(34), ArchitectureCode::Aarch64),
            &linked_network_snapshot()?,
            &mesh_snapshot(),
            &DiagnosticLog::default(),
        );
        let archive = bundle.to_archive()?;
        assert!(matches!(
            DiagnosticsBundle::from_archive(&archive[..4]),
            Err(DiagnosticsError::Truncated { need: 8, have: 4 })
        ));
        let mut bad_magic = archive.clone();
        bad_magic[0] = 0;
        assert_eq!(
            DiagnosticsBundle::from_archive(&bad_magic)
                .err()
                .map(|error| error.to_string()),
            Some(DiagnosticsError::BadMagic.to_string())
        );
        let mut bad_version = archive.clone();
        bad_version[8] = 2;
        assert_eq!(
            DiagnosticsBundle::from_archive(&bad_version)
                .err()
                .map(|error| error.to_string()),
            Some(DiagnosticsError::UnsupportedArchiveVersion(2).to_string())
        );
        let json_error = serde_json::from_str::<u8>("x").expect_err("invalid json");
        assert!(DiagnosticsError::Json(json_error).source().is_some());
        Ok(())
    }
}
