//! Chunked copy-on-write columns. Snapshot clones bump Arcs; writes clone
//! only the dirty chunk.

use std::sync::Arc;

/// Rows per CoW chunk. A pose write after publish clones this many rows, not
/// the whole column.
pub(crate) const COW_CHUNK: usize = 256;

/// Parallel column that shares unchanged chunks with published snapshots.
#[derive(Clone, Debug)]
pub(crate) struct CowCol<T> {
    chunks: Vec<Arc<Vec<T>>>,
    len: usize,
}

impl<T> Default for CowCol<T> {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
        }
    }
}

impl<T: Clone> CowCol<T> {
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn push(&mut self, v: T) {
        let c = self.len / COW_CHUNK;
        if c == self.chunks.len() {
            let mut chunk = Vec::with_capacity(COW_CHUNK);
            chunk.push(v);
            self.chunks.push(Arc::new(chunk));
        } else {
            Arc::make_mut(&mut self.chunks[c]).push(v);
        }
        self.len += 1;
    }

    #[must_use]
    pub(crate) fn get(&self, i: usize) -> Option<&T> {
        if i >= self.len {
            return None;
        }
        self.chunks.get(i / COW_CHUNK)?.get(i % COW_CHUNK)
    }

    pub(crate) fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        if i >= self.len {
            return None;
        }
        let chunk = Arc::make_mut(self.chunks.get_mut(i / COW_CHUNK)?);
        chunk.get_mut(i % COW_CHUNK)
    }

    pub(crate) fn set(&mut self, i: usize, v: T) {
        *self.get_mut(i).expect("packed index in range") = v;
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn shares_chunk(&self, other: &Self, i: usize) -> bool {
        let c = i / COW_CHUNK;
        match (self.chunks.get(c), other.chunks.get(c)) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    #[must_use]
    pub(crate) fn approx_bytes(&self) -> usize {
        self.len.saturating_mul(core::mem::size_of::<T>()) + self.chunks.len().saturating_mul(16)
    }
}

impl<T: Clone + PartialEq> PartialEq for CowCol<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        (0..self.len).all(|i| self.get(i) == other.get(i))
    }
}

impl<T: Clone + Eq> Eq for CowCol<T> {}
