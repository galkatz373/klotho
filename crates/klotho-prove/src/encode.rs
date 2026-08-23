//! Canonical little-endian byte buffer. Hashed bytes never go through serde.

/// Growable LE writer used to freeze provenance and license encodings.
#[derive(Clone, Debug, Default)]
pub(crate) struct CanonBuf {
    bytes: Vec<u8>,
}

impl CanonBuf {
    pub(crate) fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }

    pub(crate) fn u32_le(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn bytes(&mut self, data: &[u8]) {
        self.u32_le(data.len() as u32);
        self.bytes.extend_from_slice(data);
    }

    pub(crate) fn arr32(&mut self, data: &[u8; 32]) {
        self.bytes.extend_from_slice(data);
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_is_little_endian() {
        let mut b = CanonBuf::new();
        b.u32_le(1);
        assert_eq!(b.as_slice(), &[1, 0, 0, 0]);
    }

    #[test]
    fn bytes_are_length_prefixed() {
        let mut b = CanonBuf::new();
        b.bytes(b"ab");
        assert_eq!(b.as_slice(), &[2, 0, 0, 0, b'a', b'b']);
    }
}
