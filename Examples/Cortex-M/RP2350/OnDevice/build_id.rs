//! Fixed-layout build identity exported by RP2350 example binaries.
//!
//! This gives the Pico tooling one truthful way to answer the only question that matters after a
//! flash: "is the board actually running the ELF we just built, or are we all participating in
//! another embedded séance?"

pub const FUSION_RP2350_BUILD_ID_MAGIC: u32 = 0x4642_4944;
pub const FUSION_RP2350_BUILD_ID_VERSION: u16 = 1;

const PACKAGE_NAME_LEN: usize = 48;
const BIN_NAME_LEN: usize = 32;
const PROFILE_LEN: usize = 12;
const TARGET_LEN: usize = 32;
const GIT_SHA_LEN: usize = 16;
const FEATURES_HASH_LEN: usize = 16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FusionRp2350BuildIdV1 {
    pub magic: u32,
    pub version: u16,
    pub dirty: u8,
    pub reserved: u8,
    pub package_name: [u8; PACKAGE_NAME_LEN],
    pub bin_name: [u8; BIN_NAME_LEN],
    pub profile: [u8; PROFILE_LEN],
    pub target: [u8; TARGET_LEN],
    pub git_sha: [u8; GIT_SHA_LEN],
    pub features_hash: [u8; FEATURES_HASH_LEN],
}

impl FusionRp2350BuildIdV1 {
    #[must_use]
    pub const fn new(
        package_name: &str,
        bin_name: &str,
        profile: &str,
        target: &str,
        git_sha: &str,
        features_hash: &str,
        dirty: bool,
    ) -> Self {
        Self {
            magic: FUSION_RP2350_BUILD_ID_MAGIC,
            version: FUSION_RP2350_BUILD_ID_VERSION,
            dirty: if dirty { 1 } else { 0 },
            reserved: 0,
            package_name: copy_ascii_fixed::<PACKAGE_NAME_LEN>(package_name),
            bin_name: copy_ascii_fixed::<BIN_NAME_LEN>(bin_name),
            profile: copy_ascii_fixed::<PROFILE_LEN>(profile),
            target: copy_ascii_fixed::<TARGET_LEN>(target),
            git_sha: copy_ascii_fixed::<GIT_SHA_LEN>(git_sha),
            features_hash: copy_ascii_fixed::<FEATURES_HASH_LEN>(features_hash),
        }
    }
}

const fn copy_ascii_fixed<const N: usize>(value: &str) -> [u8; N] {
    let bytes = value.as_bytes();
    let mut out = [0_u8; N];
    let mut index = 0;
    while index < N && index < bytes.len() {
        out[index] = bytes[index];
        index += 1;
    }
    out
}

pub const fn option_or<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    match value {
        Some(value) => value,
        None => fallback,
    }
}

pub const fn option_is_one(value: Option<&str>) -> bool {
    match value {
        Some(value) => {
            let bytes = value.as_bytes();
            bytes.len() == 1 && bytes[0] == b'1'
        }
        None => false,
    }
}

#[macro_export]
macro_rules! fusion_rp2350_export_build_id {
    () => {
        #[used]
        #[unsafe(no_mangle)]
        // This is the *input* object-file section name. The linker script intentionally gathers it
        // into the ELF output section `.fusion_build_id` (underscore), which is what the Pico
        // tooling later reads back from the linked image.
        #[unsafe(link_section = ".fusion.build_id")]
        pub static FUSION_RP2350_BUILD_ID: $crate::build_id::FusionRp2350BuildIdV1 =
            $crate::build_id::FusionRp2350BuildIdV1::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_BIN_NAME"),
                $crate::build_id::option_or(option_env!("FUSION_RP2350_BUILD_PROFILE"), "unknown"),
                $crate::build_id::option_or(option_env!("FUSION_RP2350_BUILD_TARGET"), "unknown"),
                $crate::build_id::option_or(option_env!("FUSION_RP2350_BUILD_GIT_SHA"), "nogit"),
                $crate::build_id::option_or(
                    option_env!("FUSION_RP2350_BUILD_FEATURES_HASH"),
                    "0",
                ),
                $crate::build_id::option_is_one(option_env!("FUSION_RP2350_BUILD_DIRTY")),
            );
    };
}
