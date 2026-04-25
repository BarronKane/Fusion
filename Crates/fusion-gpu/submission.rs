//! GPU submission composition vocabulary.
//!
//! This layer turns framebuffer/pipeline/work descriptions into one validated submission unit
//! before any backend-specific lowering happens.

use crate::{
    GpuDrawCall,
    GpuDrawPipeline,
    GpuFillOperation,
    GpuFramebuffer,
    GpuFramebufferCompatibilityError,
    GpuScissorRect,
    GpuViewport,
};
use fusion_pcu::{
    PcuDispatchSubmission,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuInvocationShape,
    PcuRenderKernel,
    PcuRenderSubmission,
};

/// Dynamic draw-state payload carried by one draw submission.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuDynamicDrawState {
    pub viewport: Option<GpuViewport>,
    pub scissor: Option<GpuScissorRect>,
}

impl GpuDynamicDrawState {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            viewport: None,
            scissor: None,
        }
    }
}

/// Submission-time validation failure for one GPU draw/fill request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuSubmissionError {
    FramebufferCompatibility(GpuFramebufferCompatibilityError),
    MissingDynamicViewport,
    UnexpectedDynamicViewport,
    MissingDynamicScissor,
    UnexpectedDynamicScissor,
    UnsupportedRenderKernelFamily,
    ZeroVertexCount,
    ZeroIndexCount,
    ZeroInstanceCount,
}

/// One validated draw-style render submission.
#[derive(Debug, Clone, Copy)]
pub struct GpuDrawSubmission<'a> {
    pub framebuffer: &'a GpuFramebuffer<'a>,
    pub pipeline: &'a GpuDrawPipeline<'a>,
    pub draw_call: GpuDrawCall,
    pub dynamic_state: GpuDynamicDrawState,
    pub bindings: PcuInvocationBindings<'a>,
    pub parameters: PcuInvocationParameters<'a>,
}

impl<'a> GpuDrawSubmission<'a> {
    /// Validates this draw submission against the framebuffer and pipeline it references.
    ///
    /// # Errors
    ///
    /// Returns one honest submission-shape or framebuffer-compatibility failure.
    pub fn validate(&self) -> Result<(), GpuSubmissionError> {
        self.framebuffer
            .validate_draw_pipeline(self.pipeline)
            .map_err(GpuSubmissionError::FramebufferCompatibility)?;

        match self.pipeline.kernel {
            PcuRenderKernel::Raster(_) | PcuRenderKernel::Mesh(_) => {}
            PcuRenderKernel::RayTrace(_) => {
                return Err(GpuSubmissionError::UnsupportedRenderKernelFamily);
            }
        }

        match self.pipeline.viewport {
            crate::GpuViewportState::Static(_) => {
                if self.dynamic_state.viewport.is_some() {
                    return Err(GpuSubmissionError::UnexpectedDynamicViewport);
                }
            }
            crate::GpuViewportState::Dynamic => {
                if self.dynamic_state.viewport.is_none() {
                    return Err(GpuSubmissionError::MissingDynamicViewport);
                }
            }
        }

        match self.pipeline.scissor {
            crate::GpuScissorState::Static(_) => {
                if self.dynamic_state.scissor.is_some() {
                    return Err(GpuSubmissionError::UnexpectedDynamicScissor);
                }
            }
            crate::GpuScissorState::Dynamic => {
                if self.dynamic_state.scissor.is_none() {
                    return Err(GpuSubmissionError::MissingDynamicScissor);
                }
            }
        }

        match self.draw_call {
            GpuDrawCall::Direct {
                vertex_count,
                instance_count,
                ..
            } => {
                if vertex_count == 0 {
                    return Err(GpuSubmissionError::ZeroVertexCount);
                }
                if instance_count == 0 {
                    return Err(GpuSubmissionError::ZeroInstanceCount);
                }
            }
            GpuDrawCall::Indexed {
                index_count,
                instance_count,
                ..
            } => {
                if index_count == 0 {
                    return Err(GpuSubmissionError::ZeroIndexCount);
                }
                if instance_count == 0 {
                    return Err(GpuSubmissionError::ZeroInstanceCount);
                }
            }
        }

        Ok(())
    }

    /// Lowers this validated draw submission into one backend-neutral PCU render submission.
    ///
    /// # Errors
    ///
    /// Returns one honest validation failure when this draw request is not admissible.
    pub fn pcu_render_submission(&self) -> Result<PcuRenderSubmission<'a>, GpuSubmissionError> {
        self.validate()?;
        Ok(PcuRenderSubmission {
            kernel: self.pipeline.kernel,
        })
    }
}

/// One validated compute-fill submission against a framebuffer.
#[derive(Debug, Clone, Copy)]
pub struct GpuFillSubmission<'a> {
    pub framebuffer: &'a GpuFramebuffer<'a>,
    pub operation: &'a GpuFillOperation<'a>,
    pub shape: PcuInvocationShape,
    pub bindings: PcuInvocationBindings<'a>,
    pub parameters: PcuInvocationParameters<'a>,
}

impl<'a> GpuFillSubmission<'a> {
    /// Validates this compute-fill submission against the framebuffer it targets.
    ///
    /// # Errors
    ///
    /// Returns one honest compatibility failure when the requested fill cannot target this
    /// framebuffer.
    pub fn validate(&self) -> Result<(), GpuSubmissionError> {
        self.framebuffer
            .validate_fill_operation(self.operation)
            .map_err(GpuSubmissionError::FramebufferCompatibility)
    }

    /// Lowers this validated fill submission into one backend-neutral PCU dispatch submission.
    ///
    /// # Errors
    ///
    /// Returns one honest validation failure when this fill request is not admissible.
    pub fn pcu_dispatch_submission(&self) -> Result<PcuDispatchSubmission<'a>, GpuSubmissionError> {
        self.validate()?;
        Ok(PcuDispatchSubmission {
            kernel: self.operation.kernel,
            shape: self.shape,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GpuDrawSubmission,
        GpuDynamicDrawState,
        GpuFillSubmission,
        GpuSubmissionError,
    };
    use crate::{
        GpuAttachmentRole,
        GpuDrawCall,
        GpuDrawPipeline,
        GpuExtent2D,
        GpuFillOperation,
        GpuFormat,
        GpuFramebuffer,
        GpuFramebufferAttachment,
        GpuFramebufferExtensions,
        GpuFrontFace,
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
    use core::num::{
        NonZeroU32,
        NonZeroU64,
    };
    use fusion_pcu::{
        PcuDispatchAluOp,
        PcuInvocationBindings,
        PcuInvocationParameters,
        PcuInvocationShape,
        PcuKernelId,
        PcuRasterFeatureCaps,
        PcuRasterKernelIr,
        PcuRenderKernel,
        PcuValueTypeCaps,
    };
    use fusion_pcu::model::PcuDispatchKernelBuilder;
    use std::boxed::Box;

    const EXTENT: GpuExtent2D = GpuExtent2D::new(1280, 720);

    fn handle(raw: u64) -> GpuResourceHandle {
        GpuResourceHandle::new(NonZeroU64::new(raw).expect("test handle must be nonzero"))
    }

    fn framebuffer<'a>() -> GpuFramebuffer<'a> {
        let attachments = Box::leak(Box::new([GpuFramebufferAttachment::new(
            Some("color0"),
            handle(1),
            GpuAttachmentRole::Color { index: 0 },
            GpuFormat::Rgba8Unorm,
            EXTENT,
            GpuSampleCount::One,
        )]));

        GpuFramebuffer::new(Some("main"), EXTENT, attachments).with_extensions(
            GpuFramebufferExtensions {
                rasterizer: Some(GpuRasterizerExtension {
                    mesh_shading: true,
                    wireframe: true,
                }),
                blending: None,
                depth_stencil: None,
                multisample: None,
                ray_trace: None,
            },
        )
    }

    fn raster_pipeline<'a>() -> GpuDrawPipeline<'a> {
        let kernel = Box::leak(Box::new(PcuRenderKernel::Raster(PcuRasterKernelIr {
            id: PcuKernelId(4),
            entry_point: "main",
            bindings: &[],
            ports: &[],
            parameters: &[],
            vertex_entry: "vs_main",
            fragment_entry: Some("fs_main"),
            type_caps: PcuValueTypeCaps::FLOAT32 | PcuValueTypeCaps::VECTOR_VALUES,
            features: PcuRasterFeatureCaps::VERTEX_STAGE
                .union(PcuRasterFeatureCaps::FRAGMENT_STAGE),
        })));

        GpuDrawPipeline {
            kernel,
            topology: GpuPrimitiveTopology::Triangles,
            rasterizer: GpuRasterizerState {
                cull_mode: crate::GpuCullMode::Back,
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
        }
    }

    fn fill_operation<'a>() -> GpuFillOperation<'a> {
        let builder = PcuDispatchKernelBuilder::<1>::new(8, "fill", [8, 1, 1])
            .with_type_caps(PcuValueTypeCaps::UINT32 | PcuValueTypeCaps::SCALAR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test builder should accept one arithmetic op");
        let builder = Box::leak(Box::new(builder));
        let kernel = Box::leak(Box::new(builder.ir()));

        GpuFillOperation {
            kernel,
            color_attachments: &[0],
        }
    }

    #[test]
    fn draw_submission_requires_dynamic_state_payloads() {
        let framebuffer = framebuffer();
        let pipeline = raster_pipeline();
        let submission = GpuDrawSubmission {
            framebuffer: &framebuffer,
            pipeline: &pipeline,
            draw_call: GpuDrawCall::Direct {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
            },
            dynamic_state: GpuDynamicDrawState::empty(),
            bindings: PcuInvocationBindings::empty(),
            parameters: PcuInvocationParameters::empty(),
        };

        assert_eq!(
            submission.validate(),
            Err(GpuSubmissionError::MissingDynamicViewport)
        );
    }

    #[test]
    fn draw_submission_lowers_to_pcu_render_submission() {
        let framebuffer = framebuffer();
        let pipeline = raster_pipeline();
        let submission = GpuDrawSubmission {
            framebuffer: &framebuffer,
            pipeline: &pipeline,
            draw_call: GpuDrawCall::Direct {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
            },
            dynamic_state: GpuDynamicDrawState {
                viewport: Some(GpuViewport {
                    x: 0.0,
                    y: 0.0,
                    width: 1280.0,
                    height: 720.0,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }),
                scissor: Some(GpuScissorRect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 720,
                }),
            },
            bindings: PcuInvocationBindings::empty(),
            parameters: PcuInvocationParameters::empty(),
        };

        let lowered = submission
            .pcu_render_submission()
            .expect("validated draw submission should lower");

        assert_eq!(lowered.kernel, pipeline.kernel);
    }

    #[test]
    fn draw_submission_rejects_zero_count_draws() {
        let framebuffer = framebuffer();
        let pipeline = raster_pipeline();
        let submission = GpuDrawSubmission {
            framebuffer: &framebuffer,
            pipeline: &pipeline,
            draw_call: GpuDrawCall::Indexed {
                index_count: 0,
                instance_count: 1,
                first_index: 0,
                vertex_offset: 0,
                first_instance: 0,
            },
            dynamic_state: GpuDynamicDrawState {
                viewport: Some(GpuViewport {
                    x: 0.0,
                    y: 0.0,
                    width: 1280.0,
                    height: 720.0,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }),
                scissor: Some(GpuScissorRect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 720,
                }),
            },
            bindings: PcuInvocationBindings::empty(),
            parameters: PcuInvocationParameters::empty(),
        };

        assert_eq!(
            submission.validate(),
            Err(GpuSubmissionError::ZeroIndexCount)
        );
    }

    #[test]
    fn fill_submission_lowers_to_pcu_dispatch_submission() {
        let framebuffer = framebuffer();
        let operation = fill_operation();
        let submission = GpuFillSubmission {
            framebuffer: &framebuffer,
            operation: &operation,
            shape: PcuInvocationShape::threads(
                NonZeroU32::new(64).expect("test shape must be nonzero"),
            ),
            bindings: PcuInvocationBindings::empty(),
            parameters: PcuInvocationParameters::empty(),
        };

        let lowered = submission
            .pcu_dispatch_submission()
            .expect("validated fill submission should lower");

        assert_eq!(lowered.kernel, operation.kernel);
        assert_eq!(lowered.shape, submission.shape);
    }
}
