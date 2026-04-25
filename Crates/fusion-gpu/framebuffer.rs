//! Framebuffer composition vocabulary.
//!
//! This first cut is deliberately narrow. It gives `fusion-gpu` one honest typed framebuffer
//! stand-in that future resource/pipeline/surface work can compose around without pretending
//! residency or presentation are already solved.

use crate::{
    GpuAttachmentRole,
    GpuExtent2D,
    GpuFormat,
    GpuFormatClass,
    GpuFillOperation,
    GpuDrawPipeline,
    GpuResourceHandle,
    GpuSampleCount,
};
use fusion_pcu::PcuRenderKernel;

/// Active framebuffer extension envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuFramebufferExtensions {
    pub rasterizer: Option<GpuRasterizerExtension>,
    pub blending: Option<GpuBlendingExtension>,
    pub depth_stencil: Option<GpuDepthStencilExtension>,
    pub multisample: Option<GpuMultisampleExtension>,
    pub ray_trace: Option<GpuRayTraceExtension>,
}

impl GpuFramebufferExtensions {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            rasterizer: None,
            blending: None,
            depth_stencil: None,
            multisample: None,
            ray_trace: None,
        }
    }
}

/// Rasterization extension envelope for one framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuRasterizerExtension {
    pub mesh_shading: bool,
    pub wireframe: bool,
}

/// Blend/ROP extension envelope for one framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuBlendingExtension {
    pub max_color_attachments: u8,
    pub independent_blend: bool,
}

/// Depth/stencil extension envelope for one framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuDepthStencilExtension {
    pub depth_test: bool,
    pub stencil_test: bool,
}

/// Multisample extension envelope for one framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuMultisampleExtension {
    pub max_samples: GpuSampleCount,
    pub sample_shading: bool,
    pub alpha_to_coverage: bool,
}

/// Ray-trace extension envelope for one framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuRayTraceExtension {
    pub max_recursion_depth: u8,
}

/// One typed framebuffer attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuFramebufferAttachment<'a> {
    pub name: Option<&'a str>,
    pub handle: GpuResourceHandle,
    pub role: GpuAttachmentRole,
    pub format: GpuFormat,
    pub extent: GpuExtent2D,
    pub samples: GpuSampleCount,
}

impl<'a> GpuFramebufferAttachment<'a> {
    #[must_use]
    pub const fn new(
        name: Option<&'a str>,
        handle: GpuResourceHandle,
        role: GpuAttachmentRole,
        format: GpuFormat,
        extent: GpuExtent2D,
        samples: GpuSampleCount,
    ) -> Self {
        Self {
            name,
            handle,
            role,
            format,
            extent,
            samples,
        }
    }

    #[must_use]
    pub const fn is_role_compatible(self) -> bool {
        match (self.role, self.format.class()) {
            (GpuAttachmentRole::Color { .. }, GpuFormatClass::Color)
            | (GpuAttachmentRole::Depth, GpuFormatClass::Depth)
            | (GpuAttachmentRole::Stencil, GpuFormatClass::Stencil)
            | (GpuAttachmentRole::DepthStencil, GpuFormatClass::DepthStencil) => true,
            _ => false,
        }
    }
}

/// Validation failure for one framebuffer description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuFramebufferValidationError {
    EmptyAttachments,
    EmptyExtent,
    AttachmentExtentMismatch,
    AttachmentSampleMismatch,
    ColorAttachmentIndexOutOfRange(u8),
    DuplicateColorAttachmentIndex(u8),
    DuplicateDepthAttachment,
    DuplicateStencilAttachment,
    DuplicateDepthStencilAttachment,
    RoleFormatMismatch,
    BlendingWithoutColorAttachment,
    DepthStencilExtensionWithoutAttachment,
    MultisampleExtensionBelowAttachmentSampleCount,
}

/// Compatibility failure for drawing/filling against one framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuFramebufferCompatibilityError {
    Structural(GpuFramebufferValidationError),
    MissingRasterizerExtension,
    MissingRayTraceExtension,
    MeshShadingUnsupported,
    MissingDepthStencilExtension,
    MissingDepthStencilAttachment,
    MissingColorAttachment,
    BlendStateWithoutExtension,
    BlendAttachmentCountExceeded {
        requested: u8,
        available: u8,
        max_supported: u8,
    },
    IndependentBlendUnsupported,
    MultisampleExtensionMissing,
    MultisampleStateMismatch,
    SampleShadingUnsupported,
    AlphaToCoverageUnsupported,
    FillTargetsEmpty,
    FillTargetMissing(u8),
    UnsupportedRenderKernelFamily,
}

/// One typed framebuffer description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuFramebuffer<'a> {
    pub name: Option<&'a str>,
    pub extent: GpuExtent2D,
    pub attachments: &'a [GpuFramebufferAttachment<'a>],
    pub extensions: GpuFramebufferExtensions,
}

impl<'a> GpuFramebuffer<'a> {
    #[must_use]
    pub const fn new(
        name: Option<&'a str>,
        extent: GpuExtent2D,
        attachments: &'a [GpuFramebufferAttachment<'a>],
    ) -> Self {
        Self {
            name,
            extent,
            attachments,
            extensions: GpuFramebufferExtensions::empty(),
        }
    }

    #[must_use]
    pub const fn with_extensions(mut self, extensions: GpuFramebufferExtensions) -> Self {
        self.extensions = extensions;
        self
    }

    /// Validates the framebuffer description.
    ///
    /// # Errors
    ///
    /// Returns one honest structural validation error when the framebuffer description is not
    /// self-consistent.
    pub fn validate(&self) -> Result<(), GpuFramebufferValidationError> {
        if self.attachments.is_empty() {
            return Err(GpuFramebufferValidationError::EmptyAttachments);
        }
        if self.extent.is_empty() {
            return Err(GpuFramebufferValidationError::EmptyExtent);
        }

        let mut color_mask = 0_u32;
        let mut seen_depth = false;
        let mut seen_stencil = false;
        let mut seen_depth_stencil = false;

        for attachment in self.attachments.iter().copied() {
            if attachment.extent != self.extent {
                return Err(GpuFramebufferValidationError::AttachmentExtentMismatch);
            }
            if !attachment.is_role_compatible() {
                return Err(GpuFramebufferValidationError::RoleFormatMismatch);
            }

            if let Some(first) = self.attachments.first().copied() {
                if attachment.samples != first.samples {
                    return Err(GpuFramebufferValidationError::AttachmentSampleMismatch);
                }
            }

            match attachment.role {
                GpuAttachmentRole::Color { index } => {
                    let Some(bit) = 1_u32.checked_shl(u32::from(index)) else {
                        return Err(
                            GpuFramebufferValidationError::ColorAttachmentIndexOutOfRange(index),
                        );
                    };
                    if (color_mask & bit) != 0 {
                        return Err(
                            GpuFramebufferValidationError::DuplicateColorAttachmentIndex(index),
                        );
                    }
                    color_mask |= bit;
                }
                GpuAttachmentRole::Depth => {
                    if seen_depth {
                        return Err(GpuFramebufferValidationError::DuplicateDepthAttachment);
                    }
                    seen_depth = true;
                }
                GpuAttachmentRole::Stencil => {
                    if seen_stencil {
                        return Err(GpuFramebufferValidationError::DuplicateStencilAttachment);
                    }
                    seen_stencil = true;
                }
                GpuAttachmentRole::DepthStencil => {
                    if seen_depth_stencil {
                        return Err(GpuFramebufferValidationError::DuplicateDepthStencilAttachment);
                    }
                    seen_depth_stencil = true;
                }
            }
        }

        let color_count = self.color_attachment_count();

        if self.extensions.blending.is_some() && color_count == 0 {
            return Err(GpuFramebufferValidationError::BlendingWithoutColorAttachment);
        }
        if self.extensions.depth_stencil.is_some() && !self.has_depth_or_stencil_attachment() {
            return Err(GpuFramebufferValidationError::DepthStencilExtensionWithoutAttachment);
        }
        if let Some(multisample) = self.extensions.multisample {
            if let Some(samples) = self.attachment_samples() {
                if !multisample.max_samples.supports(samples) {
                    return Err(
                        GpuFramebufferValidationError::MultisampleExtensionBelowAttachmentSampleCount,
                    );
                }
            }
        }

        Ok(())
    }

    /// Validates one raster/mesh/ray-trace draw pipeline against this framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest compatibility error when the pipeline and framebuffer do not agree.
    pub fn validate_draw_pipeline(
        &self,
        pipeline: &GpuDrawPipeline<'_>,
    ) -> Result<(), GpuFramebufferCompatibilityError> {
        self.validate()
            .map_err(GpuFramebufferCompatibilityError::Structural)?;

        match pipeline.kernel {
            PcuRenderKernel::Raster(_) => {
                if self.extensions.rasterizer.is_none() {
                    return Err(GpuFramebufferCompatibilityError::MissingRasterizerExtension);
                }
                if self.attachments.is_empty() {
                    return Err(GpuFramebufferCompatibilityError::MissingColorAttachment);
                }
            }
            PcuRenderKernel::Mesh(_) => {
                let Some(rasterizer) = self.extensions.rasterizer else {
                    return Err(GpuFramebufferCompatibilityError::MissingRasterizerExtension);
                };
                if !rasterizer.mesh_shading {
                    return Err(GpuFramebufferCompatibilityError::MeshShadingUnsupported);
                }
            }
            PcuRenderKernel::RayTrace(_) => {
                if self.extensions.ray_trace.is_none() {
                    return Err(GpuFramebufferCompatibilityError::MissingRayTraceExtension);
                }
            }
        }

        if pipeline.depth_stencil.is_some() {
            if self.extensions.depth_stencil.is_none() {
                return Err(GpuFramebufferCompatibilityError::MissingDepthStencilExtension);
            }
            if !self.has_depth_or_stencil_attachment() {
                return Err(GpuFramebufferCompatibilityError::MissingDepthStencilAttachment);
            }
        }

        if !pipeline.blend_attachments.is_empty() {
            let Some(blending) = self.extensions.blending else {
                return Err(GpuFramebufferCompatibilityError::BlendStateWithoutExtension);
            };
            let available = self.color_attachment_count();
            let requested = u8::try_from(pipeline.blend_attachments.len()).unwrap_or(u8::MAX);
            if requested > available || requested > blending.max_color_attachments {
                return Err(
                    GpuFramebufferCompatibilityError::BlendAttachmentCountExceeded {
                        requested,
                        available,
                        max_supported: blending.max_color_attachments,
                    },
                );
            }
            if !blending.independent_blend && requested > 1 {
                let first = pipeline.blend_attachments[0];
                if pipeline
                    .blend_attachments
                    .iter()
                    .copied()
                    .skip(1)
                    .any(|attachment| attachment != first)
                {
                    return Err(GpuFramebufferCompatibilityError::IndependentBlendUnsupported);
                }
            }
        }

        let Some(attachment_samples) = self.attachment_samples() else {
            return Err(GpuFramebufferCompatibilityError::MissingColorAttachment);
        };
        if pipeline.multisample.samples != attachment_samples {
            return Err(GpuFramebufferCompatibilityError::MultisampleStateMismatch);
        }
        if pipeline.multisample.sample_shading_enable {
            let Some(multisample) = self.extensions.multisample else {
                return Err(GpuFramebufferCompatibilityError::MultisampleExtensionMissing);
            };
            if !multisample.sample_shading {
                return Err(GpuFramebufferCompatibilityError::SampleShadingUnsupported);
            }
        }
        if pipeline.multisample.alpha_to_coverage_enable {
            let Some(multisample) = self.extensions.multisample else {
                return Err(GpuFramebufferCompatibilityError::MultisampleExtensionMissing);
            };
            if !multisample.alpha_to_coverage {
                return Err(GpuFramebufferCompatibilityError::AlphaToCoverageUnsupported);
            }
        }

        Ok(())
    }

    /// Validates one compute-fill operation against this framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest compatibility error when the fill targets do not exist.
    pub fn validate_fill_operation(
        &self,
        operation: &GpuFillOperation<'_>,
    ) -> Result<(), GpuFramebufferCompatibilityError> {
        self.validate()
            .map_err(GpuFramebufferCompatibilityError::Structural)?;
        let _ = operation.kernel;
        if operation.color_attachments.is_empty() {
            return Err(GpuFramebufferCompatibilityError::FillTargetsEmpty);
        }
        for index in operation.color_attachments.iter().copied() {
            if self.color_attachment(index).is_none() {
                return Err(GpuFramebufferCompatibilityError::FillTargetMissing(index));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn color_attachment(&self, index: u8) -> Option<GpuFramebufferAttachment<'a>> {
        self.attachments.iter().copied().find(|attachment| {
            matches!(attachment.role, GpuAttachmentRole::Color { index: candidate } if candidate == index)
        })
    }

    #[must_use]
    pub fn depth_attachment(&self) -> Option<GpuFramebufferAttachment<'a>> {
        self.attachments
            .iter()
            .copied()
            .find(|attachment| matches!(attachment.role, GpuAttachmentRole::Depth))
    }

    #[must_use]
    pub fn stencil_attachment(&self) -> Option<GpuFramebufferAttachment<'a>> {
        self.attachments
            .iter()
            .copied()
            .find(|attachment| matches!(attachment.role, GpuAttachmentRole::Stencil))
    }

    #[must_use]
    pub fn depth_stencil_attachment(&self) -> Option<GpuFramebufferAttachment<'a>> {
        self.attachments
            .iter()
            .copied()
            .find(|attachment| matches!(attachment.role, GpuAttachmentRole::DepthStencil))
    }

    #[must_use]
    pub fn color_attachment_count(&self) -> u8 {
        let count = self
            .attachments
            .iter()
            .filter(|attachment| matches!(attachment.role, GpuAttachmentRole::Color { .. }))
            .count();
        u8::try_from(count).unwrap_or(u8::MAX)
    }

    #[must_use]
    pub fn has_depth_or_stencil_attachment(&self) -> bool {
        self.attachments.iter().any(|attachment| {
            matches!(
                attachment.role,
                GpuAttachmentRole::Depth
                    | GpuAttachmentRole::Stencil
                    | GpuAttachmentRole::DepthStencil
            )
        })
    }

    #[must_use]
    pub fn attachment_samples(&self) -> Option<GpuSampleCount> {
        self.attachments
            .first()
            .map(|attachment| attachment.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GpuFramebuffer,
        GpuFramebufferAttachment,
        GpuFramebufferValidationError,
    };
    use crate::{
        GpuAttachmentRole,
        GpuBlendAttachmentState,
        GpuBlendFactor,
        GpuBlendingExtension,
        GpuBlendOp,
        GpuColorWriteMask,
        GpuCompareOp,
        GpuCullMode,
        GpuDepthStencilState,
        GpuDepthStencilExtension,
        GpuDrawCall,
        GpuDrawPipeline,
        GpuExtent2D,
        GpuFormat,
        GpuFramebufferCompatibilityError,
        GpuFramebufferExtensions,
        GpuFillOperation,
        GpuFrontFace,
        GpuMultisampleExtension,
        GpuMultisampleState,
        GpuPolygonMode,
        GpuPrimitiveTopology,
        GpuRasterizerExtension,
        GpuRasterizerState,
        GpuResourceHandle,
        GpuScissorRect,
        GpuScissorState,
        GpuSampleCount,
        GpuViewport,
        GpuViewportState,
    };
    use fusion_pcu::{
        PcuDispatchAluOp,
        PcuKernelId,
        PcuRasterFeatureCaps,
        PcuRasterKernelIr,
        PcuRenderKernel,
        PcuValueTypeCaps,
    };
    use fusion_pcu::model::PcuDispatchKernelBuilder;

    const EXTENT: GpuExtent2D = GpuExtent2D::new(1920, 1080);

    fn handle(raw: u64) -> GpuResourceHandle {
        GpuResourceHandle::new(
            core::num::NonZeroU64::new(raw).expect("test handle must be nonzero"),
        )
    }

    #[test]
    fn framebuffer_validates_matching_color_and_depth_attachments() {
        let attachments = [
            GpuFramebufferAttachment::new(
                Some("color0"),
                handle(1),
                GpuAttachmentRole::Color { index: 0 },
                GpuFormat::Rgba8Unorm,
                EXTENT,
                GpuSampleCount::One,
            ),
            GpuFramebufferAttachment::new(
                Some("depth"),
                handle(2),
                GpuAttachmentRole::Depth,
                GpuFormat::Depth32Float,
                EXTENT,
                GpuSampleCount::One,
            ),
        ];
        let framebuffer = GpuFramebuffer::new(Some("main"), EXTENT, &attachments);

        assert_eq!(framebuffer.validate(), Ok(()));
        assert!(framebuffer.color_attachment(0).is_some());
        assert!(framebuffer.depth_attachment().is_some());
    }

    #[test]
    fn framebuffer_rejects_duplicate_color_indices() {
        let attachments = [
            GpuFramebufferAttachment::new(
                Some("color0a"),
                handle(1),
                GpuAttachmentRole::Color { index: 0 },
                GpuFormat::Rgba8Unorm,
                EXTENT,
                GpuSampleCount::One,
            ),
            GpuFramebufferAttachment::new(
                Some("color0b"),
                handle(2),
                GpuAttachmentRole::Color { index: 0 },
                GpuFormat::Bgra8Unorm,
                EXTENT,
                GpuSampleCount::One,
            ),
        ];
        let framebuffer = GpuFramebuffer::new(Some("dup"), EXTENT, &attachments);

        assert_eq!(
            framebuffer.validate(),
            Err(GpuFramebufferValidationError::DuplicateColorAttachmentIndex(0))
        );
    }

    #[test]
    fn framebuffer_rejects_role_format_mismatch() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("bad-depth"),
            handle(2),
            GpuAttachmentRole::Depth,
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("bad"), EXTENT, &attachments);

        assert_eq!(
            framebuffer.validate(),
            Err(GpuFramebufferValidationError::RoleFormatMismatch)
        );
    }

    #[test]
    fn framebuffer_rejects_mismatched_attachment_extent() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color0"),
            handle(1),
            GpuAttachmentRole::Color { index: 0 },
            GpuFormat::Rgba8Unorm,
            GpuExtent2D::new(1280, 720),
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("size"), EXTENT, &attachments);

        assert_eq!(
            framebuffer.validate(),
            Err(GpuFramebufferValidationError::AttachmentExtentMismatch)
        );
    }

    #[test]
    fn framebuffer_rejects_out_of_range_color_index() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color33"),
            handle(1),
            GpuAttachmentRole::Color { index: 33 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("bad-index"), EXTENT, &attachments);

        assert_eq!(
            framebuffer.validate(),
            Err(GpuFramebufferValidationError::ColorAttachmentIndexOutOfRange(33))
        );
    }

    #[test]
    fn framebuffer_validates_raster_draw_pipeline_when_extensions_match() {
        let attachments = [
            GpuFramebufferAttachment::new(
                Some("color0"),
                handle(1),
                GpuAttachmentRole::Color { index: 0 },
                GpuFormat::Rgba8Unorm,
                EXTENT,
                GpuSampleCount::Four,
            ),
            GpuFramebufferAttachment::new(
                Some("depth"),
                handle(2),
                GpuAttachmentRole::Depth,
                GpuFormat::Depth32Float,
                EXTENT,
                GpuSampleCount::Four,
            ),
        ];
        let framebuffer = GpuFramebuffer::new(Some("main"), EXTENT, &attachments).with_extensions(
            GpuFramebufferExtensions {
                rasterizer: Some(GpuRasterizerExtension {
                    mesh_shading: false,
                    wireframe: true,
                }),
                blending: Some(GpuBlendingExtension {
                    max_color_attachments: 1,
                    independent_blend: false,
                }),
                depth_stencil: Some(GpuDepthStencilExtension {
                    depth_test: true,
                    stencil_test: false,
                }),
                multisample: Some(GpuMultisampleExtension {
                    max_samples: GpuSampleCount::Four,
                    sample_shading: true,
                    alpha_to_coverage: true,
                }),
                ray_trace: None,
            },
        );
        let kernel = PcuRenderKernel::Raster(PcuRasterKernelIr {
            id: PcuKernelId(7),
            entry_point: "raster",
            bindings: &[],
            ports: &[],
            parameters: &[],
            vertex_entry: "vs_main",
            fragment_entry: Some("fs_main"),
            type_caps: PcuValueTypeCaps::FLOAT32 | PcuValueTypeCaps::VECTOR_VALUES,
            features: PcuRasterFeatureCaps::VERTEX_STAGE
                .union(PcuRasterFeatureCaps::FRAGMENT_STAGE),
        });
        let blend = [GpuBlendAttachmentState {
            blend_enable: true,
            src_color_factor: GpuBlendFactor::One,
            dst_color_factor: GpuBlendFactor::Zero,
            color_op: GpuBlendOp::Add,
            src_alpha_factor: GpuBlendFactor::One,
            dst_alpha_factor: GpuBlendFactor::Zero,
            alpha_op: GpuBlendOp::Add,
            color_write_mask: GpuColorWriteMask::all(),
        }];
        let pipeline = GpuDrawPipeline {
            kernel: &kernel,
            topology: GpuPrimitiveTopology::Triangles,
            rasterizer: GpuRasterizerState {
                cull_mode: GpuCullMode::Back,
                front_face: GpuFrontFace::CounterClockwise,
                polygon_mode: GpuPolygonMode::Fill,
                depth_bias: None,
            },
            depth_stencil: Some(GpuDepthStencilState {
                depth_test_enable: true,
                depth_write_enable: true,
                depth_compare_op: GpuCompareOp::LessOrEqual,
                stencil_test_enable: false,
                front: super::super::pipeline::GpuStencilFaceState {
                    fail_op: super::super::pipeline::GpuStencilOp::Keep,
                    pass_op: super::super::pipeline::GpuStencilOp::Keep,
                    depth_fail_op: super::super::pipeline::GpuStencilOp::Keep,
                    compare_op: GpuCompareOp::Always,
                },
                back: super::super::pipeline::GpuStencilFaceState {
                    fail_op: super::super::pipeline::GpuStencilOp::Keep,
                    pass_op: super::super::pipeline::GpuStencilOp::Keep,
                    depth_fail_op: super::super::pipeline::GpuStencilOp::Keep,
                    compare_op: GpuCompareOp::Always,
                },
            }),
            blend_attachments: &blend,
            viewport: GpuViewportState::Static(GpuViewport {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                min_depth: 0.0,
                max_depth: 1.0,
            }),
            scissor: GpuScissorState::Static(GpuScissorRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }),
            multisample: GpuMultisampleState {
                samples: GpuSampleCount::Four,
                sample_shading_enable: true,
                alpha_to_coverage_enable: true,
            },
        };

        assert_eq!(framebuffer.validate_draw_pipeline(&pipeline), Ok(()));
        let _ = GpuDrawCall::Direct {
            vertex_count: 3,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
        };
    }

    #[test]
    fn framebuffer_rejects_draw_pipeline_without_rasterizer_extension() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color0"),
            handle(1),
            GpuAttachmentRole::Color { index: 0 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("main"), EXTENT, &attachments);
        let kernel = PcuRenderKernel::Raster(PcuRasterKernelIr {
            id: PcuKernelId(8),
            entry_point: "raster",
            bindings: &[],
            ports: &[],
            parameters: &[],
            vertex_entry: "vs_main",
            fragment_entry: Some("fs_main"),
            type_caps: PcuValueTypeCaps::FLOAT32 | PcuValueTypeCaps::VECTOR_VALUES,
            features: PcuRasterFeatureCaps::VERTEX_STAGE
                .union(PcuRasterFeatureCaps::FRAGMENT_STAGE),
        });
        let pipeline = GpuDrawPipeline {
            kernel: &kernel,
            topology: GpuPrimitiveTopology::Triangles,
            rasterizer: GpuRasterizerState {
                cull_mode: GpuCullMode::None,
                front_face: GpuFrontFace::CounterClockwise,
                polygon_mode: GpuPolygonMode::Fill,
                depth_bias: None,
            },
            depth_stencil: None,
            blend_attachments: &[],
            viewport: GpuViewportState::Dynamic,
            scissor: GpuScissorState::Dynamic,
            multisample: GpuMultisampleState {
                samples: GpuSampleCount::One,
                sample_shading_enable: false,
                alpha_to_coverage_enable: false,
            },
        };

        assert_eq!(
            framebuffer.validate_draw_pipeline(&pipeline),
            Err(GpuFramebufferCompatibilityError::MissingRasterizerExtension)
        );
    }

    #[test]
    fn framebuffer_validates_fill_operation_against_color_targets() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color0"),
            handle(1),
            GpuAttachmentRole::Color { index: 0 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("main"), EXTENT, &attachments);
        let builder = PcuDispatchKernelBuilder::<1>::new(9, "fill", [1, 1, 1])
            .with_type_caps(PcuValueTypeCaps::UINT32 | PcuValueTypeCaps::SCALAR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test dispatch builder should accept one op");
        let kernel = builder.ir();
        let fill = GpuFillOperation {
            kernel: &kernel,
            color_attachments: &[0],
        };

        assert_eq!(framebuffer.validate_fill_operation(&fill), Ok(()));
    }

    #[test]
    fn framebuffer_rejects_fill_operation_for_missing_color_target() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color0"),
            handle(1),
            GpuAttachmentRole::Color { index: 0 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("main"), EXTENT, &attachments);
        let builder = PcuDispatchKernelBuilder::<1>::new(10, "fill", [1, 1, 1])
            .with_type_caps(PcuValueTypeCaps::UINT32 | PcuValueTypeCaps::SCALAR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test dispatch builder should accept one op");
        let kernel = builder.ir();
        let fill = GpuFillOperation {
            kernel: &kernel,
            color_attachments: &[1],
        };

        assert_eq!(
            framebuffer.validate_fill_operation(&fill),
            Err(GpuFramebufferCompatibilityError::FillTargetMissing(1))
        );
    }
}
