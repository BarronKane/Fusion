//! Framebuffer composition vocabulary.

use crate::{
    GpuAttachmentRole,
    GpuBlendAttachmentState,
    GpuExtent2D,
    GpuFillOperation,
    GpuFormat,
    GpuFormatClass,
    GpuMeshPipeline,
    GpuRasterPipeline,
    GpuRayTracePipeline,
    GpuResourceHandle,
    GpuSampleCount,
};

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
    FillTargetIndexOutOfRange(u8),
    FillTargetMissing(u8),
    DuplicateFillTarget(u8),
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

        if self.extensions.blending.is_some() && self.color_attachment_count() == 0 {
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

    /// Validates one raster-family pipeline against this framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest compatibility error when the pipeline and framebuffer do not agree.
    pub fn validate_raster_pipeline(
        &self,
        pipeline: &GpuRasterPipeline<'_>,
    ) -> Result<(), GpuFramebufferCompatibilityError> {
        self.validate_draw_like_state(
            pipeline.rasterizer,
            pipeline.depth_stencil,
            pipeline.blend_attachments,
            pipeline.multisample,
            false,
        )
    }

    /// Validates one mesh-family pipeline against this framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest compatibility error when the pipeline and framebuffer do not agree.
    pub fn validate_mesh_pipeline(
        &self,
        pipeline: &GpuMeshPipeline<'_>,
    ) -> Result<(), GpuFramebufferCompatibilityError> {
        let Some(rasterizer) = self.extensions.rasterizer else {
            return Err(GpuFramebufferCompatibilityError::MissingRasterizerExtension);
        };
        if !rasterizer.mesh_shading {
            return Err(GpuFramebufferCompatibilityError::MeshShadingUnsupported);
        }

        self.validate_draw_like_state(
            pipeline.rasterizer,
            pipeline.depth_stencil,
            pipeline.blend_attachments,
            pipeline.multisample,
            true,
        )
    }

    /// Validates one ray-trace-family pipeline against this framebuffer.
    ///
    /// # Errors
    ///
    /// Returns one honest compatibility error when the pipeline and framebuffer do not agree.
    pub fn validate_ray_trace_pipeline(
        &self,
        _pipeline: &GpuRayTracePipeline<'_>,
    ) -> Result<(), GpuFramebufferCompatibilityError> {
        self.validate()
            .map_err(GpuFramebufferCompatibilityError::Structural)?;
        if self.extensions.ray_trace.is_none() {
            return Err(GpuFramebufferCompatibilityError::MissingRayTraceExtension);
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
        if operation.targets.is_empty() {
            return Err(GpuFramebufferCompatibilityError::FillTargetsEmpty);
        }

        let mut mask = 0_u32;
        for target in operation.targets.iter().copied() {
            let Some(bit) = 1_u32.checked_shl(u32::from(target.color_attachment)) else {
                return Err(GpuFramebufferCompatibilityError::FillTargetIndexOutOfRange(
                    target.color_attachment,
                ));
            };
            if (mask & bit) != 0 {
                return Err(GpuFramebufferCompatibilityError::DuplicateFillTarget(
                    target.color_attachment,
                ));
            }
            mask |= bit;

            if self.color_attachment(target.color_attachment).is_none() {
                return Err(GpuFramebufferCompatibilityError::FillTargetMissing(
                    target.color_attachment,
                ));
            }
        }

        Ok(())
    }

    fn validate_draw_like_state(
        &self,
        rasterizer: crate::GpuRasterizerState,
        depth_stencil: Option<crate::GpuDepthStencilState>,
        blend_attachments: &[GpuBlendAttachmentState],
        multisample: crate::GpuMultisampleState,
        mesh: bool,
    ) -> Result<(), GpuFramebufferCompatibilityError> {
        self.validate()
            .map_err(GpuFramebufferCompatibilityError::Structural)?;

        if self.extensions.rasterizer.is_none() {
            return Err(GpuFramebufferCompatibilityError::MissingRasterizerExtension);
        }
        if self.attachments.is_empty() {
            return Err(GpuFramebufferCompatibilityError::MissingColorAttachment);
        }
        if matches!(rasterizer.polygon_mode, crate::GpuPolygonMode::Line)
            && self.extensions.rasterizer.is_some_and(|ext| !ext.wireframe)
        {
            return Err(GpuFramebufferCompatibilityError::MissingRasterizerExtension);
        }
        if mesh && self.color_attachment_count() == 0 && !self.has_depth_or_stencil_attachment() {
            return Err(GpuFramebufferCompatibilityError::MissingColorAttachment);
        }

        if depth_stencil.is_some() {
            if self.extensions.depth_stencil.is_none() {
                return Err(GpuFramebufferCompatibilityError::MissingDepthStencilExtension);
            }
            if !self.has_depth_or_stencil_attachment() {
                return Err(GpuFramebufferCompatibilityError::MissingDepthStencilAttachment);
            }
        }

        if !blend_attachments.is_empty() {
            let Some(blending) = self.extensions.blending else {
                return Err(GpuFramebufferCompatibilityError::BlendStateWithoutExtension);
            };
            let available = self.color_attachment_count();
            let requested = u8::try_from(blend_attachments.len()).unwrap_or(u8::MAX);
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
                let first = blend_attachments[0];
                if blend_attachments
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
        if multisample.samples != attachment_samples {
            return Err(GpuFramebufferCompatibilityError::MultisampleStateMismatch);
        }
        if multisample.sample_shading_enable {
            let Some(multisample_extension) = self.extensions.multisample else {
                return Err(GpuFramebufferCompatibilityError::MultisampleExtensionMissing);
            };
            if !multisample_extension.sample_shading {
                return Err(GpuFramebufferCompatibilityError::SampleShadingUnsupported);
            }
        }
        if multisample.alpha_to_coverage_enable {
            let Some(multisample_extension) = self.extensions.multisample else {
                return Err(GpuFramebufferCompatibilityError::MultisampleExtensionMissing);
            };
            if !multisample_extension.alpha_to_coverage {
                return Err(GpuFramebufferCompatibilityError::AlphaToCoverageUnsupported);
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
        GpuFramebufferCompatibilityError,
        GpuFramebufferExtensions,
        GpuFramebufferValidationError,
        GpuMultisampleExtension,
        GpuRasterizerExtension,
    };
    use crate::{
        GpuAttachmentRole,
        GpuBlendAttachmentState,
        GpuBlendFactor,
        GpuBlendOp,
        GpuColorWriteMask,
        GpuExtent2D,
        GpuCullMode,
        GpuDepthStencilState,
        GpuFillOperation,
        GpuFillTargetBinding,
        GpuFormat,
        GpuFrontFace,
        GpuMeshPipeline,
        GpuMultisampleState,
        GpuPolygonMode,
        GpuPrimitiveTopology,
        GpuRasterPipeline,
        GpuRasterizerState,
        GpuResourceHandle,
        GpuSampleCount,
        GpuViewport,
        GpuViewportState,
        GpuScissorRect,
        GpuScissorState,
    };
    use fusion_pcu::{
        PcuDispatchAluOp,
        PcuDispatchKernelIr,
        PcuInvocationTarget,
        PcuValueTypeCaps,
    };
    use fusion_pcu::model::PcuDispatchKernelBuilder;
    use std::boxed::Box;

    const EXTENT: GpuExtent2D = GpuExtent2D::new(1920, 1080);
    const BLEND_ATTACHMENTS: [GpuBlendAttachmentState; 1] = [GpuBlendAttachmentState {
        blend_enable: true,
        src_color_factor: GpuBlendFactor::One,
        dst_color_factor: GpuBlendFactor::Zero,
        color_op: GpuBlendOp::Add,
        src_alpha_factor: GpuBlendFactor::One,
        dst_alpha_factor: GpuBlendFactor::Zero,
        alpha_op: GpuBlendOp::Add,
        color_write_mask: GpuColorWriteMask::all(),
    }];

    fn handle(raw: u64) -> GpuResourceHandle {
        GpuResourceHandle::new(
            core::num::NonZeroU64::new(raw).expect("test handle must be nonzero"),
        )
    }

    fn raster_pipeline<'a>(kernel: &'a PcuDispatchKernelIr<'a>) -> GpuRasterPipeline<'a> {
        GpuRasterPipeline {
            vertex_kernel: kernel,
            fragment_kernel: Some(kernel),
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
                depth_compare_op: crate::GpuCompareOp::Less,
                stencil_test_enable: false,
                front: crate::GpuStencilFaceState {
                    fail_op: crate::GpuStencilOp::Keep,
                    pass_op: crate::GpuStencilOp::Keep,
                    depth_fail_op: crate::GpuStencilOp::Keep,
                    compare_op: crate::GpuCompareOp::Always,
                },
                back: crate::GpuStencilFaceState {
                    fail_op: crate::GpuStencilOp::Keep,
                    pass_op: crate::GpuStencilOp::Keep,
                    depth_fail_op: crate::GpuStencilOp::Keep,
                    compare_op: crate::GpuCompareOp::Always,
                },
            }),
            blend_attachments: &BLEND_ATTACHMENTS,
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
        }
    }

    fn dispatch_kernel<'a>(id: u32, entry_point: &'static str) -> &'a PcuDispatchKernelIr<'a> {
        let builder = PcuDispatchKernelBuilder::<1>::new(id, entry_point, [1, 1, 1])
            .with_type_caps(PcuValueTypeCaps::FLOAT32 | PcuValueTypeCaps::VECTOR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test builder should accept one arithmetic op");
        let builder = Box::leak(Box::new(builder));
        Box::leak(Box::new(builder.ir()))
    }

    fn mesh_pipeline<'a>(kernel: &'a PcuDispatchKernelIr<'a>) -> GpuMeshPipeline<'a> {
        GpuMeshPipeline {
            task_kernel: None,
            mesh_kernel: kernel,
            fragment_kernel: Some(kernel),
            rasterizer: GpuRasterizerState {
                cull_mode: GpuCullMode::Back,
                front_face: GpuFrontFace::CounterClockwise,
                polygon_mode: GpuPolygonMode::Fill,
                depth_bias: None,
            },
            depth_stencil: None,
            blend_attachments: &[],
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
                samples: GpuSampleCount::One,
                sample_shading_enable: false,
                alpha_to_coverage_enable: false,
            },
        }
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
                    mesh_shading: true,
                    wireframe: true,
                }),
                blending: Some(crate::GpuBlendingExtension {
                    max_color_attachments: 1,
                    independent_blend: false,
                }),
                depth_stencil: Some(crate::GpuDepthStencilExtension {
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

        let kernel = dispatch_kernel(7, "raster");
        let pipeline = raster_pipeline(kernel);

        assert_eq!(framebuffer.validate(), Ok(()));
        assert_eq!(framebuffer.validate_raster_pipeline(&pipeline), Ok(()));
    }

    #[test]
    fn framebuffer_rejects_duplicate_color_indices() {
        let attachments = [
            GpuFramebufferAttachment::new(
                Some("color0-a"),
                handle(1),
                GpuAttachmentRole::Color { index: 0 },
                GpuFormat::Rgba8Unorm,
                EXTENT,
                GpuSampleCount::One,
            ),
            GpuFramebufferAttachment::new(
                Some("color0-b"),
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
    fn framebuffer_rejects_mismatched_attachment_extent() {
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
                GpuExtent2D::new(1280, 720),
                GpuSampleCount::One,
            ),
        ];

        let framebuffer = GpuFramebuffer::new(Some("bad"), EXTENT, &attachments);

        assert_eq!(
            framebuffer.validate(),
            Err(GpuFramebufferValidationError::AttachmentExtentMismatch)
        );
    }

    #[test]
    fn framebuffer_rejects_role_format_mismatch() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("depth"),
            handle(1),
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
    fn framebuffer_rejects_out_of_range_color_index() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color-big"),
            handle(1),
            GpuAttachmentRole::Color { index: 32 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];

        let framebuffer = GpuFramebuffer::new(Some("bad"), EXTENT, &attachments);

        assert_eq!(
            framebuffer.validate(),
            Err(GpuFramebufferValidationError::ColorAttachmentIndexOutOfRange(32))
        );
    }

    #[test]
    fn framebuffer_rejects_mesh_pipeline_without_mesh_extension() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color0"),
            handle(1),
            GpuAttachmentRole::Color { index: 0 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("main"), EXTENT, &attachments).with_extensions(
            GpuFramebufferExtensions {
                rasterizer: Some(GpuRasterizerExtension {
                    mesh_shading: false,
                    wireframe: true,
                }),
                blending: None,
                depth_stencil: None,
                multisample: None,
                ray_trace: None,
            },
        );
        let pipeline = mesh_pipeline(dispatch_kernel(9, "mesh"));

        assert_eq!(
            framebuffer.validate_mesh_pipeline(&pipeline),
            Err(GpuFramebufferCompatibilityError::MeshShadingUnsupported)
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
        let framebuffer = GpuFramebuffer::new(Some("fill"), EXTENT, &attachments);
        let builder = PcuDispatchKernelBuilder::<1>::new(10, "fill", [8, 1, 1])
            .with_type_caps(PcuValueTypeCaps::UINT32 | PcuValueTypeCaps::SCALAR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test dispatch builder should accept one arithmetic op");
        let kernel = builder.ir();
        let fill = GpuFillOperation {
            kernel: &kernel,
            targets: &[GpuFillTargetBinding {
                color_attachment: 0,
                target: PcuInvocationTarget::Binding(fusion_pcu::PcuBindingRef {
                    set: 0,
                    binding: 0,
                }),
            }],
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
        let framebuffer = GpuFramebuffer::new(Some("fill"), EXTENT, &attachments);
        let builder = PcuDispatchKernelBuilder::<1>::new(11, "fill", [8, 1, 1])
            .with_type_caps(PcuValueTypeCaps::UINT32 | PcuValueTypeCaps::SCALAR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test dispatch builder should accept one arithmetic op");
        let kernel = builder.ir();
        let fill = GpuFillOperation {
            kernel: &kernel,
            targets: &[GpuFillTargetBinding {
                color_attachment: 3,
                target: PcuInvocationTarget::Port("image"),
            }],
        };

        assert_eq!(
            framebuffer.validate_fill_operation(&fill),
            Err(GpuFramebufferCompatibilityError::FillTargetMissing(3))
        );
    }

    #[test]
    fn framebuffer_rejects_fill_operation_for_out_of_range_color_target_index() {
        let attachments = [GpuFramebufferAttachment::new(
            Some("color0"),
            handle(1),
            GpuAttachmentRole::Color { index: 0 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )];
        let framebuffer = GpuFramebuffer::new(Some("fill"), EXTENT, &attachments);
        let builder = PcuDispatchKernelBuilder::<1>::new(12, "fill", [8, 1, 1])
            .with_type_caps(PcuValueTypeCaps::UINT32 | PcuValueTypeCaps::SCALAR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test dispatch builder should accept one arithmetic op");
        let kernel = builder.ir();
        let fill = GpuFillOperation {
            kernel: &kernel,
            targets: &[GpuFillTargetBinding {
                color_attachment: 32,
                target: PcuInvocationTarget::Port("image"),
            }],
        };

        assert_eq!(
            framebuffer.validate_fill_operation(&fill),
            Err(GpuFramebufferCompatibilityError::FillTargetIndexOutOfRange(
                32
            ))
        );
    }
}
