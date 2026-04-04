//! Capability vocabulary for generic Bluetooth backends.

use bitflags::bitflags;

/// Implementation-category vocabulary specialized for Bluetooth support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothImplementationKind {
    /// Native backend implementation.
    Native,
    /// Lowered or adapted implementation that preserves the public contract with caveats.
    Emulated,
    /// Unsupported placeholder.
    Unsupported,
}

bitflags! {
    /// Provider-wide Bluetooth backend features the surfaced implementation can honestly expose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothProviderCaps: u32 {
        /// The backend can enumerate surfaced Bluetooth adapters or controllers.
        const ENUMERATE_ADAPTERS         = 1 << 0;
        /// Adapters can be opened explicitly through the control surface.
        const OPEN_ADAPTER               = 1 << 1;
        /// Surfaced adapter topology is statically known.
        const STATIC_TOPOLOGY            = 1 << 2;
        /// Adapter presence may change at runtime.
        const HOTPLUG                    = 1 << 3;
        /// Adapter power can be controlled explicitly.
        const POWER_CONTROL              = 1 << 4;
        /// The backend can surface BR/EDR capability.
        const BR_EDR                     = 1 << 5;
        /// The backend can surface Bluetooth LE capability.
        const LE                         = 1 << 6;
        /// The backend can surface L2CAP functionality.
        const L2CAP                      = 1 << 7;
        /// The backend can surface ATT transactions.
        const ATT                        = 1 << 8;
        /// The backend can surface GATT client functionality.
        const GATT_CLIENT                = 1 << 9;
        /// The backend can surface GATT server functionality.
        const GATT_SERVER                = 1 << 10;
        /// The backend can surface security-manager functionality.
        const SECURITY_MANAGER           = 1 << 11;
        /// The backend can surface isochronous Bluetooth capability.
        const ISOCHRONOUS                = 1 << 12;
        /// The backend can surface mesh capability.
        const MESH                       = 1 << 13;
        /// The backend can surface direction-finding capability.
        const DIRECTION_FINDING          = 1 << 14;
    }
}

bitflags! {
    /// Honest role set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothRoleCaps: u32 {
        const CENTRAL                    = 1 << 0;
        const PERIPHERAL                 = 1 << 1;
        const OBSERVER                   = 1 << 2;
        const BROADCASTER                = 1 << 3;
    }
}

bitflags! {
    /// Honest transport set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothTransportCaps: u32 {
        const BR_EDR                     = 1 << 0;
        const LE                         = 1 << 1;
    }
}

bitflags! {
    /// Honest LE PHY support for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothLePhyCaps: u32 {
        const LE_1M                      = 1 << 0;
        const LE_2M                      = 1 << 1;
        const LE_CODED_S2                = 1 << 2;
        const LE_CODED_S8                = 1 << 3;
    }
}

bitflags! {
    /// Honest advertising capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothAdvertisingCaps: u32 {
        const LEGACY                     = 1 << 0;
        const EXTENDED                   = 1 << 1;
        const PERIODIC                   = 1 << 2;
        const CONNECTABLE                = 1 << 3;
        const SCANNABLE                  = 1 << 4;
        const DIRECTED                   = 1 << 5;
        const ANONYMOUS                  = 1 << 6;
        const TX_POWER_REPORTING         = 1 << 7;
    }
}

bitflags! {
    /// Honest scanning capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothScanningCaps: u32 {
        const PASSIVE                    = 1 << 0;
        const ACTIVE                     = 1 << 1;
        const EXTENDED                   = 1 << 2;
        const PERIODIC_SYNC              = 1 << 3;
        const FILTER_DUPLICATES          = 1 << 4;
    }
}

bitflags! {
    /// Honest connection-management capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothConnectionCaps: u32 {
        const BR_EDR_CONNECTIONS         = 1 << 0;
        const LE_CONNECTIONS             = 1 << 1;
        const CONNECTION_PARAMETER_UPDATE= 1 << 2;
        const DATA_LENGTH_UPDATE         = 1 << 3;
        const PHY_UPDATE                 = 1 << 4;
        const SUBRATING                  = 1 << 5;
    }
}

bitflags! {
    /// Honest security capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothSecurityCaps: u32 {
        const LEGACY_PAIRING             = 1 << 0;
        const SECURE_SIMPLE_PAIRING      = 1 << 1;
        const LE_LEGACY_PAIRING          = 1 << 2;
        const LE_SECURE_CONNECTIONS      = 1 << 3;
        const BONDING                    = 1 << 4;
        const PRIVACY                    = 1 << 5;
        const MITM_PROTECTION            = 1 << 6;
        const OOB_PAIRING                = 1 << 7;
        const SIGNED_WRITES              = 1 << 8;
        const KEYPRESS_NOTIFICATIONS     = 1 << 9;
    }
}

bitflags! {
    /// Honest L2CAP capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothL2capCaps: u32 {
        const FIXED_CHANNELS             = 1 << 0;
        const CONNECTION_ORIENTED        = 1 << 1;
        const CONNECTIONLESS             = 1 << 2;
        const CREDIT_BASED               = 1 << 3;
        const ENHANCED_CREDIT_BASED      = 1 << 4;
    }
}

bitflags! {
    /// Honest ATT capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothAttCaps: u32 {
        const MTU_EXCHANGE               = 1 << 0;
        const READ                       = 1 << 1;
        const WRITE                      = 1 << 2;
        const PREPARED_WRITE             = 1 << 3;
        const SIGNED_WRITE               = 1 << 4;
        const NOTIFY                     = 1 << 5;
        const INDICATE                   = 1 << 6;
    }
}

bitflags! {
    /// Honest GATT/ATT capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothGattCaps: u32 {
        const GATT_CLIENT                = 1 << 0;
        const GATT_SERVER                = 1 << 1;
        const PRIMARY_SERVICE_DISCOVERY  = 1 << 2;
        const CHARACTERISTIC_DISCOVERY   = 1 << 3;
        const DESCRIPTOR_DISCOVERY       = 1 << 4;
        const LONG_READ                  = 1 << 5;
        const LONG_WRITE                 = 1 << 6;
        const RELIABLE_WRITE             = 1 << 7;
        const NOTIFY                     = 1 << 8;
        const INDICATE                   = 1 << 9;
        const SERVICE_CHANGED            = 1 << 10;
    }
}

bitflags! {
    /// Honest isochronous capability set for one surfaced Bluetooth adapter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BluetoothIsoCaps: u32 {
        const CONNECTED_ISOCHRONOUS      = 1 << 0;
        const BROADCAST_ISOCHRONOUS      = 1 << 1;
        const LE_AUDIO                   = 1 << 2;
    }
}

/// Full provider-wide capability surface for one generic Bluetooth backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothSupport {
    /// Backend-supported generic Bluetooth provider features.
    pub caps: BluetoothProviderCaps,
    /// Native, lowered-with-restrictions, or unsupported implementation category.
    pub implementation: BluetoothImplementationKind,
    /// Number of surfaced Bluetooth adapters/controllers.
    pub adapter_count: u16,
}

impl BluetoothSupport {
    /// Returns a fully unsupported generic Bluetooth surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: BluetoothProviderCaps::empty(),
            implementation: BluetoothImplementationKind::Unsupported,
            adapter_count: 0,
        }
    }
}
