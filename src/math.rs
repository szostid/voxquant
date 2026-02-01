use crate::*;
use glam::{Vec3, Vec4};

#[inline]
#[must_use]
pub fn interpolate_color(colors: [Rgba<u8>; 3], bary: Vec3) -> Rgba<u8> {
    let c0 = Vec4::from_array(colors[0].0.map(|c| c as f32));
    let c1 = Vec4::from_array(colors[1].0.map(|c| c as f32));
    let c2 = Vec4::from_array(colors[2].0.map(|c| c as f32));

    let final_color = c0 * bary.x + c1 * bary.y + c2 * bary.z;

    Rgba([
        final_color.x as u8,
        final_color.y as u8,
        final_color.z as u8,
        final_color.w as u8,
    ])
}

#[inline]
#[must_use]
pub fn multiply_colors(c1: Rgba<u8>, c2: Rgba<u8>) -> Rgba<u8> {
    Rgba([
        ((c1[0] as u16 * c2[0] as u16) / 255) as u8,
        ((c1[1] as u16 * c2[1] as u16) / 255) as u8,
        ((c1[2] as u16 * c2[2] as u16) / 255) as u8,
        ((c1[3] as u16 * c2[3] as u16) / 255) as u8,
    ])
}
