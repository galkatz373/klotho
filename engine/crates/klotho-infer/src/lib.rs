//! OS-process inference isolator. Returns [`InferIntent`]; never `&mut World`.
//!
//! Only `klotho-runtime` constructs and polls this host (CI allowlist). The
//! default host is disabled. An enabled host owns a child process and exchanges
//! canonical [`WorldSnapshot`] blobs and `InferIntent` replies over pipes.
//! Child termination (including panic or OOM abort) disables inference; it does
//! not claim recovery from undefined behaviour inside the sidecar.

#![allow(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::io::{self, ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use klotho_core::{RejectReason, Tick};
use klotho_ir::{InferIntent, IntentTarget, ModelId, Name, Verb, from_ron, to_ron};
use klotho_world::WorldSnapshot;

const MAGIC: [u8; 4] = *b"KINF";
const VERSION: u8 = 1;
const REQUEST: u8 = 1;
const RESPONSE: u8 = 2;
const MAX_INTENT_BYTES: usize = 1024 * 1024;
const IPC_QUEUE: usize = 2;

/// Handle returned by [`InferHost::submit`]. Zero means not queued.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct JobId(pub u64);

/// Snapshot-stamped eval request. Stale is a job property, not an IR field.
#[derive(Clone, Debug)]
pub struct InferJob {
    /// Previous published snapshot, encoded before it crosses the process boundary.
    pub snap: Arc<WorldSnapshot>,
    /// Tick the job was kicked. Cancel if `now - tick > eval_slo_ticks`.
    pub tick: Tick,
}

/// Non-blocking copy-out of a poll.
#[derive(Clone, Debug, Default)]
pub struct InferPoll {
    /// Fresh enough to ingest. Age **equal** to the SLO cap is included.
    pub intents: Vec<InferIntent>,
    /// Dropped jobs, each [`RejectReason::StaleEpoch`]. Not ingested.
    pub stale: Vec<RejectReason>,
}

struct Transport {
    requests: SyncSender<(JobId, InferJob)>,
    replies: Mutex<Receiver<Reply>>,
    child: Mutex<Child>,
}

enum Reply {
    Intent { id: JobId, intent: InferIntent },
    Failed,
}

/// Owns the inference sidecar. Never holds `&mut World`.
pub struct InferHost {
    transport: Option<Transport>,
    pending: Mutex<BTreeMap<JobId, Tick>>,
    next: AtomicU64,
    disabled: AtomicBool,
}

impl InferHost {
    /// Construct the default infer-off host. No process is spawned.
    #[must_use]
    pub fn new() -> Self {
        Self {
            transport: None,
            pending: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(1),
            disabled: AtomicBool::new(true),
        }
    }

    /// Spawn a sidecar command wired over private stdin/stdout pipes.
    ///
    /// The command must enter [`run_sidecar`] rather than the normal runtime.
    pub fn spawn(mut command: Command) -> io::Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("infer sidecar stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("infer sidecar stdout unavailable"))?;
        let (request_tx, request_rx) = mpsc::sync_channel(IPC_QUEUE);
        let (reply_tx, reply_rx) = mpsc::channel();
        let writer_reply = reply_tx.clone();
        thread::Builder::new()
            .name("klotho-infer-ipc-write".into())
            .spawn(move || writer_loop(stdin, request_rx, writer_reply))?;
        thread::Builder::new()
            .name("klotho-infer-ipc-read".into())
            .spawn(move || reader_loop(stdout, reply_tx))?;
        Ok(Self {
            transport: Some(Transport {
                requests: request_tx,
                replies: Mutex::new(reply_rx),
                child: Mutex::new(child),
            }),
            pending: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(1),
            disabled: AtomicBool::new(false),
        })
    }

    /// Whether the child is still available. Failure is observed by [`Self::poll`].
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        !self.disabled.load(Ordering::Acquire)
    }

    /// Queue a snapshot for IPC. Returns zero when off, failed, or backpressured.
    pub fn submit(&self, job: InferJob) -> JobId {
        if !self.is_enabled() {
            return JobId(0);
        }
        let Some(transport) = &self.transport else {
            return JobId(0);
        };
        let id = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        let tick = job.tick;
        let Ok(mut pending) = self.pending.lock() else {
            self.disable();
            return JobId(0);
        };
        pending.insert(id, tick);
        match transport.requests.try_send((id, job)) {
            Ok(()) => id,
            Err(TrySendError::Full(_)) => {
                pending.remove(&id);
                JobId(0)
            }
            Err(TrySendError::Disconnected(_)) => {
                pending.remove(&id);
                drop(pending);
                self.disable();
                JobId(0)
            }
        }
    }

    /// Copy out ready intents without waiting for the child process.
    #[must_use]
    pub fn poll(&self, now: Tick, eval_slo_ticks: u16) -> InferPoll {
        if !self.is_enabled() {
            return InferPoll::default();
        }
        let Some(transport) = &self.transport else {
            return InferPoll::default();
        };
        let Ok(replies) = transport.replies.lock() else {
            self.disable();
            return InferPoll::default();
        };
        let Ok(mut pending) = self.pending.lock() else {
            self.disable();
            return InferPoll::default();
        };
        let cap = u64::from(eval_slo_ticks);
        let mut out = InferPoll::default();
        loop {
            match replies.try_recv() {
                Ok(Reply::Intent { id, intent }) => {
                    if let Some(submitted_tick) = pending.remove(&id) {
                        if now - submitted_tick <= cap {
                            out.intents.push(intent);
                        } else {
                            out.stale.push(RejectReason::StaleEpoch);
                        }
                    }
                }
                Ok(Reply::Failed) | Err(TryRecvError::Disconnected) => {
                    drop(pending);
                    drop(replies);
                    self.disable();
                    return out;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        pending.retain(|_, tick| {
            let fresh = now - *tick <= cap;
            if !fresh {
                out.stale.push(RejectReason::StaleEpoch);
            }
            fresh
        });
        out
    }

    fn disable(&self) {
        self.disabled.store(true, Ordering::Release);
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
    }
}

impl Default for InferHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InferHost {
    fn drop(&mut self) {
        if let Some(transport) = &self.transport
            && let Ok(mut child) = transport.child.lock()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Serve inference IPC on stdin/stdout until the parent closes the pipe.
///
/// A production model backend replaces `fill`; the process boundary and output
/// type remain unchanged.
pub fn run_sidecar() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    while let Some((id, _tick, snapshot)) = read_request(&mut input)? {
        let intent = fill(&snapshot);
        write_response(&mut output, id, &intent)?;
        output.flush()?;
    }
    Ok(())
}

fn writer_loop(
    mut output: impl Write,
    requests: Receiver<(JobId, InferJob)>,
    replies: mpsc::Sender<Reply>,
) {
    for (id, job) in requests {
        let result = job
            .snap
            .encode()
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, format!("snapshot: {e:?}")))
            .and_then(|bytes| write_request(&mut output, id, job.tick, &bytes))
            .and_then(|()| output.flush());
        if result.is_err() {
            let _ = replies.send(Reply::Failed);
            return;
        }
    }
}

fn reader_loop(mut input: impl Read, replies: mpsc::Sender<Reply>) {
    loop {
        match read_response(&mut input) {
            Ok(Some(reply)) => {
                if replies.send(reply).is_err() {
                    return;
                }
            }
            Ok(None) | Err(_) => {
                let _ = replies.send(Reply::Failed);
                return;
            }
        }
    }
}

fn fill(snapshot: &WorldSnapshot) -> InferIntent {
    InferIntent {
        model: ModelId(Name::from("stub-sidecar")),
        locus: snapshot.view().loci().next(),
        verb: Verb::Look,
        target: IntentTarget::None,
        claimed_facts: Vec::new(),
    }
}

fn write_request(out: &mut impl Write, id: JobId, tick: Tick, snapshot: &[u8]) -> io::Result<()> {
    let len = u32::try_from(snapshot.len())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "snapshot too large for IPC"))?;
    write_header(out, REQUEST)?;
    out.write_all(&id.0.to_le_bytes())?;
    out.write_all(&tick.0.to_le_bytes())?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(snapshot)
}

fn read_request(input: &mut impl Read) -> io::Result<Option<(JobId, Tick, WorldSnapshot)>> {
    if !read_header(input, REQUEST)? {
        return Ok(None);
    }
    let id = JobId(read_u64(input)?);
    let tick = Tick(read_u64(input)?);
    let len = read_u32(input)? as usize;
    klotho_world::check_snap_size(len)
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, format!("snapshot size: {e:?}")))?;
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    let snapshot = WorldSnapshot::decode(&bytes)
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, format!("snapshot: {e:?}")))?;
    if snapshot.tick != tick {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "infer request tick does not match snapshot",
        ));
    }
    Ok(Some((id, tick, snapshot)))
}

fn write_response(out: &mut impl Write, id: JobId, intent: &InferIntent) -> io::Result<()> {
    let text = to_ron(intent)
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, format!("intent: {e}")))?;
    let bytes = text.as_bytes();
    if bytes.len() > MAX_INTENT_BYTES {
        return Err(io::Error::new(ErrorKind::InvalidData, "intent IPC cap"));
    }
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "intent IPC length"))?;
    write_header(out, RESPONSE)?;
    out.write_all(&id.0.to_le_bytes())?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(bytes)
}

fn read_response(input: &mut impl Read) -> io::Result<Option<Reply>> {
    if !read_header(input, RESPONSE)? {
        return Ok(None);
    }
    let id = JobId(read_u64(input)?);
    let len = read_u32(input)? as usize;
    if len > MAX_INTENT_BYTES {
        return Err(io::Error::new(ErrorKind::InvalidData, "intent IPC cap"));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
    let intent: InferIntent = from_ron(text)
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, format!("intent: {e}")))?;
    intent
        .validate()
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, format!("intent: {e}")))?;
    Ok(Some(Reply::Intent { id, intent }))
}

fn write_header(out: &mut impl Write, kind: u8) -> io::Result<()> {
    out.write_all(&MAGIC)?;
    out.write_all(&[VERSION, kind, 0, 0])
}

fn read_header(input: &mut impl Read, expected_kind: u8) -> io::Result<bool> {
    let mut first = [0; 1];
    match input.read(&mut first)? {
        0 => return Ok(false),
        1 => {}
        _ => unreachable!(),
    }
    let mut rest = [0; 7];
    input.read_exact(&mut rest)?;
    let header = [
        first[0], rest[0], rest[1], rest[2], rest[3], rest[4], rest[5], rest[6],
    ];
    if header[..4] != MAGIC
        || header[4] != VERSION
        || header[5] != expected_kind
        || header[6..] != [0, 0]
    {
        return Err(io::Error::new(ErrorKind::InvalidData, "infer IPC header"));
    }
    Ok(true)
}

fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use klotho_core::{Budget, Tick};

    use super::*;

    #[test]
    fn default_is_infer_off() {
        let host = InferHost::new();
        assert!(!host.is_enabled());
        let p = InferHost::poll(&host, Tick(0), Budget::HEARTH.eval_slo_ticks);
        assert!(p.intents.is_empty());
        assert!(p.stale.is_empty());
    }
}
