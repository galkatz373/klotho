//! Read-only file map after a size cap. mmap on the production path.

use std::fs::{self, File};
use std::path::Path;

use crate::error::StreamError;

/// Cap `path`'s length, then map (or read, under Miri).
pub(crate) fn map_or_read(path: &Path, cap: usize) -> Result<Mapped, StreamError> {
    let file = File::open(path)?;
    let len = file_len_at_most(path, cap)?;
    map_file(&file, len)
}

/// Read at most `cap` bytes. Used for the catalog (small) and Miri.
pub(crate) fn read_capped(path: &Path, cap: usize) -> Result<Vec<u8>, StreamError> {
    let _ = file_len_at_most(path, cap)?;
    let bytes = fs::read(path)?;
    if bytes.len() > cap {
        return Err(StreamError::Oversize {
            size: bytes.len(),
            cap,
        });
    }
    Ok(bytes)
}

pub(crate) fn file_len_at_most(path: &Path, cap: usize) -> Result<usize, StreamError> {
    let meta = fs::metadata(path)?;
    let len = meta.len();
    if len > cap as u64 {
        return Err(StreamError::Oversize {
            size: usize::try_from(len).unwrap_or(usize::MAX),
            cap,
        });
    }
    Ok(len as usize)
}

fn map_file(file: &File, len: usize) -> Result<Mapped, StreamError> {
    if len == 0 {
        return Ok(Mapped {
            inner: MapInner::Owned(Vec::new()),
        });
    }
    #[cfg(miri)]
    {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = file.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        let mut buf = Vec::new();
        file.take(len as u64).read_to_end(&mut buf)?;
        if buf.len() != len {
            return Err(StreamError::Truncated);
        }
        Ok(Mapped {
            inner: MapInner::Owned(buf),
        })
    }
    #[cfg(not(miri))]
    {
        // SAFETY: `len` is the on-disk size already checked against the shard
        // or volume cap. The mapping is read-only. Callers must not mutate the
        // file for the lifetime of the returned slice; header validation has
        // already run on a bounded prefix.
        let mmap = unsafe { memmap2::MmapOptions::new().len(len).map(file)? };
        Ok(Mapped {
            inner: MapInner::Mmap(mmap),
        })
    }
}

/// Owned or mapped file bytes.
#[derive(Debug)]
pub(crate) struct Mapped {
    inner: MapInner,
}

#[derive(Debug)]
enum MapInner {
    #[cfg(not(miri))]
    Mmap(memmap2::Mmap),
    Owned(Vec<u8>),
}

impl Mapped {
    pub(crate) fn as_slice(&self) -> &[u8] {
        match &self.inner {
            #[cfg(not(miri))]
            MapInner::Mmap(m) => m,
            MapInner::Owned(v) => v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "klotho-stream-map-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[test]
    fn oversize_is_refused_before_read() {
        let path = temp_path();
        fs::write(&path, b"abcdefgh").unwrap();
        let e = read_capped(&path, 4).unwrap_err();
        assert!(
            matches!(e, StreamError::Oversize { size: 8, cap: 4 }),
            "{e}"
        );
        let e = map_or_read(&path, 4).unwrap_err();
        assert!(
            matches!(e, StreamError::Oversize { size: 8, cap: 4 }),
            "{e}"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn map_or_read_matches_bytes() {
        let path = temp_path();
        fs::write(&path, b"kplc-payload").unwrap();
        let mapped = map_or_read(&path, 64).unwrap();
        assert_eq!(mapped.as_slice(), b"kplc-payload");
        let _ = fs::remove_file(&path);
    }
}
