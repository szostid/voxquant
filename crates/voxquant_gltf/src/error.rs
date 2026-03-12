/// Error during parsing and scene loading of a `glTF` file
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Returned anytime the `glTF` parsing fails. This is returned whenever
    /// a method from [`gltf`] returns a [`gltf::Error`]
    #[error("failed to load the file")]
    Gltf(#[from] gltf::Error),
    /// Required for the sake of [`voxquant_core::OutputFormat::Error`]
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Returned in [`crate::parse_mesh_instance`] if the mesh contains a
    /// primitive that does not have [`gltf::mesh::Reader::read_positions`]
    #[error("the mesh contains a primitive with no positions")]
    PrimitiveWithNoPositions,
    /// Returned in [`crate::convert_image`], if the [`gltf::image::Data`]'s
    /// `pixels` buffer does not have the correct size given its
    /// `width` and `height`
    #[error("loaded image has invalid dimensions given its data")]
    InvalidImageDimensions,
    /// Returned in [`crate::get_material_texture_data`] or [`crate::parse_image`]
    /// whenever a material/image contains an index to a texture data that is
    /// out of bounds
    #[error("the provided glTF file contains references to out-of-bounds assets")]
    OutOfBounds,
}
