#![no_std]

use spirv_std::spirv;
use spirv_std::glam::{vec2, Vec2, vec3, Vec3, vec4, Vec4};

#[spirv(fragment)]
pub fn main_fs(output: &mut Vec4) {
    *output = vec4(1.0, 0.0, 0.0, 1.0);
}
