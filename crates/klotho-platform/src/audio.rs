//! Audio output device. No world mutation.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[cfg(feature = "audio-device")]
use cpal::Sample;
#[cfg(feature = "audio-device")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Mix and device sample rate, hertz.
pub const OUTPUT_HZ: u32 = 48_000;
/// Interleaved stereo.
pub const OUTPUT_CHANNELS: u16 = 2;
/// Max interleaved i16 samples retained for the callback (~250 ms at 48 kHz stereo).
pub const PCM_QUEUE_CAP: usize = 24_000;

/// Push interleaved i16 PCM toward an output.
pub trait AudioSink: Send {
    /// Append samples. Implementations must bound retained length.
    fn push(&mut self, pcm: &[i16]);
}

/// Why opening an output device failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum AudioDeviceError {
    /// Audio host could not be initialised.
    Host,
    /// Host has no default output device.
    NoDevice,
    /// Device cannot run 48 kHz stereo i16 or f32.
    Unsupported,
    /// Stream could not be built or started.
    Stream,
}

impl std::fmt::Display for AudioDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Host => f.write_str("audio host is unavailable"),
            Self::NoDevice => f.write_str("no default audio output device"),
            Self::Unsupported => f.write_str("audio output does not support 48 kHz stereo"),
            Self::Stream => f.write_str("audio output stream failed"),
        }
    }
}

impl std::error::Error for AudioDeviceError {}

#[derive(Debug)]
struct PcmQueue {
    samples: VecDeque<i16>,
    cap: usize,
}

impl PcmQueue {
    fn with_cap(cap: usize) -> Self {
        Self {
            samples: VecDeque::new(),
            cap,
        }
    }

    fn len(&self) -> usize {
        self.samples.len()
    }

    fn snapshot(&self) -> Vec<i16> {
        self.samples.iter().copied().collect()
    }

    #[cfg(feature = "audio-device")]
    fn pop(&mut self) -> i16 {
        self.samples.pop_front().unwrap_or(0)
    }

    #[cfg(feature = "audio-device")]
    fn pull(&mut self, dest: &mut [i16]) {
        for s in dest.iter_mut() {
            *s = self.pop();
        }
    }

    fn push(&mut self, pcm: &[i16]) {
        if self.cap == 0 {
            return;
        }
        let src = if pcm.len() > self.cap {
            &pcm[pcm.len() - self.cap..]
        } else {
            pcm
        };
        let overflow = self
            .samples
            .len()
            .saturating_add(src.len())
            .saturating_sub(self.cap);
        if overflow > 0 {
            let n = overflow.min(self.samples.len());
            let _ = self.samples.drain(..n);
        }
        self.samples.extend(src.iter().copied());
    }
}

fn lock_queue(q: &Mutex<PcmQueue>) -> std::sync::MutexGuard<'_, PcmQueue> {
    q.lock().unwrap_or_else(|e| e.into_inner())
}

/// Records PCM in memory. Shared so a test thread can inspect a mixer-owned sink.
#[derive(Clone, Debug)]
pub struct MemorySink {
    inner: Arc<Mutex<PcmQueue>>,
}

impl MemorySink {
    /// Empty sink, cap [`PCM_QUEUE_CAP`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_cap(PCM_QUEUE_CAP)
    }

    /// Empty sink with an explicit sample cap.
    #[must_use]
    pub fn with_cap(cap: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PcmQueue::with_cap(cap))),
        }
    }

    /// Copy of samples currently retained (oldest first).
    #[must_use]
    pub fn samples(&self) -> Vec<i16> {
        lock_queue(&self.inner).snapshot()
    }

    /// Number of retained interleaved samples.
    #[must_use]
    pub fn len(&self) -> usize {
        lock_queue(&self.inner).len()
    }

    /// `true` if no samples are retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for MemorySink {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSink for MemorySink {
    fn push(&mut self, pcm: &[i16]) {
        lock_queue(&self.inner).push(pcm);
    }
}

/// cpal output wrapping a bounded PCM queue. Failure to open is a [`Result`].
#[cfg(feature = "audio-device")]
pub struct CpalDevice {
    _stream: cpal::Stream,
    queue: Arc<Mutex<PcmQueue>>,
}

#[cfg(feature = "audio-device")]
impl CpalDevice {
    /// Open the default output at 48 kHz stereo. Does not panic if hardware is missing.
    pub fn open() -> Result<Self, AudioDeviceError> {
        let host = open_host()?;
        let device = host
            .default_output_device()
            .ok_or(AudioDeviceError::NoDevice)?;
        let config = cpal::StreamConfig {
            channels: OUTPUT_CHANNELS,
            sample_rate: cpal::SAMPLE_RATE_48K,
            buffer_size: cpal::BufferSize::Default,
        };
        let queue = Arc::new(Mutex::new(PcmQueue::with_cap(PCM_QUEUE_CAP)));
        let stream = build_stream(&device, config, Arc::clone(&queue))?;
        stream.play().map_err(map_cpal_err)?;
        Ok(Self {
            _stream: stream,
            queue,
        })
    }
}

#[cfg(feature = "audio-device")]
impl AudioSink for CpalDevice {
    fn push(&mut self, pcm: &[i16]) {
        lock_queue(&self.queue).push(pcm);
    }
}

#[cfg(feature = "audio-device")]
fn open_host() -> Result<cpal::Host, AudioDeviceError> {
    // `default_host()` panics on Linux when ALSA init fails; host_from_id does not.
    let ids = cpal::available_hosts();
    if ids.is_empty() {
        return Err(AudioDeviceError::Host);
    }
    let mut last = AudioDeviceError::Host;
    for id in ids {
        match cpal::host_from_id(id) {
            Ok(host) => return Ok(host),
            Err(e) => last = map_cpal_err(e),
        }
    }
    Err(last)
}

#[cfg(feature = "audio-device")]
fn map_cpal_err(err: cpal::Error) -> AudioDeviceError {
    match err.kind() {
        cpal::ErrorKind::UnsupportedConfig | cpal::ErrorKind::UnsupportedOperation => {
            AudioDeviceError::Unsupported
        }
        cpal::ErrorKind::DeviceNotAvailable => AudioDeviceError::NoDevice,
        cpal::ErrorKind::HostUnavailable => AudioDeviceError::Host,
        _ => AudioDeviceError::Stream,
    }
}

#[cfg(feature = "audio-device")]
fn is_unsupported(err: &cpal::Error) -> bool {
    matches!(
        err.kind(),
        cpal::ErrorKind::UnsupportedConfig | cpal::ErrorKind::UnsupportedOperation
    )
}

#[cfg(feature = "audio-device")]
fn ignore_stream_err(_err: cpal::Error) {}

#[cfg(feature = "audio-device")]
fn build_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    queue: Arc<Mutex<PcmQueue>>,
) -> Result<cpal::Stream, AudioDeviceError> {
    let q_i16 = Arc::clone(&queue);
    match device.build_output_stream(
        config,
        move |data: &mut [i16], _| match q_i16.try_lock() {
            Ok(mut q) => q.pull(data),
            Err(_) => data.fill(0),
        },
        ignore_stream_err,
        None,
    ) {
        Ok(stream) => return Ok(stream),
        Err(e) if is_unsupported(&e) => {}
        Err(e) => return Err(map_cpal_err(e)),
    }
    let q_f32 = queue;
    device
        .build_output_stream(
            config,
            move |data: &mut [f32], _| match q_f32.try_lock() {
                Ok(mut q) => {
                    for s in data.iter_mut() {
                        *s = q.pop().to_sample::<f32>();
                    }
                }
                Err(_) => data.fill(0.0),
            },
            ignore_stream_err,
            None,
        )
        .map_err(map_cpal_err)
}

const _: () = assert!(OUTPUT_HZ == 48_000);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_spec_is_48k_stereo() {
        assert_eq!(OUTPUT_HZ, 48_000);
        assert_eq!(OUTPUT_CHANNELS, 2);
        assert_eq!(PCM_QUEUE_CAP, 24_000);
    }

    #[test]
    fn memory_sink_records_exact_pcm() {
        let mut s = MemorySink::new();
        s.push(&[1, 2, 3, 4]);
        assert_eq!(s.samples(), [1, 2, 3, 4]);
        s.push(&[5, 6]);
        assert_eq!(s.samples(), [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn queue_cap_drops_oldest() {
        let mut s = MemorySink::with_cap(4);
        s.push(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(s.len() <= 4);
        assert_eq!(s.samples(), [5, 6, 7, 8]);
        s.push(&[9, 10]);
        assert_eq!(s.len(), 4);
        assert_eq!(s.samples(), [7, 8, 9, 10]);
    }

    #[test]
    fn huge_push_is_capped() {
        let mut s = MemorySink::with_cap(8);
        let huge = vec![1i16; PCM_QUEUE_CAP + 100];
        s.push(&huge);
        assert!(s.len() <= 8);
        assert_eq!(s.len(), 8);
        assert!(s.samples().iter().all(|&v| v == 1));
    }

    #[test]
    fn device_error_display_matches_kind() {
        assert_eq!(
            AudioDeviceError::Host.to_string(),
            "audio host is unavailable"
        );
        assert_eq!(
            AudioDeviceError::NoDevice.to_string(),
            "no default audio output device"
        );
        assert_eq!(
            AudioDeviceError::Unsupported.to_string(),
            "audio output does not support 48 kHz stereo"
        );
        assert_eq!(
            AudioDeviceError::Stream.to_string(),
            "audio output stream failed"
        );
    }

    #[cfg(feature = "audio-device")]
    #[test]
    fn cpal_error_kind_maps_stream_vs_unsupported() {
        assert_eq!(
            map_cpal_err(cpal::Error::new(cpal::ErrorKind::UnsupportedConfig)),
            AudioDeviceError::Unsupported
        );
        assert_eq!(
            map_cpal_err(cpal::Error::new(cpal::ErrorKind::DeviceBusy)),
            AudioDeviceError::Stream
        );
        assert_eq!(
            map_cpal_err(cpal::Error::new(cpal::ErrorKind::HostUnavailable)),
            AudioDeviceError::Host
        );
    }

    #[test]
    fn cloned_sink_sees_pushed_pcm() {
        let mut a = MemorySink::with_cap(4);
        let b = a.clone();
        a.push(&[9, 8, 7]);
        assert_eq!(b.samples(), [9, 8, 7]);
    }
}
