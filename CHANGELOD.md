## [0.5.0] - 2026-03-12

### Breaking changes
* The signature of `InputFormat` and `OutputFormat` was changed:
  * Methods on `InputFormat` and `OutputFormat` now return a custom `type Error`. `anyhow` was removed from the core/format API but remains to exist in the CLI tool.
  * Public facing methods/types from `voxquant_core` will no longer use `image::Rgba` and `glam` types - instead they will use raw array types. Both crates remain to be a dependency. Both crates are still on versions `0.---` so this could have potentially introduced friction. `image::RgbaImage` is unavoidable but is reexported together with the whole `image` crate (within `voxquant_core`)
  * Format implementations now must implement `InputFormat::read`/`OutputFormat::voxelize_and_write`. The old methods `InputFormat::load`/`InputFormat::voxelize_and_save` have default implementations that have the same signature except the error return type.

### Added
* Added the trait `SceneReader` and `SceneWriter`, which is used in the newly added `InputFormat`/`OutputFormat` methods. This allows full decoupling from the file system. A default implementation `LocalFile` is available (and used in the old methods as the default implementation).

  Apart from being `Read`/`Write` + `Seek` the traits have an additional method `base_path`. Some formats (like `.gltf`, but not `.glb`) may require loading additional files placed relative to the base path. They may return errors if no `base_path` is provided.

* `clap` is now just an optional dependency (for the core crate and format crates)