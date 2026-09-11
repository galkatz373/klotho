//! Window spec and capped file reads. No world mutation.

use std::fs;
use std::path::Path;

/// Desktop window request. Actual `winit::Window` is created by the runtime
/// event loop (PR 12b pixels / Distaff preview).
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct WindowSpec {
    /// Title bar.
    pub title: String,
    /// Inner width, pixels.
    pub width: u32,
    /// Inner height, pixels.
    pub height: u32,
}

impl WindowSpec {
    /// Hearth default 1280×720.
    #[must_use]
    pub fn hearth() -> Self {
        Self {
            title: "Klotho".into(),
            width: 1280,
            height: 720,
        }
    }
}

impl Default for WindowSpec {
    fn default() -> Self {
        Self::hearth()
    }
}

/// Read a file, refusing anything larger than `max` bytes before `fs::read`.
pub fn read_capped(path: &Path, max: usize) -> Result<Vec<u8>, std::io::Error> {
    let meta = fs::metadata(path)?;
    let len = meta.len() as usize;
    if len > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file {} bytes exceeds cap {max}", meta.len()),
        ));
    }
    fs::read(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hearth_window_is_720p() {
        let w = WindowSpec::hearth();
        assert_eq!(w.width, 1280);
        assert_eq!(w.height, 720);
    }

    #[test]
    fn read_capped_refuses_oversize() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/kitbash/catalog.ron");
        let err = read_capped(&p, 8).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(read_capped(&p, 1_000_000).unwrap().starts_with(b"Catalog"));
    }
}
