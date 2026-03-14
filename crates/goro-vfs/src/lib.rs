use std::io;
use std::path::{Path, PathBuf};

/// File open mode
#[derive(Debug, Clone, Copy)]
pub enum OpenMode {
    Read,
    Write,
    Append,
    ReadWrite,
}

/// File metadata
#[derive(Debug, Clone)]
pub struct FileStat {
    pub size: u64,
    pub is_file: bool,
    pub is_dir: bool,
    pub mtime: u64,
}

/// Virtual file handle
pub trait VfsFile: io::Read + io::Write {
    fn stat(&self) -> io::Result<FileStat>;
}

/// Virtual filesystem trait - all file operations go through this
pub trait Vfs {
    fn open(&self, path: &Path, mode: OpenMode) -> io::Result<Box<dyn VfsFile>>;
    fn stat(&self, path: &Path) -> io::Result<FileStat>;
    fn exists(&self, path: &Path) -> io::Result<bool>;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn read_to_bytes(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn realpath(&self, path: &Path) -> io::Result<PathBuf>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
}

/// Real filesystem implementation with optional path restrictions
pub struct RealVfs {
    /// Allowed root directories (empty = allow all)
    allowed_roots: Vec<PathBuf>,
}

impl RealVfs {
    /// Create an unrestricted real filesystem
    pub fn new() -> Self {
        Self {
            allowed_roots: Vec::new(),
        }
    }

    /// Create a restricted filesystem that only allows access under the given roots
    pub fn restricted(roots: Vec<PathBuf>) -> Self {
        Self {
            allowed_roots: roots,
        }
    }

    fn check_access(&self, path: &Path) -> io::Result<()> {
        if self.allowed_roots.is_empty() {
            return Ok(());
        }
        let canonical = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf());
        for root in &self.allowed_roots {
            if canonical.starts_with(root) {
                return Ok(());
            }
        }
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("access denied: {}", path.display()),
        ))
    }
}

struct RealFile {
    file: std::fs::File,
}

impl io::Read for RealFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl io::Write for RealFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl VfsFile for RealFile {
    fn stat(&self) -> io::Result<FileStat> {
        let meta = self.file.metadata()?;
        Ok(FileStat {
            size: meta.len(),
            is_file: meta.is_file(),
            is_dir: meta.is_dir(),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        })
    }
}

impl Vfs for RealVfs {
    fn open(&self, path: &Path, mode: OpenMode) -> io::Result<Box<dyn VfsFile>> {
        self.check_access(path)?;
        let file = match mode {
            OpenMode::Read => std::fs::File::open(path)?,
            OpenMode::Write => std::fs::File::create(path)?,
            OpenMode::Append => std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)?,
            OpenMode::ReadWrite => std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(path)?,
        };
        Ok(Box::new(RealFile { file }))
    }

    fn stat(&self, path: &Path) -> io::Result<FileStat> {
        self.check_access(path)?;
        let meta = std::fs::metadata(path)?;
        Ok(FileStat {
            size: meta.len(),
            is_file: meta.is_file(),
            is_dir: meta.is_dir(),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        })
    }

    fn exists(&self, path: &Path) -> io::Result<bool> {
        self.check_access(path)?;
        Ok(path.exists())
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.check_access(path)?;
        std::fs::read_to_string(path)
    }

    fn read_to_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.check_access(path)?;
        std::fs::read(path)
    }

    fn realpath(&self, path: &Path) -> io::Result<PathBuf> {
        self.check_access(path)?;
        std::fs::canonicalize(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.check_access(path).is_ok() && path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.check_access(path).is_ok() && path.is_dir()
    }
}

impl Default for RealVfs {
    fn default() -> Self {
        Self::new()
    }
}

/// Null filesystem - denies all access (for sandboxed execution)
pub struct NullVfs;

impl Vfs for NullVfs {
    fn open(&self, _path: &Path, _mode: OpenMode) -> io::Result<Box<dyn VfsFile>> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "filesystem access denied",
        ))
    }
    fn stat(&self, _path: &Path) -> io::Result<FileStat> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "filesystem access denied",
        ))
    }
    fn exists(&self, _path: &Path) -> io::Result<bool> {
        Ok(false)
    }
    fn read_to_string(&self, _path: &Path) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "filesystem access denied",
        ))
    }
    fn read_to_bytes(&self, _path: &Path) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "filesystem access denied",
        ))
    }
    fn realpath(&self, _path: &Path) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "filesystem access denied",
        ))
    }
    fn is_file(&self, _path: &Path) -> bool {
        false
    }
    fn is_dir(&self, _path: &Path) -> bool {
        false
    }
}
