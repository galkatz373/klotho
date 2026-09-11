//! Loom model for the last-item owner/thief race in the steal deque.

use loom::sync::Arc;
use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::thread;

#[test]
fn last_item_is_claimed_at_most_once() {
    loom::model(|| {
        let top = Arc::new(AtomicUsize::new(0));
        let bottom = Arc::new(AtomicUsize::new(1));
        let claims = Arc::new(AtomicUsize::new(0));

        let thief_top = Arc::clone(&top);
        let thief_bottom = Arc::clone(&bottom);
        let thief_claims = Arc::clone(&claims);
        let thief = thread::spawn(move || {
            let t = thief_top.load(Ordering::Acquire);
            let b = thief_bottom.load(Ordering::Acquire);
            if t < b
                && thief_top
                    .compare_exchange(t, t + 1, Ordering::SeqCst, Ordering::Relaxed)
                    .is_ok()
            {
                thief_claims.fetch_add(1, Ordering::SeqCst);
            }
        });

        let owner_top = Arc::clone(&top);
        let owner_bottom = Arc::clone(&bottom);
        let owner_claims = Arc::clone(&claims);
        let owner = thread::spawn(move || {
            let b = owner_bottom.load(Ordering::Relaxed);
            if b == 0 {
                return;
            }
            let last = b - 1;
            owner_bottom.store(last, Ordering::Relaxed);
            let t = owner_top.load(Ordering::Acquire);
            if last >= t {
                let won = last != t
                    || owner_top
                        .compare_exchange(t, t + 1, Ordering::SeqCst, Ordering::Relaxed)
                        .is_ok();
                if won {
                    owner_claims.fetch_add(1, Ordering::SeqCst);
                }
            }
            owner_bottom.store(last + 1, Ordering::Relaxed);
        });

        thief.join().unwrap();
        owner.join().unwrap();
        assert!(claims.load(Ordering::SeqCst) <= 1);
    });
}
