//! GPU pipeline/composition vocabulary.

use fusion_pcu::{
    PcuDispatchKernelIr,
    PcuInvocationTarget,
};

use crate::GpuSampleCount;

/// Primitive assembly topology for one raster draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuPrimitiveTopology {
    Points,
    Lines,
    LineStrip,
    Triangles,
    TriangleStrip,
}

/// Face culling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuCullMode {
    None,
    Front,
    Back,
    FrontAndBack,
}

/// Front-face winding rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuFrontFace {
    CounterClockwise,
    Clockwise,
}

/// Polygon rasterization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuPolygonMode {
    Fill,
    Line,
    Point,
}

/// Optional depth-bias state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuDepthBias {
    pub constant_factor: f32,
    pub clamp: f32,
    pub slope_factor: f32,
}

/// Rasterizer state for one raster or mesh draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuRasterizerState {
    pub cull_mode: GpuCullMode,
    pub front_face: GpuFrontFace,
    pub polygon_mode: GpuPolygonMode,
    pub depth_bias: Option<GpuDepthBias>,
}

/// Depth/stencil comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuCompareOp {
    Never,
    Less,
    LessOrEqual,
    Equal,
    GreaterOrEqual,
    Greater,
    NotEqual,
    Always,
}

/// Stencil update operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuStencilOp {
    Keep,
    Zero,
    Replace,
    IncrementClamp,
    DecrementClamp,
    Invert,
    IncrementWrap,
    DecrementWrap,
}

/// One stencil-face state record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuStencilFaceState {
    pub fail_op: GpuStencilOp,
    pub pass_op: GpuStencilOp,
    pub depth_fail_op: GpuStencilOp,
    pub compare_op: GpuCompareOp,
}

/// Combined depth/stencil state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuDepthStencilState {
    pub depth_test_enable: bool,
    pub depth_write_enable: bool,
    pub depth_compare_op: GpuCompareOp,
    pub stencil_test_enable: bool,
    pub front: GpuStencilFaceState,
    pub back: GpuStencilFaceState,
}

/// Blend factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuBlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
}

/// Blend equation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuBlendOp {
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

/// Color write-mask bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GpuColorWriteMask(u8);

impl GpuColorWriteMask {
    pub const RED: Self = Self(1 << 0);
    pub const GREEN: Self = Self(1 << 1);
    pub const BLUE: Self = Self(1 << 2);
    pub const ALPHA: Self = Self(1 << 3);

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn all() -> Self {
        Self(Self::RED.0 | Self::GREEN.0 | Self::BLUE.0 | Self::ALPHA.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// One blend attachment state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuBlendAttachmentState {
    pub blend_enable: bool,
    pub src_color_factor: GpuBlendFactor,
    pub dst_color_factor: GpuBlendFactor,
    pub color_op: GpuBlendOp,
    pub src_alpha_factor: GpuBlendFactor,
    pub dst_alpha_factor: GpuBlendFactor,
    pub alpha_op: GpuBlendOp,
    pub color_write_mask: GpuColorWriteMask,
}

/// Fixed viewport rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuViewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

/// Fixed scissor rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Viewport state selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuViewportState {
    Static(GpuViewport),
    Dynamic,
}

/// Scissor state selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuScissorState {
    Static(GpuScissorRect),
    Dynamic,
}

/// Multisample state for one raster or mesh draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuMultisampleState {
    pub samples: GpuSampleCount,
    pub sample_shading_enable: bool,
    pub alpha_to_coverage_enable: bool,
}

/// One raster-family pipeline description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuRasterPipeline<'a> {
    pub vertex_kernel: &'a PcuDispatchKernelIr<'a>,
    pub fragment_kernel: Option<&'a PcuDispatchKernelIr<'a>>,
    pub topology: GpuPrimitiveTopology,
    pub rasterizer: GpuRasterizerState,
    pub depth_stencil: Option<GpuDepthStencilState>,
    pub blend_attachments: &'a [GpuBlendAttachmentState],
    pub viewport: GpuViewportState,
    pub scissor: GpuScissorState,
    pub multisample: GpuMultisampleState,
}

/// One mesh-family pipeline description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuMeshPipeline<'a> {
    pub task_kernel: Option<&'a PcuDispatchKernelIr<'a>>,
    pub mesh_kernel: &'a PcuDispatchKernelIr<'a>,
    pub fragment_kernel: Option<&'a PcuDispatchKernelIr<'a>>,
    pub rasterizer: GpuRasterizerState,
    pub depth_stencil: Option<GpuDepthStencilState>,
    pub blend_attachments: &'a [GpuBlendAttachmentState],
    pub viewport: GpuViewportState,
    pub scissor: GpuScissorState,
    pub multisample: GpuMultisampleState,
}

/// One ray-trace-family pipeline description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuRayTracePipeline<'a> {
    pub raygen_kernel: &'a PcuDispatchKernelIr<'a>,
    pub miss_kernels: &'a [&'a PcuDispatchKernelIr<'a>],
    pub closest_hit_kernels: &'a [&'a PcuDispatchKernelIr<'a>],
    pub any_hit_kernels: &'a [&'a PcuDispatchKernelIr<'a>],
    pub callable_kernels: &'a [&'a PcuDispatchKernelIr<'a>],
}

/// Closed graphics-family pipeline payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuRenderPipeline<'a> {
    Raster(GpuRasterPipeline<'a>),
    Mesh(GpuMeshPipeline<'a>),
    RayTrace(GpuRayTracePipeline<'a>),
}

/// Direct or indexed raster draw-call geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuRasterDrawCall {
    Direct {
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    },
    Indexed {
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    },
}

/// Mesh-family dispatch geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuMeshDispatch {
    pub groups_x: u32,
    pub groups_y: u32,
    pub groups_z: u32,
}

impl GpuMeshDispatch {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.groups_x == 0 || self.groups_y == 0 || self.groups_z == 0
    }
}

/// Ray-trace dispatch extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuTraceExtent {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

impl GpuTraceExtent {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0 || self.depth == 0
    }
}

/// One explicit attachment-to-binding routing record for compute-fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuFillTargetBinding<'a> {
    pub color_attachment: u8,
    pub target: PcuInvocationTarget<'a>,
}

/// Compute-fill operation against one framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuFillOperation<'a> {
    pub kernel: &'a PcuDispatchKernelIr<'a>,
    pub targets: &'a [GpuFillTargetBinding<'a>],
}
