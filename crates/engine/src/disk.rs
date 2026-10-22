//! Where commands read and write files. On the desktop and in the CLI that is the file system; the
//! browser has none, so there the session keeps files in memory: the web app puts in the files a
//! person picks or drops, and hands back, as downloads, the files commands write. Commands take
//! the same `path` parameters either way.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mkaegis_core::{Error, Result};

/// The most a memory disk holds: past it, putting or writing a file fails rather than exhaust the
/// page's memory (a wasm32 page has 4 GB at most, the map included).
const MAX_MEMORY_BYTES: usize = 1 << 30;

/// The files commands read and write.
#[derive(Debug, Clone)]
pub enum Disk {
    /// The file system.
    Native,
    /// Files held in memory by path (the browser).
    Memory(Memory),
}

/// Files in memory, and which of them commands wrote.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    files: BTreeMap<PathBuf, Vec<u8>>,
    /// Written since the host last took them, oldest first, each once.
    written: Vec<PathBuf>,
}

impl Default for Disk {
    /// The file system, except in the browser, which has none.
    fn default() -> Self {
        if cfg!(target_arch = "wasm32") { Disk::Memory(Memory::default()) } else { Disk::Native }
    }
}

impl Disk {
    /// A file's size in bytes, without reading it.
    pub fn len(&self, path: &Path) -> Result<u64> {
        match self {
            Disk::Native => Ok(std::fs::metadata(path)?.len()),
            Disk::Memory(m) => Ok(m.get(path)?.len() as u64),
        }
    }

    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        match self {
            Disk::Native => Ok(std::fs::read(path)?),
            Disk::Memory(m) => Ok(m.get(path)?.to_vec()),
        }
    }

    /// A file's text, which must be UTF-8.
    pub fn read_text(&self, path: &Path) -> Result<String> {
        String::from_utf8(self.read(path)?).map_err(|_| Error::Io(format!("`{}` is not UTF-8 text", path.display())))
    }

    /// Writes a whole file: on the file system with [`crate::io::write_atomic`], so a failed write
    /// leaves the old file as it was.
    pub fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        match self {
            Disk::Native => crate::io::write_atomic(path, bytes),
            Disk::Memory(m) => m.write(path, bytes),
        }
    }

    /// The files in memory, when that is where they are.
    pub fn memory_mut(&mut self) -> Option<&mut Memory> {
        match self {
            Disk::Native => None,
            Disk::Memory(m) => Some(m),
        }
    }

    pub fn is_memory(&self) -> bool {
        matches!(self, Disk::Memory(_))
    }
}

impl Memory {
    /// Puts in a file of the person's own (picked or dropped), replacing one at the same path. It
    /// doesn't count as written.
    pub fn put(&mut self, path: impl Into<PathBuf>, bytes: Vec<u8>) -> Result<()> {
        let path = path.into();
        self.make_room(&path, bytes.len())?;
        self.files.insert(path, bytes);
        Ok(())
    }

    /// The files written since the last call, oldest first, each as last written.
    pub fn take_written(&mut self) -> Vec<(PathBuf, Vec<u8>)> {
        std::mem::take(&mut self.written).into_iter().filter_map(|p| self.files.get(&p).map(|b| (p.clone(), b.clone()))).collect()
    }

    fn get(&self, path: &Path) -> Result<&[u8]> {
        self.files.get(path).map(Vec::as_slice).ok_or_else(|| Error::NotFound(format!("`{}`", path.display())))
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.put(path, bytes.to_vec())?;
        if !self.written.iter().any(|p| p == path) {
            self.written.push(path.to_path_buf());
        }
        Ok(())
    }

    /// Fails when `len` more bytes at `path` would take the files past [`MAX_MEMORY_BYTES`].
    fn make_room(&self, path: &Path, len: usize) -> Result<()> {
        let others: usize = self.files.iter().filter(|(p, _)| p.as_path() != path).map(|(_, b)| b.len()).sum();
        if others.saturating_add(len) > MAX_MEMORY_BYTES {
            return Err(Error::Io(format!(
                "`{}` doesn't fit: the files open in this page are limited to {} MB",
                path.display(),
                MAX_MEMORY_BYTES >> 20
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_reads_what_was_put_and_reports_only_what_was_written() {
        let mut disk = Disk::Memory(Memory::default());
        disk.memory_mut().unwrap().put("towns.geojson", b"{}".to_vec()).unwrap();
        assert_eq!(disk.read_text(Path::new("towns.geojson")).unwrap(), "{}");
        assert_eq!(disk.len(Path::new("towns.geojson")).unwrap(), 2);
        assert!(matches!(disk.read(Path::new("missing.shp")), Err(Error::NotFound(_))));

        disk.write(Path::new("map.aegis"), b"first").unwrap();
        disk.write(Path::new("map.png"), b"png").unwrap();
        disk.write(Path::new("map.aegis"), b"second").unwrap();
        let written = disk.memory_mut().map(Memory::take_written).unwrap_or_default();
        assert_eq!(written, vec![(PathBuf::from("map.aegis"), b"second".to_vec()), (PathBuf::from("map.png"), b"png".to_vec())]);
        assert!(disk.memory_mut().map(Memory::take_written).unwrap_or_default().is_empty());
        // A written file stays readable: a saved project opens again.
        assert_eq!(disk.read(Path::new("map.aegis")).unwrap(), b"second");
    }

    #[test]
    fn memory_refuses_files_past_its_limit() {
        let mut m = Memory::default();
        m.put("a.bin", vec![0; MAX_MEMORY_BYTES / 2]).unwrap();
        assert!(m.put("b.bin", vec![0; MAX_MEMORY_BYTES / 2 + 1]).is_err());
        // Replacing a file counts only the new bytes.
        m.put("a.bin", vec![0; MAX_MEMORY_BYTES]).unwrap();
    }

    #[test]
    fn text_must_be_utf8() {
        let mut disk = Disk::Memory(Memory::default());
        disk.write(Path::new("bad.geojson"), &[0xff, 0xfe]).unwrap();
        assert!(disk.read_text(Path::new("bad.geojson")).is_err());
    }
}
