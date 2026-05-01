//! GPU submission composition vocabulary.

use crate::{
    GpuFillOperation,
    GpuFramebuffer,
    GpuFramebufferCompatibilityError,
    GpuMeshDispatch,
    GpuMeshPipeline,
    GpuRasterDrawCall,
    GpuRasterPipeline,
    GpuRayTracePipeline,
    GpuScissorRect,
    GpuTraceExtent,
    GpuViewport,
};
use fusion_pcu::{
    PcuDispatchSubmission,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuInvocationShape,
};

/// Dynamic draw-state payload carried by one raster or mesh submission.
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

/// Submission-time validation failure for one GPU request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuSubmissionError {
    FramebufferCompatibility(GpuFramebufferCompatibilityError),
    MissingDynamicViewport,
    UnexpectedDynamicViewport,
    MissingDynamicScissor,
    UnexpectedDynamicScissor,
    ZeroVertexCount,
    ZeroIndexCount,
    ZeroInstanceCount,
    ZeroMeshDispatch,
    ZeroTraceExtent,
}

/// One validated raster draw submission.
#[derive(Debug, Clone, Copy)]
pub struct GpuRasterSubmission<'a> {
    pub framebuffer: &'a GpuFramebuffer<'a>,
    pub pipeline: &'a GpuRasterPipeline<'a>,
    pub draw_call: GpuRasterDrawCall,
    pub dynamic_state: GpuDynamicDrawState,
    pub bindings: PcuInvocationBindings<'a>,
    pub parameters: PcuInvocationParameters<'a>,
}

impl<'a> GpuRasterSubmission<'a> {
    /// Validates this raster submission against the framebuffer and pipeline it references.
    ///
    /// # Errors
    ///
    /// Returns one honest submission-shape or framebuffer-compatibility failure.
    pub fn validate(&self) -> Result<(), GpuSubmissionError> {
        self.framebuffer
            .validate_raster_pipeline(self.pipeline)
            .map_err(GpuSubmissionError::FramebufferCompatibility)?;
        validate_dynamic_state(
            self.pipeline.viewport,
            self.pipeline.scissor,
            self.dynamic_state,
        )?;

        match self.draw_call {
            GpuRasterDrawCall::Direct {
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
            GpuRasterDrawCall::Indexed {
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
}

/// One validated mesh draw submission.
#[derive(Debug, Clone, Copy)]
pub struct GpuMeshSubmission<'a> {
    pub framebuffer: &'a GpuFramebuffer<'a>,
    pub pipeline: &'a GpuMeshPipeline<'a>,
    pub dispatch: GpuMeshDispatch,
    pub dynamic_state: GpuDynamicDrawState,
    pub bindings: PcuInvocationBindings<'a>,
    pub parameters: PcuInvocationParameters<'a>,
}

impl<'a> GpuMeshSubmission<'a> {
    /// Validates this mesh submission against the framebuffer and pipeline it references.
    ///
    /// # Errors
    ///
    /// Returns one honest submission-shape or framebuffer-compatibility failure.
    pub fn validate(&self) -> Result<(), GpuSubmissionError> {
        self.framebuffer
            .validate_mesh_pipeline(self.pipeline)
            .map_err(GpuSubmissionError::FramebufferCompatibility)?;
        validate_dynamic_state(
            self.pipeline.viewport,
            self.pipeline.scissor,
            self.dynamic_state,
        )?;
        if self.dispatch.is_empty() {
            return Err(GpuSubmissionError::ZeroMeshDispatch);
        }
        Ok(())
    }
}

/// One validated ray-trace submission.
#[derive(Debug, Clone, Copy)]
pub struct GpuRayTraceSubmission<'a> {
    pub framebuffer: &'a GpuFramebuffer<'a>,
    pub pipeline: &'a GpuRayTracePipeline<'a>,
    pub trace_extent: GpuTraceExtent,
    pub bindings: PcuInvocationBindings<'a>,
    pub parameters: PcuInvocationParameters<'a>,
}

impl<'a> GpuRayTraceSubmission<'a> {
    /// Validates this ray-trace submission against the framebuffer and pipeline it references.
    ///
    /// # Errors
    ///
    /// Returns one honest submission-shape or framebuffer-compatibility failure.
    pub fn validate(&self) -> Result<(), GpuSubmissionError> {
        self.framebuffer
            .validate_ray_trace_pipeline(self.pipeline)
            .map_err(GpuSubmissionError::FramebufferCompatibility)?;
        if self.trace_extent.is_empty() {
            return Err(GpuSubmissionError::ZeroTraceExtent);
        }
        Ok(())
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
    pub fn pcu_submission(&self) -> Result<PcuDispatchSubmission<'a>, GpuSubmissionError> {
        self.validate()?;
        Ok(PcuDispatchSubmission {
            kernel: self.operation.kernel,
            shape: self.shape,
        })
    }
}

fn validate_dynamic_state(
    viewport_state: crate::GpuViewportState,
    scissor_state: crate::GpuScissorState,
    dynamic_state: GpuDynamicDrawState,
) -> Result<(), GpuSubmissionError> {
    match viewport_state {
        crate::GpuViewportState::Static(_) => {
            if dynamic_state.viewport.is_some() {
                return Err(GpuSubmissionError::UnexpectedDynamicViewport);
            }
        }
        crate::GpuViewportState::Dynamic => {
            if dynamic_state.viewport.is_none() {
                return Err(GpuSubmissionError::MissingDynamicViewport);
            }
        }
    }

    match scissor_state {
        crate::GpuScissorState::Static(_) => {
            if dynamic_state.scissor.is_some() {
                return Err(GpuSubmissionError::UnexpectedDynamicScissor);
            }
        }
        crate::GpuScissorState::Dynamic => {
            if dynamic_state.scissor.is_none() {
                return Err(GpuSubmissionError::MissingDynamicScissor);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        GpuDynamicDrawState,
        GpuFillSubmission,
        GpuMeshSubmission,
        GpuRasterSubmission,
        GpuRayTraceSubmission,
        GpuSubmissionError,
    };
    use crate::{
        GpuAttachmentRole,
        GpuExtent2D,
        GpuFillOperation,
        GpuFillTargetBinding,
        GpuFormat,
        GpuFramebuffer,
        GpuFramebufferAttachment,
        GpuFramebufferExtensions,
        GpuFrontFace,
        GpuMeshDispatch,
        GpuMeshPipeline,
        GpuMultisampleState,
        GpuPolygonMode,
        GpuPrimitiveTopology,
        GpuRasterDrawCall,
        GpuRasterPipeline,
        GpuRasterizerExtension,
        GpuRasterizerState,
        GpuRayTracePipeline,
        GpuResourceHandle,
        GpuScissorRect,
        GpuScissorState,
        GpuSampleCount,
        GpuTraceExtent,
        GpuViewport,
        GpuViewportState,
    };
    use core::num::{
        NonZeroU32,
        NonZeroU64,
    };
    use fusion_pcu::{
        PcuDispatchAluOp,
        PcuDispatchKernelIr,
        PcuInvocationBindings,
        PcuInvocationParameters,
        PcuInvocationShape,
        PcuInvocationTarget,
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
                ray_trace: Some(crate::GpuRayTraceExtension {
                    max_recursion_depth: 1,
                }),
            },
        )
    }

    fn dispatch_kernel<'a>(id: u32, entry_point: &'static str) -> &'a PcuDispatchKernelIr<'a> {
        let builder = PcuDispatchKernelBuilder::<1>::new(id, entry_point, [1, 1, 1])
            .with_type_caps(PcuValueTypeCaps::FLOAT32 | PcuValueTypeCaps::VECTOR_VALUES)
            .with_arithmetic_op(PcuDispatchAluOp::Add)
            .expect("test builder should accept one arithmetic op");
        let builder = Box::leak(Box::new(builder));
        Box::leak(Box::new(builder.ir()))
    }

    fn raster_pipeline<'a>() -> GpuRasterPipeline<'a> {
        let vertex_kernel = dispatch_kernel(4, "vertex");
        let fragment_kernel = dispatch_kernel(5, "fragment");
        GpuRasterPipeline {
            vertex_kernel,
            fragment_kernel: Some(fragment_kernel),
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

    fn mesh_pipeline<'a>() -> GpuMeshPipeline<'a> {
        let task_kernel = dispatch_kernel(6, "task");
        let mesh_kernel = dispatch_kernel(7, "mesh");
        let fragment_kernel = dispatch_kernel(8, "fragment");
        GpuMeshPipeline {
            task_kernel: Some(task_kernel),
            mesh_kernel,
            fragment_kernel: Some(fragment_kernel),
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

    fn ray_trace_pipeline<'a>() -> GpuRayTracePipeline<'a> {
        GpuRayTracePipeline {
            raygen_kernel: dispatch_kernel(9, "raygen"),
            miss_kernels: &[],
            closest_hit_kernels: &[],
            any_hit_kernels: &[],
            callable_kernels: &[],
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
            targets: &[GpuFillTargetBinding {
                color_attachment: 0,
                target: PcuInvocationTarget::Port("image"),
            }],
        }
    }

    #[test]
    fn raster_submission_requires_dynamic_state_payloads() {
        let framebuffer = framebuffer();
        let pipeline = raster_pipeline();
        let submission = GpuRasterSubmission {
            framebuffer: &framebuffer,
            pipeline: &pipeline,
            draw_call: GpuRasterDrawCall::Direct {
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
    fn raster_submission_validates_graphics_pipeline() {
        let framebuffer = framebuffer();
        let pipeline = raster_pipeline();
        let submission = GpuRasterSubmission {
            framebuffer: &framebuffer,
            pipeline: &pipeline,
            draw_call: GpuRasterDrawCall::Direct {
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

        assert_eq!(submission.validate(), Ok(()));
    }

    #[test]
    fn mesh_submission_validates_graphics_pipeline() {
        let framebuffer = framebuffer();
        let pipeline = mesh_pipeline();
        let submission = GpuMeshSubmission {
            framebuffer: &framebuffer,
            pipeline: &pipeline,
            dispatch: GpuMeshDispatch {
                groups_x: 1,
                groups_y: 1,
                groups_z: 1,
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

        assert_eq!(submission.validate(), Ok(()));
    }

    #[test]
    fn ray_trace_submission_validates_graphics_pipeline() {
        let framebuffer = framebuffer();
        let pipeline = ray_trace_pipeline();
        let submission = GpuRayTraceSubmission {
            framebuffer: &framebuffer,
            pipeline: &pipeline,
            trace_extent: GpuTraceExtent {
                width: 1280,
                height: 720,
                depth: 1,
            },
            bindings: PcuInvocationBindings::empty(),
            parameters: PcuInvocationParameters::empty(),
        };

        assert_eq!(submission.validate(), Ok(()));
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
            .pcu_submission()
            .expect("validated fill submission should lower");

        assert_eq!(lowered.kernel, operation.kernel);
        assert_eq!(lowered.shape, submission.shape);
    }
}
