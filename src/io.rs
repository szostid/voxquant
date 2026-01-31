use std::collections::HashMap;

use crate::octree::*;
use crate::*;
use bytemuck::Pod;
use bytemuck::Zeroable;

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct Vertex {
    pub position: Vec3,
    pub color: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct FloatVertex {
    pub position: Vec3,
    pub color: [f32; 4],
}

impl From<Vertex> for FloatVertex {
    fn from(value: Vertex) -> Self {
        let color = value.color.map(|c| c as f32 / 255.0);
        Self {
            position: value.position,
            color,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct VertexExtras {
    normal: Vec3,
    uv: Vec2,

    pub material_idx: u32,
}

impl VertexExtras {
    pub fn new(normal: Option<Vec3>, uv: Option<Vec2>, material_idx: u32) -> Self {
        Self {
            normal: normal.unwrap_or(Vec3::NAN),
            uv: uv.unwrap_or(Vec2::NAN),
            material_idx,
        }
    }

    #[inline]
    #[must_use]
    pub fn normal(&self) -> Option<Vec3> {
        (self.normal != Vec3::NAN).then_some(self.normal)
    }

    #[inline]
    #[must_use]
    pub fn uv(&self) -> Option<Vec2> {
        (self.uv != Vec2::NAN).then_some(self.uv)
    }
}

#[derive(Debug, Clone)]
pub enum ImageOrColor {
    Image(image::RgbaImage),
    Color(image::Rgba<u8>),
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub triangles: Vec<[Vec3; 3]>,
    pub triangle_extras: Vec<[VertexExtras; 3]>,
    pub materials: Vec<ImageOrColor>,

    pub bounds: BoundingBox,
    pub view: View,
}

#[derive(Debug, Clone)]
pub struct PerspectiveCamera {
    pub yfov: f32,
    pub znear: f32,

    pub zfar: Option<f32>,
    pub aspect_ratio: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct OrthographicCamera {
    pub xmag: f32,
    pub ymag: f32,
    pub zfar: f32,
    pub znear: f32,
}

impl PerspectiveCamera {
    pub fn new(value: &gltf::camera::Perspective<'_>) -> Self {
        Self {
            yfov: value.yfov(),
            znear: value.znear(),
            zfar: value.zfar(),
            aspect_ratio: value.aspect_ratio(),
        }
    }
}

impl OrthographicCamera {
    pub fn new(value: &gltf::camera::Orthographic<'_>) -> Self {
        Self {
            xmag: value.xmag(),
            ymag: value.ymag(),
            zfar: value.zfar(),
            znear: value.znear(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Camera {
    PerspectiveCamera(PerspectiveCamera),
    OrthographiCamera(OrthographicCamera),
}

impl Camera {
    pub fn new(cam: &gltf::camera::Projection<'_>) -> Self {
        match cam {
            gltf::camera::Projection::Orthographic(ort) => {
                Camera::OrthographiCamera(OrthographicCamera::new(ort))
            }
            gltf::camera::Projection::Perspective(per) => {
                Camera::PerspectiveCamera(PerspectiveCamera::new(per))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct View {
    pub camera: Option<Camera>,
    pub model_view_projection: Mat4,
}

mod magica {
    pub const fn encode(color: image::Rgba<u8>) -> u8 {
        let color = color.0;
        (color[0] >> 5) | ((color[1] >> 5) << 3) | ((color[2] >> 6) << 6)
    }

    pub const fn decode(byte: u8) -> image::Rgba<u8> {
        let mask3 = (1 << 3) - 1;
        let mask2 = (1 << 2) - 1;

        let r = (byte & mask3) << 5;
        let g = ((byte >> 3) & mask3) << 5;
        let b = ((byte >> 6) & mask2) << 6;

        image::Rgba([r, g, b, 255])
    }

    #[cfg(test)]
    pub const fn _gather() {
        let mut counter = 0;
        loop {
            if encode(decode(counter)) != counter {
                panic!()
            }
            if counter == u8::MAX {
                break;
            }
            counter += 1;
        }
    }

    #[cfg(test)]
    pub const _: () = _gather();
}

impl Octree {
    pub fn save_as_magica_voxel(&self, file_path: &str) -> Result<()> {
        use dot_vox::*;

        const CHUNK_SIZE: i32 = 256;

        let nodes = self.collect_nodes();

        let mut chunks = HashMap::<IVec3, Vec<dot_vox::Voxel>>::new();

        // the palette starts at index 1 and ends later because magicavoxel only allows for 254
        // indices and reserves the first index for a black color. we can therefore skip the black
        // color
        let mut palette = Vec::with_capacity(256);

        for index in 1..=255 {
            let color = magica::decode(index);
            palette.push(dot_vox::Color {
                r: color.0[0],
                g: color.0[1],
                b: color.0[2],
                a: 255,
            });
        }

        for (coords, color) in nodes {
            let color = octree_header::to_color(color);
            let color_idx = magica::encode(color);

            let chunk = coords.coords / CHUNK_SIZE;
            let local_coords = (coords.coords % CHUNK_SIZE).as_u8vec3();

            chunks.entry(chunk).or_default().push(dot_vox::Voxel {
                x: local_coords.x,
                y: local_coords.z,
                z: local_coords.y,
                // as said previously, the palette starts at index 1, and dot_vox
                // will offset this index by adding one to it. we want black indices
                // to be `0` after this operation, so they have to be `255` before
                // this operation, we can perform a wrapping subtraction to achieve that
                i: color_idx.wrapping_sub(1),
            });
        }

        let mut models = Vec::new();
        let mut nodes = Vec::new();

        nodes.push(SceneNode::Transform {
            attributes: Default::default(),
            frames: vec![Frame {
                attributes: Default::default(),
            }],
            child: 1,
            layer_id: 0,
        });

        nodes.push(SceneNode::Group {
            attributes: Default::default(),
            children: Vec::new(),
        });

        for (chunk, voxels) in chunks {
            let model_id = models.len() as u32;

            models.push(Model {
                size: Size {
                    x: CHUNK_SIZE as u32,
                    y: CHUNK_SIZE as u32,
                    z: CHUNK_SIZE as u32,
                },
                voxels,
            });

            let transform_index = nodes.len() as u32;
            let shape_index = transform_index + 1;

            nodes.push(SceneNode::Transform {
                attributes: Default::default(),
                frames: vec![Frame {
                    attributes: [(
                        "_t".to_string(),
                        format!(
                            "{} {} {}",
                            chunk.x * CHUNK_SIZE,
                            chunk.z * CHUNK_SIZE,
                            chunk.y * CHUNK_SIZE
                        ),
                    )]
                    .into(),
                }],
                child: shape_index,
                layer_id: 0,
            });

            nodes.push(SceneNode::Shape {
                attributes: Default::default(),
                models: vec![ShapeModel {
                    model_id,
                    attributes: Default::default(),
                }],
            });

            let SceneNode::Group { children, .. } = &mut nodes[1] else {
                unreachable!()
            };

            children.push(transform_index);
        }

        // Construct the scene
        let data = dot_vox::DotVoxData {
            version: 150,
            models,
            palette,
            materials: Vec::new(),
            layers: Vec::new(),
            scenes: nodes,
        };

        // Write the file
        let mut file = std::fs::File::create(file_path)?;

        data.write_vox(&mut file)?;

        Ok(())
    }
}
