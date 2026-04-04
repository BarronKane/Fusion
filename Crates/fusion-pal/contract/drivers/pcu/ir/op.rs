//! Instruction-family vocabulary for the PCU IR core.

use super::{
    PcuBinding,
    PcuBindingAccess,
    PcuBindingRef,
    PcuImageDimension,
    PcuValueType,
};

/// Value-construction or representation-changing operation families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuValueOp {
    Constant,
    Cast,
    Pack,
    Unpack,
    Swizzle,
}

/// Arithmetic / logical operation families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuAluOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    And,
    Or,
    Xor,
    ShiftLeft,
    ShiftRight,
    Compare,
    Select,
}

/// Control-flow operation families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuControlOp {
    Branch,
    Loop,
    Return,
}

/// Sampling level-selection model for one image sampling operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSampleLevel {
    Implicit,
    ExplicitLod,
    Bias,
    Gradient,
}

/// One typed addressed image sampling operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSampleOp {
    pub image: PcuBindingRef,
    pub sampler: PcuBindingRef,
    pub coordinates: PcuValueType,
    pub result_type: PcuValueType,
    pub level: PcuSampleLevel,
    pub offset_components: u8,
}

impl PcuSampleOp {
    /// Creates one basic image+sampler sample operation with implicit level selection.
    #[must_use]
    pub const fn new(
        image: PcuBindingRef,
        sampler: PcuBindingRef,
        coordinates: PcuValueType,
        result_type: PcuValueType,
    ) -> Self {
        Self {
            image,
            sampler,
            coordinates,
            result_type,
            level: PcuSampleLevel::Implicit,
            offset_components: 0,
        }
    }

    /// Replaces the level-selection model.
    #[must_use]
    pub const fn with_level(mut self, level: PcuSampleLevel) -> Self {
        self.level = level;
        self
    }

    /// Declares how many coordinate-space offset components this sample operation carries.
    #[must_use]
    pub const fn with_offset_components(mut self, offset_components: u8) -> Self {
        self.offset_components = offset_components;
        self
    }

    /// Validates that this sample op targets one readable image binding and one sampler binding.
    ///
    /// # Errors
    ///
    /// Returns the first contract mismatch that makes the operation dishonest.
    pub fn validate(self, bindings: &[PcuBinding<'_>]) -> Result<(), PcuSampleValidationError> {
        let image = find_binding(bindings, self.image)
            .ok_or(PcuSampleValidationError::MissingImageBinding(self.image))?;
        let sampler = find_binding(bindings, self.sampler).ok_or(
            PcuSampleValidationError::MissingSamplerBinding(self.sampler),
        )?;
        let Some(image_type) = image.image_type() else {
            return Err(PcuSampleValidationError::ImageBindingIsNotImage(self.image));
        };
        if matches!(image.access, PcuBindingAccess::WriteOnly) {
            return Err(PcuSampleValidationError::ImageBindingNotReadable(
                self.image,
            ));
        }
        if sampler.sampler_type().is_none() {
            return Err(PcuSampleValidationError::SamplerBindingIsNotSampler(
                self.sampler,
            ));
        }
        if !matches!(sampler.access, PcuBindingAccess::ReadOnly) {
            return Err(PcuSampleValidationError::SamplerBindingNotReadable(
                self.sampler,
            ));
        }
        if self.result_type != image_type.texel_type {
            return Err(PcuSampleValidationError::ResultTypeMismatch {
                expected: image_type.texel_type,
                found: self.result_type,
            });
        }

        let required_lanes = image_type.dimension.coordinate_lanes();
        let actual_lanes = self.coordinates.lanes();
        let coordinate_lanes_are_valid = actual_lanes == required_lanes
            || (image_type.arrayed && actual_lanes == required_lanes + 1);
        if !coordinate_lanes_are_valid {
            return Err(PcuSampleValidationError::CoordinateTypeMismatch {
                dimension: image_type.dimension,
                arrayed: image_type.arrayed,
                found: self.coordinates,
            });
        }
        if self.offset_components > 0 && self.offset_components != required_lanes {
            return Err(PcuSampleValidationError::OffsetComponentMismatch {
                expected: required_lanes,
                found: self.offset_components,
            });
        }

        Ok(())
    }
}

/// Contract failures surfaced when one sample op does not actually match the binding graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSampleValidationError {
    MissingImageBinding(PcuBindingRef),
    MissingSamplerBinding(PcuBindingRef),
    ImageBindingIsNotImage(PcuBindingRef),
    SamplerBindingIsNotSampler(PcuBindingRef),
    ImageBindingNotReadable(PcuBindingRef),
    SamplerBindingNotReadable(PcuBindingRef),
    ResultTypeMismatch {
        expected: PcuValueType,
        found: PcuValueType,
    },
    CoordinateTypeMismatch {
        dimension: PcuImageDimension,
        arrayed: bool,
        found: PcuValueType,
    },
    OffsetComponentMismatch {
        expected: u8,
        found: u8,
    },
}

/// Binding-side memory/resource operation families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuBindingOp {
    Load,
    Store,
    Atomic,
    Sample(PcuSampleOp),
}

/// Port-side dataflow operation families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPortOp {
    Receive,
    Send,
    Peek,
    Discard,
}

/// Synchronization / ordering operation families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSyncOp {
    Barrier,
    Fence,
}

/// One abstract PCU instruction family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuOp<'a> {
    Value(PcuValueOp),
    Alu(PcuAluOp),
    Control(PcuControlOp),
    Binding(PcuBindingOp),
    Port(PcuPortOp),
    Sync(PcuSyncOp),
    Intrinsic { name: &'a str },
}

fn find_binding<'a>(
    bindings: &'a [PcuBinding<'a>],
    reference: PcuBindingRef,
) -> Option<PcuBinding<'a>> {
    bindings
        .iter()
        .copied()
        .find(|binding| binding.reference() == reference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::drivers::pcu::{
        PcuBindingStorageClass,
        PcuBindingType,
        PcuImageBindingType,
        PcuSamplerAddressMode,
        PcuSamplerBindingType,
        PcuSamplerCoordinateNormalization,
        PcuSamplerFilter,
        PcuSamplerMipmapMode,
        PcuScalarType,
    };

    #[test]
    fn sample_op_validates_image_and_sampler_bindings() {
        let bindings = [
            PcuBinding::image(
                Some("image"),
                0,
                0,
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
            ),
            PcuBinding::sampler(
                Some("sampler"),
                0,
                1,
                PcuSamplerBindingType {
                    coordinate_normalization: PcuSamplerCoordinateNormalization::Normalized,
                    min_filter: PcuSamplerFilter::Linear,
                    mag_filter: PcuSamplerFilter::Linear,
                    mipmap_mode: PcuSamplerMipmapMode::Linear,
                    address_u: PcuSamplerAddressMode::Repeat,
                    address_v: PcuSamplerAddressMode::Repeat,
                    address_w: PcuSamplerAddressMode::Repeat,
                },
            ),
        ];
        let sample = PcuSampleOp::new(
            PcuBindingRef::new(0, 0),
            PcuBindingRef::new(0, 1),
            PcuValueType::Vector {
                scalar: PcuScalarType::F32,
                lanes: 2,
            },
            PcuValueType::Vector {
                scalar: PcuScalarType::F32,
                lanes: 4,
            },
        );

        assert_eq!(sample.validate(&bindings), Ok(()));
        assert_eq!(PcuBindingOp::Sample(sample), PcuBindingOp::Sample(sample));
    }

    #[test]
    fn sample_op_rejects_non_image_or_non_sampler_bindings() {
        let bindings = [
            PcuBinding::value(
                Some("buffer"),
                0,
                0,
                PcuBindingStorageClass::Storage,
                PcuBindingAccess::ReadOnly,
                PcuValueType::Scalar(PcuScalarType::U32),
            ),
            PcuBinding {
                name: Some("fake_sampler"),
                set: 0,
                binding: 1,
                storage: PcuBindingStorageClass::Uniform,
                access: PcuBindingAccess::ReadOnly,
                binding_type: PcuBindingType::Value(PcuValueType::Scalar(PcuScalarType::U32)),
                builtin: None,
            },
        ];
        let sample = PcuSampleOp::new(
            PcuBindingRef::new(0, 0),
            PcuBindingRef::new(0, 1),
            PcuValueType::Vector {
                scalar: PcuScalarType::F32,
                lanes: 2,
            },
            PcuValueType::Scalar(PcuScalarType::U32),
        );

        assert_eq!(
            sample.validate(&bindings),
            Err(PcuSampleValidationError::ImageBindingIsNotImage(
                PcuBindingRef::new(0, 0)
            ))
        );
    }
}
