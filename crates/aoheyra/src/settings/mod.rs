//! User-facing settings and their mapping onto sibling crate configs.
//!
//! The service validates values at the app boundary, before any sibling config
//! is built, and maps one snapshot onto aoNetwork, aoMesh, the mesh store
//! limits and the runtime settings seam. It is sans-IO: no clock, radio,
//! database or task spawn. APP-08's runtime is still a draft, so
//! [`RuntimeSettings`] is a plain data seam with the same bounds and field
//! meanings; the host will pass it to `RuntimeConfig::new` when that PR lands.

use std::fmt;
use std::time::Duration;

use ao_mesh::MeshConfig;
use ao_mesh::store::StoreLimits;
use ao_network::{BeaconFlags, ConfigError as NetworkConfigError, NetworkConfig};

/// Lower bound accepted for the maximum link count, from aoNetwork.
pub const MIN_MAX_LINKS: u32 = 1;
/// Upper bound accepted for the maximum link count, from aoNetwork.
pub const MAX_MAX_LINKS: u32 = 32;
/// Default maximum link count, from aoNetwork.
pub const DEFAULT_MAX_LINKS: u32 = 5;

/// Smallest carried-message quota worth configuring (64 KiB).
pub const MIN_STORE_QUOTA_BYTES: usize = 64 * 1024;
/// Largest carried-message quota on a phone (64 MiB).
pub const MAX_STORE_QUOTA_BYTES: usize = 64 * 1024 * 1024;
/// Default carried-message quota (8 MiB).
pub const DEFAULT_STORE_QUOTA_BYTES: usize = 8 * 1024 * 1024;

const DEFAULT_TTL: u8 = 8;
const DEFAULT_LIFETIME_S: u64 = 3_600;
const DEFAULT_RUNTIME_EVENT_CAPACITY: usize = 64;
const ENTRY_BUDGET_BYTES: usize = 8 * 1024;
const MAX_STORE_ENTRIES: usize = 1024;
const MIN_RUNTIME_EVENT_CAPACITY: usize = 1;
const MAX_RUNTIME_EVENT_CAPACITY: usize = 1024;

/// Battery/performance trade-off chosen by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryProfile {
    /// Longest battery life: slower discovery and keepalives, smaller queue.
    PowerSaver,
    /// aoNetwork and APP-08 defaults.
    Balanced,
    /// Faster discovery at the cost of radio and CPU time.
    Performance,
}

impl BatteryProfile {
    /// Discovery interval applied to aoNetwork.
    #[must_use]
    pub const fn discovery_interval(self) -> Duration {
        match self {
            Self::PowerSaver => Duration::from_secs(30),
            Self::Balanced => Duration::from_secs(10),
            Self::Performance => Duration::from_secs(5),
        }
    }

    /// Keepalive interval applied to aoNetwork.
    #[must_use]
    pub const fn keepalive_interval(self) -> Duration {
        match self {
            Self::PowerSaver => Duration::from_secs(60),
            Self::Balanced => Duration::from_secs(15),
            Self::Performance => Duration::from_secs(10),
        }
    }

    /// Host event-queue capacity applied to APP-08.
    #[must_use]
    pub const fn event_capacity(self) -> usize {
        match self {
            Self::PowerSaver => 16,
            Self::Balanced => DEFAULT_RUNTIME_EVENT_CAPACITY,
            Self::Performance => 128,
        }
    }

    /// Whether the local beacon should advertise low battery.
    #[must_use]
    pub const fn low_battery(self) -> bool {
        matches!(self, Self::PowerSaver)
    }
}

/// Default location precision for new sharing sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationPrecision {
    /// Share no location.
    Off,
    /// Share an approximate area only.
    Coarse,
    /// Share exact coordinates.
    Precise,
}

impl LocationPrecision {
    /// Whether outgoing sessions may contain exact coordinates.
    #[must_use]
    pub const fn allows_precise(self) -> bool {
        matches!(self, Self::Precise)
    }
}

/// Validated user-facing settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppSettings {
    max_links: u32,
    relay_enabled: bool,
    store_quota_bytes: usize,
    battery_profile: BatteryProfile,
    presence_enabled: bool,
    location_precision: LocationPrecision,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            max_links: DEFAULT_MAX_LINKS,
            relay_enabled: true,
            store_quota_bytes: DEFAULT_STORE_QUOTA_BYTES,
            battery_profile: BatteryProfile::Balanced,
            presence_enabled: true,
            location_precision: LocationPrecision::Off,
        }
    }
}

impl AppSettings {
    /// Builds a settings snapshot, rejecting out-of-range values.
    ///
    /// # Errors
    ///
    /// [`SettingsError::MaxLinks`] or [`SettingsError::StoreQuota`] when a
    /// numeric setting is outside its documented range.
    pub fn new(
        max_links: u32,
        relay_enabled: bool,
        store_quota_bytes: usize,
        battery_profile: BatteryProfile,
        presence_enabled: bool,
        location_precision: LocationPrecision,
    ) -> Result<Self, SettingsError> {
        let settings = Self {
            max_links,
            relay_enabled,
            store_quota_bytes,
            battery_profile,
            presence_enabled,
            location_precision,
        };
        settings.validate()?;
        Ok(settings)
    }

    /// Maximum number of simultaneous links.
    #[must_use]
    pub const fn max_links(&self) -> u32 {
        self.max_links
    }

    /// Whether this device advertises willingness to relay.
    #[must_use]
    pub const fn relay_enabled(&self) -> bool {
        self.relay_enabled
    }

    /// Carried-message quota in bytes.
    #[must_use]
    pub const fn store_quota_bytes(&self) -> usize {
        self.store_quota_bytes
    }

    /// Selected battery profile.
    #[must_use]
    pub const fn battery_profile(&self) -> BatteryProfile {
        self.battery_profile
    }

    /// Whether signed presence announcements are exchanged.
    #[must_use]
    pub const fn presence_enabled(&self) -> bool {
        self.presence_enabled
    }

    /// Default precision for location sharing.
    #[must_use]
    pub const fn location_precision(&self) -> LocationPrecision {
        self.location_precision
    }

    fn validate(&self) -> Result<(), SettingsError> {
        if !(MIN_MAX_LINKS..=MAX_MAX_LINKS).contains(&self.max_links) {
            return Err(SettingsError::MaxLinks {
                value: self.max_links,
                min: MIN_MAX_LINKS,
                max: MAX_MAX_LINKS,
            });
        }
        if !(MIN_STORE_QUOTA_BYTES..=MAX_STORE_QUOTA_BYTES).contains(&self.store_quota_bytes) {
            return Err(SettingsError::StoreQuota {
                value: self.store_quota_bytes,
                min: MIN_STORE_QUOTA_BYTES,
                max: MAX_STORE_QUOTA_BYTES,
            });
        }
        Ok(())
    }
}

/// One live settings change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsChange {
    /// Change the link cap.
    MaxLinks(u32),
    /// Change relay willingness.
    RelayEnabled(bool),
    /// Change the carried-message quota.
    StoreQuotaBytes(usize),
    /// Change the battery profile.
    BatteryProfile(BatteryProfile),
    /// Change presence visibility.
    PresenceEnabled(bool),
    /// Change the default location precision.
    LocationPrecision(LocationPrecision),
}

/// The setting that changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKey {
    /// Link cap.
    MaxLinks,
    /// Relay willingness.
    RelayEnabled,
    /// Carried-message quota.
    StoreQuotaBytes,
    /// Battery profile.
    BatteryProfile,
    /// Presence visibility.
    PresenceEnabled,
    /// Location precision default.
    LocationPrecision,
}

/// A typed settings event for the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    /// A setting was validated and applied.
    Changed {
        /// Setting that changed.
        key: SettingKey,
    },
}

/// A rejected setting, reported with the value and its valid range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsError {
    /// Link cap outside 1..=32.
    MaxLinks {
        /// Rejected value.
        value: u32,
        /// Smallest accepted value.
        min: u32,
        /// Largest accepted value.
        max: u32,
    },
    /// Store quota outside the configured byte range.
    StoreQuota {
        /// Rejected value.
        value: usize,
        /// Smallest accepted value.
        min: usize,
        /// Largest accepted value.
        max: usize,
    },
    /// A sibling config rejected the mapped value.
    Network(NetworkConfigError),
    /// The runtime event capacity left the validated range.
    EventCapacity {
        /// Rejected value.
        value: usize,
    },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxLinks { value, min, max } => {
                write!(f, "max links {value} outside {min}..={max}")
            }
            Self::StoreQuota { value, min, max } => {
                write!(f, "store quota {value} bytes outside {min}..={max}")
            }
            Self::Network(error) => write!(f, "network config rejected the mapping: {error}"),
            Self::EventCapacity { value } => {
                write!(f, "runtime event capacity {value} outside 1..=1024")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

/// Config values handed to APP-08 once that runtime is merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeSettings {
    event_capacity: usize,
    ttl: u8,
    lifetime_s: u64,
    relay_willing: bool,
}

impl RuntimeSettings {
    /// Maximum host events held before fan-out overflow.
    #[must_use]
    pub const fn event_capacity(&self) -> usize {
        self.event_capacity
    }

    /// Default Direct hop limit.
    #[must_use]
    pub const fn ttl(&self) -> u8 {
        self.ttl
    }

    /// Default Direct lifetime in seconds.
    #[must_use]
    pub const fn lifetime_s(&self) -> u64 {
        self.lifetime_s
    }

    /// Maps these values onto the APP-08 runtime configuration.
    ///
    /// # Errors
    ///
    /// Returns the runtime's typed config error if a validated snapshot ever
    /// drifts outside the runtime's own bounds.
    pub fn to_runtime_config(&self) -> Result<crate::RuntimeConfig, crate::runtime::ConfigError> {
        crate::RuntimeConfig::new(self.event_capacity, self.ttl, self.lifetime_s)
    }

    /// Whether the runtime should advertise relay willingness.
    #[must_use]
    pub const fn relay_willing(&self) -> bool {
        self.relay_willing
    }
}

/// Privacy gates derived from the user's privacy defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyConfig {
    presence_enabled: bool,
    allow_precise_coordinates: bool,
}

impl PrivacyConfig {
    /// Whether signed presence announcements are exchanged.
    #[must_use]
    pub const fn presence_enabled(&self) -> bool {
        self.presence_enabled
    }

    /// Whether any outgoing session may carry exact coordinates.
    #[must_use]
    pub const fn allow_precise_coordinates(&self) -> bool {
        self.allow_precise_coordinates
    }
}

/// The full mapping of one settings snapshot onto app and sibling configs.
#[derive(Debug)]
pub struct MappedConfig {
    /// Validated aoNetwork config.
    pub network: NetworkConfig,
    /// Validated aoMesh engine config.
    pub mesh: MeshConfig,
    /// Store quotas for aoMesh's store seam.
    pub store: StoreLimits,
    /// Values for APP-08's runtime config seam.
    pub runtime: RuntimeSettings,
    /// Local discovery beacon flags.
    pub beacon: BeaconFlags,
    /// App-level privacy gates.
    pub privacy: PrivacyConfig,
}

/// Holds validated settings and applies live changes atomically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsService {
    settings: AppSettings,
}

impl SettingsService {
    /// Wraps an already validated snapshot.
    #[must_use]
    pub const fn new(settings: AppSettings) -> Self {
        Self { settings }
    }

    /// Current validated settings.
    #[must_use]
    pub const fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// Validates and applies one change.
    ///
    /// The stored snapshot is untouched when validation fails.
    ///
    /// # Errors
    ///
    /// [`SettingsError::MaxLinks`] or [`SettingsError::StoreQuota`] with the
    /// rejected value and its range.
    pub fn update(&mut self, change: SettingsChange) -> Result<Vec<SettingsEvent>, SettingsError> {
        let mut candidate = self.settings;
        let key = match change {
            SettingsChange::MaxLinks(value) => {
                candidate.max_links = value;
                SettingKey::MaxLinks
            }
            SettingsChange::RelayEnabled(value) => {
                candidate.relay_enabled = value;
                SettingKey::RelayEnabled
            }
            SettingsChange::StoreQuotaBytes(value) => {
                candidate.store_quota_bytes = value;
                SettingKey::StoreQuotaBytes
            }
            SettingsChange::BatteryProfile(value) => {
                candidate.battery_profile = value;
                SettingKey::BatteryProfile
            }
            SettingsChange::PresenceEnabled(value) => {
                candidate.presence_enabled = value;
                SettingKey::PresenceEnabled
            }
            SettingsChange::LocationPrecision(value) => {
                candidate.location_precision = value;
                SettingKey::LocationPrecision
            }
        };
        candidate.validate()?;
        self.settings = candidate;
        Ok(vec![SettingsEvent::Changed { key }])
    }

    /// Maps the snapshot onto all downstream configs.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Network`] if aoNetwork rejects a mapped value and
    /// [`SettingsError::EventCapacity`] if the battery profile leaves the
    /// runtime's validated range.
    pub fn map(&self) -> Result<MappedConfig, SettingsError> {
        let profile = self.settings.battery_profile;
        let event_capacity = profile.event_capacity();
        if !(MIN_RUNTIME_EVENT_CAPACITY..=MAX_RUNTIME_EVENT_CAPACITY).contains(&event_capacity) {
            return Err(SettingsError::EventCapacity {
                value: event_capacity,
            });
        }

        let network = NetworkConfig::default()
            .with_max_links(self.settings.max_links)
            .map_err(SettingsError::Network)?
            .with_discovery_interval(profile.discovery_interval())
            .map_err(SettingsError::Network)?
            .with_keepalive_interval(profile.keepalive_interval())
            .map_err(SettingsError::Network)?;

        let mut mesh = MeshConfig::default();
        mesh.presence.enabled = self.settings.presence_enabled;

        let quota = self.settings.store_quota_bytes;
        let store = StoreLimits {
            entries: (quota / ENTRY_BUDGET_BYTES).clamp(1, MAX_STORE_ENTRIES),
            total_bytes: quota,
            per_origin_bytes: quota / 8,
            per_class_bytes: [quota, quota / 4, quota / 32, 0],
        };

        Ok(MappedConfig {
            network,
            mesh,
            store,
            runtime: RuntimeSettings {
                event_capacity,
                ttl: DEFAULT_TTL,
                lifetime_s: DEFAULT_LIFETIME_S,
                relay_willing: self.settings.relay_enabled,
            },
            beacon: BeaconFlags {
                accepting: true,
                relay_willing: self.settings.relay_enabled,
                low_battery: profile.low_battery(),
            },
            privacy: PrivacyConfig {
                presence_enabled: self.settings.presence_enabled,
                allow_precise_coordinates: self.settings.location_precision.allows_precise(),
            },
        })
    }
}
