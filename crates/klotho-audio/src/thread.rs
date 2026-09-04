//! Dedicated audio thread. Sim does not wait on mix.

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use klotho_core::Tick;
use klotho_manifest::{Observer, SonicManifest};

use crate::mix::{MixBudget, Mixer};

enum Msg {
    Mix {
        sonic: Box<SonicManifest>,
        observer: Observer,
        now: Tick,
        budget: MixBudget,
    },
    Stop,
}

/// Owns a [`Mixer`] on a thread named `klotho-audio`.
pub struct AudioThread {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

impl AudioThread {
    /// Mixer is moved here so the sim thread does not mix.
    pub fn spawn<M: Mixer + 'static>(mut mixer: M) -> Self {
        let (tx, rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("klotho-audio".into())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Mix {
                            sonic,
                            observer,
                            now,
                            budget,
                        } => {
                            let _ = mixer.mix(&sonic, observer, now, budget);
                        }
                        Msg::Stop => break,
                    }
                }
            })
            .expect("audio thread");
        Self {
            tx,
            join: Some(join),
        }
    }

    /// Queue a mix. Returns immediately — the sim thread must not wait.
    pub fn submit(&self, sonic: SonicManifest, observer: Observer, now: Tick, budget: MixBudget) {
        let _ = self.tx.send(Msg::Mix {
            sonic: Box::new(sonic),
            observer,
            now,
            budget,
        });
    }
}

impl Drop for AudioThread {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use klotho_compile::{MAGIC, VERSION};
    use klotho_core::{BlobId, Epoch, Tick};
    use klotho_manifest::{GrainVoice, Observer, SonicManifest};
    use klotho_platform::MemorySink;

    use super::*;
    use crate::mix::{DeviceMixer, MixBudget, SAMPLES_PER_TICK};

    fn blob(n: u8) -> BlobId {
        let mut b = [0u8; 32];
        b[0] = n;
        BlobId::from_bytes(b)
    }

    fn valid_pcm(pcm: &[i16]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&MAGIC);
        b.push(VERSION);
        b.push(3);
        b.push(0);
        b.push(0);
        b.extend_from_slice(&crate::GRAIN_HZ.to_le_bytes());
        b.push(1);
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        for s in pcm {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    #[test]
    fn submit_returns_and_sink_gets_frame() {
        let id = blob(1);
        let sink = MemorySink::new();
        let probe = sink.clone();
        let mut mixer = DeviceMixer::new(sink);
        mixer.insert(id, &valid_pcm(&[3_000; 8]));
        let at = AudioThread::spawn(mixer);
        let submit_thread = thread::current().id();
        let sonic = SonicManifest::from_voices(
            Epoch::ZERO,
            [GrainVoice {
                blob: id,
                at: Tick(0),
                gain_milli: 1000,
                pos: None,
                occluded: false,
            }],
            None,
        );
        at.submit(sonic, Observer::origin(), Tick(0), MixBudget::HEARTH);
        assert_eq!(submit_thread, thread::current().id());
        let want_len = (SAMPLES_PER_TICK as usize).saturating_mul(2);
        let start = Instant::now();
        while probe.len() < want_len && start.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(1));
        }
        let rec = probe.samples();
        assert_eq!(
            rec.len(),
            want_len,
            "audio thread did not push a mix quantum within timeout"
        );
        assert_eq!(rec[0], 3_000);
        assert_eq!(rec[1], 3_000);
        drop(at);
    }
}
