//! Shared GPU composition vocabulary.

use core::num::NonZeroU64;

/// Opaque GPU-visible resource handle.
///
/// This is intentionally a stand-in until `fusion-gpu` is wired to a real `fusion-sys::mem`
/// residency/resource handle story. The value remains opaque to consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuResourceHandle(NonZeroU64);

impl GpuResourceHandle {
    #[must_use]
    pub const fn new(raw: NonZeroU64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> NonZeroU64 {
        self.0
    }
}

/// Two-dimensional framebuffer extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuExtent2D {
    pub width: u32,
    pub height: u32,
}

impl GpuExtent2D {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Multisample count surfaced by one framebuffer attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuSampleCount {
    One,
    Two,
    Four,
    Eight,
    Sixteen,
}

impl GpuSampleCount {
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }

    #[must_use]
    pub const fn supports(self, required: Self) -> bool {
        self.as_u8() >= required.as_u8()
    }
}

/// Attachment format class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuFormatClass {
    Color,
    Depth,
    Stencil,
    DepthStencil,
}

/// Typed attachment/storage format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuFormat {
    Rgba8Unorm,
    Bgra8Unorm,
    Rgba16Float,
    Rgba32Float,
    R32Uint,
    Depth16Unorm,
    Depth32Float,
    Stencil8Uint,
    Depth24Stencil8,
    Depth32FloatStencil8,
}

impl GpuFormat {
    #[must_use]
    pub const fn class(self) -> GpuFormatClass {
        match self {
            Self::Rgba8Unorm
            | Self::Bgra8Unorm
            | Self::Rgba16Float
            | Self::Rgba32Float
            | Self::R32Uint => GpuFormatClass::Color,
            Self::Depth16Unorm | Self::Depth32Float => GpuFormatClass::Depth,
            Self::Stencil8Uint => GpuFormatClass::Stencil,
            Self::Depth24Stencil8 | Self::Depth32FloatStencil8 => GpuFormatClass::DepthStencil,
        }
    }
}

/// Role of one framebuffer attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuAttachmentRole {
    Color { index: u8 },
    Depth,
    Stencil,
    DepthStencil,
}

impl GpuAttachmentRole {
    #[must_use]
    pub const fn color(index: u8) -> Self {
        Self::Color { index }
    }
}
