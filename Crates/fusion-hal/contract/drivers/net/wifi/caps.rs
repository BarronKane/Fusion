//! Capability vocabulary for generic Wi-Fi backends.

use bitflags::bitflags;

/// Implementation-category vocabulary specialized for Wi-Fi support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiImplementationKind {
    /// Native backend implementation.
    Native,
    /// Lowered or adapted implementation that preserves the public contract with caveats.
    Emulated,
    /// Unsupported placeholder.
    Unsupported,
}

bitflags! {
    /// Provider-wide Wi-Fi backend features the surfaced implementation can honestly expose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiProviderCaps: u32 {
        const ENUMERATE_ADAPTERS         = 1 << 0;
        const OPEN_ADAPTER               = 1 << 1;
        const STATIC_TOPOLOGY            = 1 << 2;
        const HOTPLUG                    = 1 << 3;
        const POWER_CONTROL              = 1 << 4;
        const RADIO_CONTROL              = 1 << 5;
        const SCAN                       = 1 << 6;
        const STATION                    = 1 << 7;
        const ACCESS_POINT               = 1 << 8;
        const DATA                       = 1 << 9;
        const MONITOR                    = 1 << 10;
        const P2P                        = 1 << 11;
        const MESH                       = 1 << 12;
        const SECURITY                   = 1 << 13;
    }
}

bitflags! {
    /// Supported Wi-Fi standard families.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiStandardFamilyCaps: u32 {
        const LEGACY                     = 1 << 0;
        const HT                         = 1 << 1;
        const VHT                        = 1 << 2;
        const HE                         = 1 << 3;
        const EHT                        = 1 << 4;
    }
}

bitflags! {
    /// Supported Wi-Fi adapter roles.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiRoleCaps: u32 {
        const STATION                    = 1 << 0;
        const ACCESS_POINT               = 1 << 1;
        const MONITOR                    = 1 << 2;
        const P2P                        = 1 << 3;
        const MESH                       = 1 << 4;
    }
}

bitflags! {
    /// Supported Wi-Fi frequency bands.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiBandCaps: u32 {
        const GHZ_2_4                    = 1 << 0;
        const GHZ_5                      = 1 << 1;
        const GHZ_6                      = 1 << 2;
        const GHZ_60                     = 1 << 3;
    }
}

bitflags! {
    /// Supported Wi-Fi channel widths.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiChannelWidthCaps: u32 {
        const WIDTH_20_MHZ               = 1 << 0;
        const WIDTH_40_MHZ               = 1 << 1;
        const WIDTH_80_MHZ               = 1 << 2;
        const WIDTH_160_MHZ              = 1 << 3;
        const WIDTH_320_MHZ              = 1 << 4;
    }
}

bitflags! {
    /// Supported Wi-Fi security suite families.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiSecurityCaps: u32 {
        const OPEN                       = 1 << 0;
        const WEP                        = 1 << 1;
        const WPA_PERSONAL               = 1 << 2;
        const WPA2_PERSONAL              = 1 << 3;
        const WPA3_PERSONAL              = 1 << 4;
        const WPA2_ENTERPRISE            = 1 << 5;
        const WPA3_ENTERPRISE            = 1 << 6;
        const PMF                        = 1 << 7;
        const SAE                        = 1 << 8;
        const OWE                        = 1 << 9;
    }
}

bitflags! {
    /// Supported scan-mode features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiScanCaps: u32 {
        const PASSIVE                    = 1 << 0;
        const ACTIVE                     = 1 << 1;
        const FILTER_BY_SSID             = 1 << 2;
        const FILTER_BY_BSSID            = 1 << 3;
        const FILTER_BY_CHANNEL          = 1 << 4;
        const BACKGROUND_SCAN            = 1 << 5;
    }
}

bitflags! {
    /// Supported station-mode features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiStationCaps: u32 {
        const CONNECT                    = 1 << 0;
        const DISCONNECT                 = 1 << 1;
        const ROAM                       = 1 << 2;
        const POWERSAVE                  = 1 << 3;
        const FAST_TRANSITION            = 1 << 4;
    }
}

bitflags! {
    /// Supported access-point features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiAccessPointCaps: u32 {
        const HOST_BSS                   = 1 << 0;
        const HIDDEN_SSID                = 1 << 1;
        const MULTI_BSS                  = 1 << 2;
        const CLIENT_ISOLATION           = 1 << 3;
        const WMM                        = 1 << 4;
    }
}

bitflags! {
    /// Supported data-plane features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiDataCaps: u32 {
        const LINK_DATA                  = 1 << 0;
        const QOS                        = 1 << 1;
        const AMSDU                      = 1 << 2;
        const AMPDU                      = 1 << 3;
        const RAW_FRAME_TX               = 1 << 4;
        const RAW_FRAME_RX               = 1 << 5;
    }
}

bitflags! {
    /// Supported monitor-mode features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiMonitorCaps: u32 {
        const CAPTURE                    = 1 << 0;
        const FCS_STATUS                 = 1 << 1;
        const INJECT                     = 1 << 2;
        const RADIOTAP                   = 1 << 3;
    }
}

bitflags! {
    /// Supported Wi-Fi Direct features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiP2pCaps: u32 {
        const DISCOVERY                  = 1 << 0;
        const GROUP_OWNER                = 1 << 1;
        const CLIENT                     = 1 << 2;
        const SERVICE_DISCOVERY          = 1 << 3;
    }
}

bitflags! {
    /// Supported Wi-Fi mesh features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiMeshCaps: u32 {
        const JOIN                       = 1 << 0;
        const FORWARDING                 = 1 << 1;
        const PEERING                    = 1 << 2;
        const GATEWAY                    = 1 << 3;
    }
}

bitflags! {
    /// Supported multi-link operation features.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WifiMloCaps: u32 {
        const MULTI_LINK_STATION         = 1 << 0;
        const MULTI_LINK_AP              = 1 << 1;
        const LINK_STEERING              = 1 << 2;
    }
}

/// Full provider-wide capability surface for one generic Wi-Fi backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WifiSupport {
    /// Backend-supported generic Wi-Fi provider features.
    pub caps: WifiProviderCaps,
    /// Native, lowered-with-restrictions, or unsupported implementation category.
    pub implementation: WifiImplementationKind,
    /// Number of surfaced Wi-Fi adapters/controllers.
    pub adapter_count: u16,
}

impl WifiSupport {
    /// Returns a fully unsupported generic Wi-Fi surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: WifiProviderCaps::empty(),
            implementation: WifiImplementationKind::Unsupported,
            adapter_count: 0,
        }
    }
}
