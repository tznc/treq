//! Provider-neutral managed compute types.
//!
//! The normalized lifecycle states of a Treq-managed VM, VM size presets and
//! region records. Provider adapters live in the Edge Functions
//! (`supabase/functions/_shared/remote/provider.ts`); nothing here talks to a
//! vendor API. UI code must depend only on these types, never on vendor
//! status strings.

use serde::{Deserialize, Serialize};

/// Normalized lifecycle state for a managed instance. Provider-specific status
/// strings are mapped into this enum by the adapter; nothing outside the
/// adapter should see a raw vendor status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedInstanceState {
  /// No instance has been provisioned for this user yet.
  Unprovisioned,
  /// A create request has been accepted by the provider and is in progress.
  Provisioning,
  /// The instance exists and is installing the boot manifest.
  Bootstrapping,
  /// Access material (host key trust, certificate authority trust) is being
  /// installed on the instance.
  InstallingAccess,
  /// Expanded readiness checks are running.
  Verifying,
  /// The instance passed readiness and is available for use.
  Ready,
  /// The provider reports the instance suspended (typically vendor-driven
  /// idleness). Treq records this; it does not run a second idle timer.
  Suspended,
  /// A wake request has been issued and Treq is waiting for the instance to
  /// become reachable again.
  Waking,
  /// The instance is being replaced (new size, region, or manifest). The
  /// generation is incremented when this transition begins.
  Reprovisioning,
  /// The instance is reachable but failed a readiness or reconciliation
  /// check and requires attention.
  Degraded,
  /// The instance failed to provision, bootstrap, or reprovision.
  Failed,
  /// A delete request has been accepted and teardown is in progress.
  Deleting,
  /// The instance and its provider-side resources have been removed.
  Deleted,
}

impl ManagedInstanceState {
  /// True when the instance is expected to serve SSH connections.
  pub fn is_connectable(self) -> bool {
    matches!(self, Self::Ready)
  }

  /// True when the state is terminal and no further reconciliation applies
  /// without a new user-initiated operation.
  pub fn is_terminal(self) -> bool {
    matches!(self, Self::Deleted | Self::Failed)
  }
}

/// A small, Treq-defined set of VM size presets. Presets map to
/// provider-specific CPU, memory, and storage settings inside the adapter;
/// callers outside the adapter select only from this closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizePreset {
  Small,
  Medium,
  Large,
}

/// Static metadata describing a [`SizePreset`] for display purposes. Values
/// here are Treq-facing approximations, not a live provider quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SizePresetInfo {
  pub preset: SizePreset,
  pub label: &'static str,
  pub vcpus: u32,
  pub memory_gb: u32,
  pub storage_gb: u32,
}

/// The full, ordered list of size presets offered to users. Order is the
/// display order (smallest to largest).
///
/// Every size is pay-as-you-go; there is no free base allocation. The
/// priced tiers users pick from come from the server catalog
/// (`core::cloud_catalog`).
pub const SIZE_PRESETS: &[SizePresetInfo] = &[
  SizePresetInfo {
    preset: SizePreset::Small,
    label: "Small",
    vcpus: 1,
    memory_gb: 2,
    storage_gb: 5,
  },
  SizePresetInfo {
    preset: SizePreset::Medium,
    label: "Medium",
    vcpus: 2,
    memory_gb: 4,
    storage_gb: 40,
  },
  SizePresetInfo {
    preset: SizePreset::Large,
    label: "Large",
    vcpus: 4,
    memory_gb: 8,
    storage_gb: 80,
  },
];

/// A selectable provisioning region. Region codes are Treq-defined identifiers
/// that the adapter maps to provider-specific region names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionCode {
  UsEast,
  UsWest,
  EuWest,
  ApSoutheast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionInfo {
  pub code: RegionCode,
  pub label: &'static str,
}

/// The full list of regions offered to users, in display order.
pub const REGIONS: &[RegionInfo] = &[
  RegionInfo {
    code: RegionCode::UsEast,
    label: "US East",
  },
  RegionInfo {
    code: RegionCode::UsWest,
    label: "US West",
  },
  RegionInfo {
    code: RegionCode::EuWest,
    label: "Europe West",
  },
  RegionInfo {
    code: RegionCode::ApSoutheast,
    label: "Asia Pacific Southeast",
  },
];

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ready_state_is_connectable() {
    assert!(ManagedInstanceState::Ready.is_connectable());
    assert!(!ManagedInstanceState::Suspended.is_connectable());
  }

  #[test]
  fn terminal_states_are_deleted_and_failed() {
    assert!(ManagedInstanceState::Deleted.is_terminal());
    assert!(ManagedInstanceState::Failed.is_terminal());
    assert!(!ManagedInstanceState::Ready.is_terminal());
  }

  #[test]
  fn size_presets_are_defined_smallest_first() {
    assert_eq!(SIZE_PRESETS.first().unwrap().preset, SizePreset::Small);
    assert!(SIZE_PRESETS.windows(2).all(|w| w[0].vcpus <= w[1].vcpus));
  }
}
