//! Owner-bottom / thief-top island deque. Cap is fixed so the buffer never reallocates.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One worker's island queue. Owner pushes/pops `bottom`; thieves steal `top`.
pub(crate) struct StealDeque {
    buf: Box<[UnsafeCell<u16>]>,
    top: AtomicUsize,
    bottom: AtomicUsize,
}

// Workers share `&StealDeque`. Only the owner writes `bottom`; thieves CAS `top`.
unsafe impl Sync for StealDeque {}

impl StealDeque {
    pub(crate) fn with_cap(cap: usize) -> Self {
        let n = cap.max(1);
        let mut buf = Vec::with_capacity(n);
        for _ in 0..n {
            buf.push(UnsafeCell::new(0));
        }
        Self {
            buf: buf.into_boxed_slice(),
            top: AtomicUsize::new(0),
            bottom: AtomicUsize::new(0),
        }
    }

    /// Owner-only. Must not run concurrently with another owner op on this deque.
    pub(crate) fn push_bottom(&self, island: u16) {
        let b = self.bottom.load(Ordering::Relaxed);
        if b >= self.buf.len() {
            return;
        }
        // SAFETY: owner has exclusive bottom; `b < cap` so the slot is in range.
        // Setup runs before workers spawn; later owner pushes are not used.
        unsafe {
            *self.buf[b].get() = island;
        }
        self.bottom.store(b + 1, Ordering::Release);
    }

    /// Owner-only pop.
    pub(crate) fn pop_bottom(&self) -> Option<u16> {
        let b = self.bottom.load(Ordering::Relaxed);
        if b == 0 {
            return None;
        }
        let b = b - 1;
        self.bottom.store(b, Ordering::Relaxed);
        let t = self.top.load(Ordering::Acquire);
        if b < t {
            self.bottom.store(b + 1, Ordering::Relaxed);
            return None;
        }
        // SAFETY: `b` was in [top, bottom) when we decremented; slot is frozen
        // until a later push overwrites it, which we never do after workers start.
        let v = unsafe { *self.buf[b].get() };
        if b == t {
            let ok = self
                .top
                .compare_exchange(t, t + 1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok();
            self.bottom.store(b + 1, Ordering::Relaxed);
            if !ok {
                return None;
            }
            return Some(v);
        }
        Some(v)
    }

    /// Thief pop from the top.
    pub(crate) fn steal(&self) -> Option<u16> {
        let t = self.top.load(Ordering::Acquire);
        let b = self.bottom.load(Ordering::Acquire);
        if t >= b {
            return None;
        }
        // SAFETY: `t < b` and `t < cap`; CAS claims this slot before we read it.
        if self
            .top
            .compare_exchange(t, t + 1, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            Some(unsafe { *self.buf[t].get() })
        } else {
            None
        }
    }
}

pub(crate) fn next_island(self_ix: usize, deques: &[StealDeque]) -> Option<u16> {
    if let Some(id) = deques[self_ix].pop_bottom() {
        return Some(id);
    }
    for (i, q) in deques.iter().enumerate() {
        if i == self_ix {
            continue;
        }
        if let Some(id) = q.steal() {
            return Some(id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_pops_lifo_thief_pops_fifo() {
        let q = StealDeque::with_cap(4);
        q.push_bottom(1);
        q.push_bottom(2);
        q.push_bottom(3);
        assert_eq!(q.steal(), Some(1));
        assert_eq!(q.pop_bottom(), Some(3));
        assert_eq!(q.pop_bottom(), Some(2));
        assert_eq!(q.pop_bottom(), None);
    }

    #[test]
    fn cap_drop_is_fail_closed() {
        let q = StealDeque::with_cap(1);
        q.push_bottom(7);
        q.push_bottom(8);
        assert_eq!(q.pop_bottom(), Some(7));
        assert_eq!(q.pop_bottom(), None);
    }
}
