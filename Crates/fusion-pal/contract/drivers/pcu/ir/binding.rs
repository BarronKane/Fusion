//! Memory/resource attachment vocabulary for the PCU IR core.

use super::{PcuScalarType, PcuValueType};

/// Storage-class vocabulary for one resource binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuBindingStorageClass {
    Input,
    Output,
    Uniform,
    Storage,
    Workgroup,
    PushConstant,
    Private,
    Image,
    Sampler,
    Constant,
}

/// Access mode for one resource binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuBindingAccess {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

/// Canonical set/binding address for one kernel resource attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuBindingRef {
    pub set: u32,
    pub binding: u32,
}

impl PcuBindingRef {
    /// Creates one binding reference from its canonical set/binding coordinates.
    #[must_use]
    pub const fn new(set: u32, binding: u32) -> Self {
        Self { set, binding }
    }
}

/// Dimensional shape for one image binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuImageDimension {
    D1,
    D2,
    D3,
    Cube,
}

impl PcuImageDimension {
    /// Returns the base coordinate lane count required for one sample from this image shape.
    #[must_use]
    pub const fn coordinate_lanes(self) -> u8 {
        match self {
            Self::D1 => 1,
            Self::D2 => 2,
            Self::D3 | Self::Cube => 3,
        }
    }
}

/// Typed image resource description for one image binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuImageBindingType {
    pub dimension: PcuImageDimension,
    pub texel_type: PcuValueType,
    pub arrayed: bool,
    pub multisampled: bool,
}

impl PcuImageBindingType {
    /// Returns the base coordinate type shape required to address this image.
    #[must_use]
    pub const fn coordinate_type(self) -> PcuValueType {
        match self.dimension.coordinate_lanes() {
            1 => PcuValueType::Scalar(PcuScalarType::F32),
            lanes => PcuValueType::Vector {
                scalar: PcuScalarType::F32,
                lanes,
            },
        }
    }
}

/// Coordinate normalization model for one sampler binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSamplerCoordinateNormalization {
    Normalized,
    Unnormalized,
}

/// Addressing mode surfaced by one sampler binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSamplerAddressMode {
    ClampToEdge,
    ClampToBorder,
    Repeat,
    MirrorRepeat,
}

/// Filter kernel surfaced by one sampler binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSamplerFilter {
    Nearest,
    Linear,
}

/// Mipmap selection mode surfaced by one sampler binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSamplerMipmapMode {
    None,
    Nearest,
    Linear,
}

/// Typed sampler-state description for one sampler binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSamplerBindingType {
    pub coordinate_normalization: PcuSamplerCoordinateNormalization,
    pub min_filter: PcuSamplerFilter,
    pub mag_filter: PcuSamplerFilter,
    pub mipmap_mode: PcuSamplerMipmapMode,
    pub address_u: PcuSamplerAddressMode,
    pub address_v: PcuSamplerAddressMode,
    pub address_w: PcuSamplerAddressMode,
}

/// Honest resource payload carried by one binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuBindingType {
    Value(PcuValueType),
    Image(PcuImageBindingType),
    Sampler(PcuSamplerBindingType),
}

impl PcuBindingType {
    /// Returns the bound value type, when this attachment is ordinary typed memory/storage.
    #[must_use]
    pub const fn value_type(self) -> Option<PcuValueType> {
        match self {
            Self::Value(value_type) => Some(value_type),
            Self::Image(_) | Self::Sampler(_) => None,
        }
    }

    /// Returns the bound image description, when this attachment is one typed image surface.
    #[must_use]
    pub const fn image_type(self) -> Option<PcuImageBindingType> {
        match self {
            Self::Image(image_type) => Some(image_type),
            Self::Value(_) | Self::Sampler(_) => None,
        }
    }

    /// Returns the bound sampler description, when this attachment is one sampler-state surface.
    #[must_use]
    pub const fn sampler_type(self) -> Option<PcuSamplerBindingType> {
        match self {
            Self::Sampler(sampler_type) => Some(sampler_type),
            Self::Value(_) | Self::Image(_) => None,
        }
    }
}

/// Builtin values surfaced through the binding path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuBuiltinValue {
    GlobalInvocationId,
    LocalInvocationId,
    WorkgroupId,
    NumWorkgroups,
    LocalInvocationIndex,
    VertexId,
    InstanceId,
    Position,
    FragCoord,
    FrontFacing,
    SampleId,
}

/// One typed memory/resource attachment for one kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuBinding<'a> {
    pub name: Option<&'a str>,
    pub set: u32,
    pub binding: u32,
    pub storage: PcuBindingStorageClass,
    pub access: PcuBindingAccess,
    pub binding_type: PcuBindingType,
    pub builtin: Option<PcuBuiltinValue>,
}

impl<'a> PcuBinding<'a> {
    /// Creates one ordinary typed value binding.
    #[must_use]
    pub const fn value(
        name: Option<&'a str>,
        set: u32,
        binding: u32,
        storage: PcuBindingStorageClass,
        access: PcuBindingAccess,
        value_type: PcuValueType,
    ) -> Self {
        Self {
            name,
            set,
            binding,
            storage,
            access,
            binding_type: PcuBindingType::Value(value_type),
            builtin: None,
        }
    }

    /// Creates one typed image binding.
    #[must_use]
    pub const fn image(
        name: Option<&'a str>,
        set: u32,
        binding: u32,
        access: PcuBindingAccess,
        image_type: PcuImageBindingType,
    ) -> Self {
        Self {
            name,
            set,
            binding,
            storage: PcuBindingStorageClass::Image,
            access,
            binding_type: PcuBindingType::Image(image_type),
            builtin: None,
        }
    }

    /// Creates one typed sampler binding.
    #[must_use]
    pub const fn sampler(
        name: Option<&'a str>,
        set: u32,
        binding: u32,
        sampler_type: PcuSamplerBindingType,
    ) -> Self {
        Self {
            name,
            set,
            binding,
            storage: PcuBindingStorageClass::Sampler,
            access: PcuBindingAccess::ReadOnly,
            binding_type: PcuBindingType::Sampler(sampler_type),
            builtin: None,
        }
    }

    /// Returns the canonical reference for this binding.
    #[must_use]
    pub const fn reference(self) -> PcuBindingRef {
        PcuBindingRef::new(self.set, self.binding)
    }

    /// Returns the value type, when this binding is ordinary typed memory/storage.
    #[must_use]
    pub const fn value_type(self) -> Option<PcuValueType> {
        self.binding_type.value_type()
    }

    /// Returns the image description, when this binding is one image surface.
    #[must_use]
    pub const fn image_type(self) -> Option<PcuImageBindingType> {
        self.binding_type.image_type()
    }

    /// Returns the sampler description, when this binding is one sampler surface.
    #[must_use]
    pub const fn sampler_type(self) -> Option<PcuSamplerBindingType> {
        self.binding_type.sampler_type()
    }

    /// Returns whether the binding's storage class and payload shape describe the same reality.
    #[must_use]
    pub const fn is_well_formed(self) -> bool {
        match (self.storage, self.binding_type) {
            (PcuBindingStorageClass::Image, PcuBindingType::Image(_)) => true,
            (PcuBindingStorageClass::Sampler, PcuBindingType::Sampler(_)) => {
                matches!(self.access, PcuBindingAccess::ReadOnly)
            }
            (PcuBindingStorageClass::Image, _)
            | (PcuBindingStorageClass::Sampler, _)
            | (_, PcuBindingType::Image(_))
            | (_, PcuBindingType::Sampler(_)) => false,
            (_, PcuBindingType::Value(_)) => true,
        }
    }
}

/// Back-compat alias while the dispatch profile keeps using the older vocabulary.
pub type PcuComputeStorageClass = PcuBindingStorageClass;

/// Back-compat alias while the dispatch profile keeps using the older vocabulary.
pub type PcuComputeBuiltin = PcuBuiltinValue;

/// Back-compat alias while the dispatch profile keeps using the older vocabulary.
pub type PcuComputeBinding<'a> = PcuBinding<'a>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_and_image_bindings_are_not_forced_through_value_types() {
        let image = PcuBinding::image(
            Some("albedo"),
            0,
            3,
            PcuBindingAccess::ReadOnly,
            PcuImageBindingType {
                dimension: PcuImageDimension::D2,
                texel_type: PcuValueType::Vector {
                    scalar: PcuScalarType::F32,
                    lanes: 4,
                },
                arrayed: false,
                multisampled: false,
            },
        );
        let sampler = PcuBinding::sampler(
            Some("linear_sampler"),
            0,
            4,
            PcuSamplerBindingType {
                coordinate_normalization: PcuSamplerCoordinateNormalization::Normalized,
                min_filter: PcuSamplerFilter::Linear,
                mag_filter: PcuSamplerFilter::Linear,
                mipmap_mode: PcuSamplerMipmapMode::Linear,
                address_u: PcuSamplerAddressMode::Repeat,
                address_v: PcuSamplerAddressMode::Repeat,
                address_w: PcuSamplerAddressMode::Repeat,
            },
        );

        assert!(image.is_well_formed());
        assert!(sampler.is_well_formed());
        assert_eq!(image.value_type(), None);
        assert_eq!(sampler.value_type(), None);
        assert_eq!(image.reference(), PcuBindingRef::new(0, 3));
        assert_eq!(sampler.reference(), PcuBindingRef::new(0, 4));
    }

    #[test]
    fn binding_shape_must_match_storage_class() {
        let invalid_sampler = PcuBinding {
            name: Some("broken"),
            set: 0,
            binding: 9,
            storage: PcuBindingStorageClass::Uniform,
            access: PcuBindingAccess::ReadOnly,
            binding_type: PcuBindingType::Sampler(PcuSamplerBindingType {
                coordinate_normalization: PcuSamplerCoordinateNormalization::Normalized,
                min_filter: PcuSamplerFilter::Nearest,
                mag_filter: PcuSamplerFilter::Nearest,
                mipmap_mode: PcuSamplerMipmapMode::None,
                address_u: PcuSamplerAddressMode::ClampToEdge,
                address_v: PcuSamplerAddressMode::ClampToEdge,
                address_w: PcuSamplerAddressMode::ClampToEdge,
            }),
            builtin: None,
        };

        assert!(!invalid_sampler.is_well_formed());
    }
}
