//! Dedicated render thread (Q2). Sim does not wait on GPU.

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use klotho_manifest::{GpuBudget, Observer, VisualManifest};

use crate::presenter::Presenter;

enum Msg {
    Frame {
        vis: Box<VisualManifest>,
        observer: Observer,
        budget: GpuBudget,
    },
    Stop,
}

/// Owns a [`Presenter`] on a thread named `klotho-render`.
pub struct RenderThread {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

impl RenderThread {
    /// Spawn. The presenter moves onto the render thread.
    pub fn spawn<P: Presenter + 'static>(mut presenter: P) -> Self {
        let (tx, rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("klotho-render".into())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Frame {
                            vis,
                            observer,
                            budget,
                        } => presenter.present(&vis, observer, budget),
                        Msg::Stop => break,
                    }
                }
            })
            .expect("render thread");
        Self {
            tx,
            join: Some(join),
        }
    }

    /// Queue a frame. Returns immediately — the sim thread must not wait (Q2).
    pub fn submit(&self, vis: VisualManifest, observer: Observer, budget: GpuBudget) {
        let _ = self.tx.send(Msg::Frame {
            vis: Box::new(vis),
            observer,
            budget,
        });
    }
}

impl Drop for RenderThread {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    use klotho_core::Epoch;
    use klotho_manifest::{GpuBudget, Observer, VisualManifest};

    use super::*;
    use crate::presenter::{NullPresenter, Presenter};

    struct Counting {
        n: Arc<AtomicU32>,
        inner: NullPresenter,
    }

    impl Presenter for Counting {
        fn present(&mut self, vis: &VisualManifest, observer: Observer, budget: GpuBudget) {
            self.inner.present(vis, observer, budget);
            self.n.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn sim_does_not_run_present() {
        let n = Arc::new(AtomicU32::new(0));
        let rt = RenderThread::spawn(Counting {
            n: Arc::clone(&n),
            inner: NullPresenter::default(),
        });
        let submit_thread = thread::current().id();
        rt.submit(
            VisualManifest::empty(Epoch::ZERO),
            Observer::origin(),
            GpuBudget::HEARTH,
        );
        let start = Instant::now();
        while n.load(Ordering::SeqCst) == 0 && start.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(n.load(Ordering::SeqCst) >= 1);
        assert_eq!(submit_thread, thread::current().id());
        drop(rt);
    }
}
