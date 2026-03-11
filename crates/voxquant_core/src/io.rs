//! IO utilities for defining custom read and write types
//!
//! Instead of taking a `Path`, format implementations will take
//! either [`SceneReader`] or [`SceneWriter`] which can provide
//! the implementations with base paths, or not, if they are not
//! available, or aren't necessary for that exact format.
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read, Result, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// An input stream for reading scene data.
#[diagnostic::on_unimplemented(
    note = "`std::io::Read` is not enough to implement `SceneReader`",
    note = "if you want to use a file, use a dedicated method or `voxquant_core::io::LocalFile`"
)]
pub trait SceneReader: Read + Seek {
    /// The directory used to resolve external files.
    /// Return `None` if this reader is not backed by a
    /// filesystem.
    ///
    /// Implementations of scene loaders may expect you to provide
    /// a base path in your reader. For instance, `.gltf` will expect
    /// it, while `.glb` will not.
    fn base_path(&self) -> Option<&Path>;
}

#[diagnostic::do_not_recommend]
impl<R: SceneReader + ?Sized> SceneReader for &mut R {
    fn base_path(&self) -> Option<&Path> {
        (**self).base_path()
    }
}

impl<T> SceneReader for Cursor<T>
where
    Cursor<T>: Read + Seek,
{
    fn base_path(&self) -> Option<&Path> {
        None
    }
}

/// An output stream for writing scene data.
#[diagnostic::on_unimplemented(
    note = "`std::io::Write` is not enough to implement `SceneWriter`",
    note = "if you want to use a file, use a dedicated method or `voxquant_core::io::LocalFile`"
)]
pub trait SceneWriter: Write + Seek {
    /// The directory used to save external files (materials, textures, etc.).
    /// Return `None` if this writer is not backed by a filesystem.
    fn base_path(&self) -> Option<&Path>;
}

#[diagnostic::do_not_recommend]
impl<W: SceneWriter + ?Sized> SceneWriter for &mut W {
    fn base_path(&self) -> Option<&Path> {
        (**self).base_path()
    }
}

impl<T> SceneWriter for Cursor<T>
where
    Cursor<T>: Write + Seek,
{
    fn base_path(&self) -> Option<&Path> {
        None
    }
}

/// An [`std::fs`]-backed implementation of [`SceneReader`] and [`SceneWriter`].
///
/// This wraps a standard [`std::fs::File`] and automatically infers and
/// manages the base path for formats to use.
pub struct LocalFile {
    file: File,
    base_path: PathBuf,
}

impl LocalFile {
    /// Opens a file with custom options and infers its parent directory.
    pub fn new<P: AsRef<Path>>(path: P, options: &OpenOptions) -> Result<Self> {
        let file = options.open(path.as_ref())?;

        let base_path = path
            .as_ref()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        Ok(Self { file, base_path })
    }

    /// Opens a file in read-only mode, and infers its base directory
    ///
    /// Equivalent to [`std::fs::File::open`]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::new(path, OpenOptions::new().read(true))
    }

    /// Opens a file in write-only mode, and infers its base directory
    ///
    /// Creates the file if it does not exist, and truncates it if it does.
    ///
    /// Equivalent to [`std::fs::File::create`]
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::new(
            path,
            OpenOptions::new().write(true).create(true).truncate(true),
        )
    }

    /// Creates a new file in read-write mode, and infers its base directory.
    /// Fails if the file already exists.
    ///
    /// Equivalent to [`std::fs::File::create_new`]
    pub fn create_new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::new(
            path,
            OpenOptions::new().read(true).write(true).create_new(true),
        )
    }
}

impl Read for LocalFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.file.read(buf)
    }
}

impl Write for LocalFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        self.file.flush()
    }
}

impl Seek for LocalFile {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.file.seek(pos)
    }
}

impl SceneReader for LocalFile {
    fn base_path(&self) -> Option<&Path> {
        Some(&self.base_path)
    }
}

impl SceneWriter for LocalFile {
    fn base_path(&self) -> Option<&Path> {
        Some(&self.base_path)
    }
}
