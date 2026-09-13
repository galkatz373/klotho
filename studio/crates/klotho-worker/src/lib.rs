//! Privileged typed DCC/media worker broker (K82).
//!
//! Models can submit only [`WorkerJob`] values. They never receive an
//! executable, shell, host path, credential, device, or ambient network
//! capability. Platform runners must enforce the returned [`SandboxProfile`]
//! and are deliberately kept behind [`WorkerRunner`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use klotho_core::{BlobId, Hash};
use klotho_dcc::{AssetRequestId, DccApplication, PipelineLock};
use klotho_prove::{ArtifactKind, blob_id_of, hash_bytes};
use serde::{Deserialize, Serialize};

/// Maximum staged files in one job.
pub const MAX_JOB_FILES: usize = 4_096;
/// Maximum recursively expanded archive entries.
pub const MAX_ARCHIVE_ENTRIES: usize = 16_384;
/// Maximum archive expansion ratio.
pub const MAX_ARCHIVE_RATIO: u64 = 20;

/// Stable registered worker id.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerId(pub String);

/// Closed worker operation. There is no arbitrary executable/shell operation.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerOperation {
    /// Export a DCC scene to a locked interchange format.
    ExportScene,
    /// Build/repair visual LODs.
    BuildLods,
    /// Retarget animation/mocap.
    Retarget,
    /// Bake materials/textures.
    BakeMaterial,
    /// Encode bounded media.
    EncodeMedia,
    /// Produce fixed neutral reference captures.
    NeutralCapture,
}

/// Network policy attached to a registered worker.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkPolicy {
    /// No network namespace/entitlement.
    Deny,
    /// Exact administrator-approved endpoints.
    Allow(BTreeSet<String>),
}

/// A worker consumes/produces only job-root-relative logical mounts.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountClass {
    /// Read-only staged inputs.
    Input,
    /// Write-only collected outputs.
    Output,
    /// Read-only pinned application bundle.
    ToolBundle,
}

/// Administrator-signed worker declaration.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerSpec {
    /// Registry id.
    pub id: WorkerId,
    /// DCC/media family.
    pub application: DccApplication,
    /// Exact executable hash.
    pub executable_hash: Hash,
    /// Exact container/application bundle hash.
    pub bundle_hash: Hash,
    /// Supported closed operations.
    pub operations: BTreeSet<WorkerOperation>,
    /// Named typed arguments accepted by the adapter.
    pub argument_fields: BTreeSet<String>,
    /// Mounted classes.
    pub mounts: BTreeSet<MountClass>,
    /// Egress policy.
    pub network: NetworkPolicy,
    /// CPU time cap.
    pub cpu_ms: u64,
    /// Address-space cap.
    pub memory_bytes: u64,
    /// Output byte cap.
    pub output_bytes: u64,
    /// Child-process cap. One includes the worker itself.
    pub processes: u16,
    /// Pipeline lock hash this worker was qualified against.
    pub pipeline_lock: Hash,
    /// Output parser identity; must differ from worker identity.
    pub parser_identity: String,
}

/// Signed canonical registry payload.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedRegistry {
    /// Monotonic registry version.
    pub version: u64,
    /// Ordered worker specs.
    pub workers: BTreeMap<WorkerId, WorkerSpec>,
    /// Ed25519 signature over [`Self::signing_bytes`].
    pub signature: Vec<u8>,
}

impl SignedRegistry {
    /// Canonical bytes signed by the security owner.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, WorkerError> {
        ron::ser::to_string(&(self.version, &self.workers))
            .map(String::into_bytes)
            .map_err(|error| WorkerError::Registry(error.to_string()))
    }

    /// Verify signature and every fail-closed worker invariant.
    pub fn verify(&self, key: &VerifyingKey) -> Result<(), WorkerError> {
        let signature = Signature::try_from(self.signature.as_slice())
            .map_err(|_| WorkerError::Registry("invalid registry signature bytes".into()))?;
        key.verify(&self.signing_bytes()?, &signature)
            .map_err(|_| WorkerError::Registry("registry signature rejected".into()))?;
        if self.workers.is_empty() {
            return Err(WorkerError::Registry("empty registry".into()));
        }
        for (id, spec) in &self.workers {
            if id != &spec.id
                || id.0.trim().is_empty()
                || spec.executable_hash == Hash::ZERO
                || spec.bundle_hash == Hash::ZERO
                || spec.pipeline_lock == Hash::ZERO
                || spec.operations.is_empty()
                || spec.cpu_ms == 0
                || spec.memory_bytes == 0
                || spec.output_bytes == 0
                || spec.processes == 0
                || spec.parser_identity.trim().is_empty()
                || spec.parser_identity == id.0
                || !spec.mounts.contains(&MountClass::Input)
                || !spec.mounts.contains(&MountClass::Output)
                || !spec.mounts.contains(&MountClass::ToolBundle)
            {
                return Err(WorkerError::Registry(format!("invalid worker {}", id.0)));
            }
            if let NetworkPolicy::Allow(endpoints) = &spec.network
                && (endpoints.is_empty() || endpoints.iter().any(|e| !valid_endpoint(e)))
            {
                return Err(WorkerError::Registry("invalid egress allowlist".into()));
            }
        }
        Ok(())
    }
}

/// Staged input. `relative_name` is under the fresh input mount only.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedInput {
    /// Safe relative logical name.
    pub relative_name: String,
    /// Exact content hash.
    pub content: Hash,
    /// Bytes staged.
    pub bytes: u64,
}

/// Typed job submitted by the AI asset tool or a human operator.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerJob {
    /// Worker registry id.
    pub worker: WorkerId,
    /// Asset request.
    pub request: AssetRequestId,
    /// Closed operation.
    pub operation: WorkerOperation,
    /// Schema fields, interpreted structurally by a pinned adapter, never a shell.
    pub arguments: BTreeMap<String, String>,
    /// Immutable staged inputs.
    pub inputs: Vec<StagedInput>,
    /// Optional endpoint; must match the spec allowlist exactly.
    pub endpoint: Option<String>,
}

/// Supported OS sandbox implementation.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum WorkerOs {
    /// Linux namespaces/cgroup/seccomp runner.
    Linux,
    /// Windows AppContainer/restricted token + Job Object runner.
    Windows,
    /// macOS signed sandbox profile + per-job container runner.
    MacOs,
}

/// Required OS-enforced profile. Missing any required control is a broker error.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SandboxProfile {
    /// Target OS.
    pub os: WorkerOs,
    /// Fresh unprivileged identity/container.
    pub fresh_identity: bool,
    /// Read-only root/application bundle.
    pub read_only_root: bool,
    /// No inherited environment or credentials.
    pub clear_environment: bool,
    /// Host IPC denied.
    pub deny_host_ipc: bool,
    /// Device access denied.
    pub deny_devices: bool,
    /// Process-tree cap enforced by OS.
    pub process_limit: u16,
    /// CPU cap.
    pub cpu_ms: u64,
    /// Memory cap.
    pub memory_bytes: u64,
    /// Network namespace/entitlement policy.
    pub network: NetworkPolicy,
}

impl SandboxProfile {
    fn for_spec(os: WorkerOs, spec: &WorkerSpec) -> Self {
        Self {
            os,
            fresh_identity: true,
            read_only_root: true,
            clear_environment: true,
            deny_host_ipc: true,
            deny_devices: true,
            process_limit: spec.processes,
            cpu_ms: spec.cpu_ms,
            memory_bytes: spec.memory_bytes,
            network: spec.network.clone(),
        }
    }
}

/// Output declared by the worker and revalidated by another parser identity.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WorkerOutput {
    /// Safe job-root-relative output name.
    pub relative_name: String,
    /// Declared artifact kind.
    pub kind: ArtifactKind,
    /// Collected bytes.
    pub bytes: Vec<u8>,
    /// Hash declared by the untrusted worker.
    pub declared_blob: BlobId,
}

/// Validated worker result.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WorkerResult {
    /// Validated content-addressed outputs.
    pub outputs: Vec<(ArtifactKind, BlobId)>,
    /// Hash of bounded worker logs.
    pub log_hash: Hash,
}

/// Platform implementation. It receives an exact registered worker and a
/// mandatory sandbox profile; it never receives a command string.
pub trait WorkerRunner {
    /// Run one fresh disposable job.
    fn run(
        &self,
        spec: &WorkerSpec,
        profile: &SandboxProfile,
        job: &WorkerJob,
    ) -> Result<(Vec<WorkerOutput>, Vec<u8>), WorkerError>;
}

/// Verified broker.
pub struct WorkerBroker {
    registry: SignedRegistry,
    os: WorkerOs,
}

impl WorkerBroker {
    /// Open only from an administrator-signed registry.
    pub fn open(
        registry: SignedRegistry,
        administrator: &VerifyingKey,
        os: WorkerOs,
    ) -> Result<Self, WorkerError> {
        registry.verify(administrator)?;
        Ok(Self { registry, os })
    }

    /// Validate, run under the mandatory profile, then collect and hash
    /// outputs. Actual format parsing belongs to the separately identified
    /// parser process selected by the platform runner.
    pub fn submit(
        &self,
        job: &WorkerJob,
        pipeline: &PipelineLock,
        runner: &dyn WorkerRunner,
    ) -> Result<WorkerResult, WorkerError> {
        let spec = self
            .registry
            .workers
            .get(&job.worker)
            .ok_or_else(|| WorkerError::Policy("unregistered worker".into()))?;
        if pipeline
            .validate_and_hash()
            .map_err(|error| WorkerError::Policy(error.to_string()))?
            != spec.pipeline_lock
        {
            return Err(WorkerError::Policy("pipeline lock mismatch".into()));
        }
        validate_job(spec, job)?;
        let profile = SandboxProfile::for_spec(self.os, spec);
        let (outputs, logs) = runner.run(spec, &profile, job)?;
        if logs.len() as u64 > 4 * 1024 * 1024 {
            return Err(WorkerError::Output("worker log cap exceeded".into()));
        }
        let total = outputs.iter().try_fold(0u64, |sum, output| {
            sum.checked_add(output.bytes.len() as u64)
                .ok_or_else(|| WorkerError::Output("output size overflow".into()))
        })?;
        if total > spec.output_bytes || outputs.len() > MAX_JOB_FILES {
            return Err(WorkerError::Output("worker output cap exceeded".into()));
        }
        let mut validated = Vec::with_capacity(outputs.len());
        for output in outputs {
            validate_relative(&output.relative_name)?;
            let parsed_kind = klotho_compile::peek_kind(&output.bytes)
                .map_err(|error| WorkerError::Output(format!("isolated parser: {error}")))?;
            if parsed_kind != output.kind {
                return Err(WorkerError::Output("output kind mismatch".into()));
            }
            klotho_compile::validate_blob(&output.bytes)
                .map_err(|error| WorkerError::Output(format!("isolated parser: {error}")))?;
            let actual = blob_id_of(&output.bytes);
            if actual != output.declared_blob {
                return Err(WorkerError::Output("forged output manifest".into()));
            }
            validated.push((output.kind, actual));
        }
        validated.sort_by_key(|(kind, blob)| (*kind, *blob));
        Ok(WorkerResult {
            outputs: validated,
            log_hash: hash_bytes(&logs),
        })
    }
}

fn validate_job(spec: &WorkerSpec, job: &WorkerJob) -> Result<(), WorkerError> {
    if !spec.operations.contains(&job.operation) || job.inputs.len() > MAX_JOB_FILES {
        return Err(WorkerError::Policy("operation/input cap rejected".into()));
    }
    for (key, value) in &job.arguments {
        if !spec.argument_fields.contains(key)
            || value.len() > 4_096
            || value.contains('\0')
            || value.contains('\n')
            || value.contains('\r')
        {
            return Err(WorkerError::Policy(
                "unregistered/unsafe typed argument".into(),
            ));
        }
    }
    for input in &job.inputs {
        validate_relative(&input.relative_name)?;
        if input.content == Hash::ZERO || input.bytes == 0 {
            return Err(WorkerError::Policy("invalid staged input".into()));
        }
    }
    match (&spec.network, &job.endpoint) {
        (NetworkPolicy::Deny, None) | (NetworkPolicy::Allow(_), None) => {}
        (NetworkPolicy::Allow(allowed), Some(endpoint)) if allowed.contains(endpoint) => {}
        _ => return Err(WorkerError::Policy("network endpoint rejected".into())),
    }
    Ok(())
}

fn valid_endpoint(endpoint: &str) -> bool {
    (endpoint.starts_with("https://") || endpoint.starts_with("grpcs://"))
        && !endpoint.contains('@')
        && !endpoint.contains(char::is_whitespace)
}

fn validate_relative(path: &str) -> Result<(), WorkerError> {
    if path.is_empty()
        || path.len() > 240
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\0')
        || path
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
        || (path.len() >= 2 && path.as_bytes()[1] == b':')
    {
        return Err(WorkerError::Policy(
            "path traversal/link target rejected".into(),
        ));
    }
    Ok(())
}

/// Validate archive expansion before extraction. Links are never accepted.
pub fn validate_archive(
    compressed_bytes: u64,
    entries: &[(String, u64, bool)],
) -> Result<(), WorkerError> {
    if compressed_bytes == 0 || entries.len() > MAX_ARCHIVE_ENTRIES {
        return Err(WorkerError::Policy("archive cap rejected".into()));
    }
    let mut expanded = 0u64;
    for (path, bytes, is_link) in entries {
        validate_relative(path)?;
        if *is_link {
            return Err(WorkerError::Policy("archive link rejected".into()));
        }
        expanded = expanded
            .checked_add(*bytes)
            .ok_or_else(|| WorkerError::Policy("archive size overflow".into()))?;
    }
    if expanded > compressed_bytes.saturating_mul(MAX_ARCHIVE_RATIO) {
        return Err(WorkerError::Policy(
            "archive expansion ratio rejected".into(),
        ));
    }
    Ok(())
}

/// Broker error. Jobs fail closed and never weaken the sandbox profile.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum WorkerError {
    /// Signed registry invalid.
    Registry(String),
    /// Job/capability/path/network policy invalid.
    Policy(String),
    /// Platform runner failed or sandbox unavailable.
    Runner(String),
    /// Output validation failed.
    Output(String),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(message) => write!(f, "WorkerRegistry({message})"),
            Self::Policy(message) => write!(f, "WorkerPolicy({message})"),
            Self::Runner(message) => write!(f, "WorkerRunner({message})"),
            Self::Output(message) => write!(f, "WorkerOutput({message})"),
        }
    }
}

impl core::error::Error for WorkerError {}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use klotho_dcc::{InterchangeFormat, ToolPin};

    use super::*;

    fn hash(byte: u8) -> Hash {
        Hash([byte; 32])
    }

    fn lock() -> PipelineLock {
        PipelineLock {
            interchange: [(InterchangeFormat::Gltf, "2.0".into())].into(),
            ocio_config: hash(1),
            tools: [(
                DccApplication::Blender,
                vec![ToolPin {
                    id: "blender".into(),
                    version: "4.5.3".into(),
                    hash: hash(2),
                }],
            )]
            .into(),
            skeletons: BTreeSet::new(),
            compressors: [hash(3)].into(),
        }
    }

    fn registry(signing: &SigningKey) -> SignedRegistry {
        let pipeline_lock = lock().validate_and_hash().unwrap();
        let spec = WorkerSpec {
            id: WorkerId("blender.export.v1".into()),
            application: DccApplication::Blender,
            executable_hash: hash(4),
            bundle_hash: hash(5),
            operations: [WorkerOperation::ExportScene].into(),
            argument_fields: ["format".into()].into(),
            mounts: [
                MountClass::Input,
                MountClass::Output,
                MountClass::ToolBundle,
            ]
            .into(),
            network: NetworkPolicy::Deny,
            cpu_ms: 30_000,
            memory_bytes: 2 * 1024 * 1024 * 1024,
            output_bytes: 1024 * 1024,
            processes: 4,
            pipeline_lock,
            parser_identity: "gltf-validator".into(),
        };
        let mut registry = SignedRegistry {
            version: 1,
            workers: [(spec.id.clone(), spec)].into(),
            signature: Vec::new(),
        };
        registry.signature = signing
            .sign(&registry.signing_bytes().unwrap())
            .to_bytes()
            .to_vec();
        registry
    }

    fn job() -> WorkerJob {
        WorkerJob {
            worker: WorkerId("blender.export.v1".into()),
            request: AssetRequestId(hash(9)),
            operation: WorkerOperation::ExportScene,
            arguments: [("format".into(), "gltf".into())].into(),
            inputs: vec![StagedInput {
                relative_name: "scene/source.blend".into(),
                content: hash(8),
                bytes: 100,
            }],
            endpoint: None,
        }
    }

    struct FakeRunner;

    impl WorkerRunner for FakeRunner {
        fn run(
            &self,
            _: &WorkerSpec,
            profile: &SandboxProfile,
            _: &WorkerJob,
        ) -> Result<(Vec<WorkerOutput>, Vec<u8>), WorkerError> {
            assert!(profile.fresh_identity && profile.clear_environment && profile.deny_devices);
            let bytes =
                klotho_compile::encode_mesh_i16(&[[0, 0, 0], [10, 0, 0], [0, 10, 0]], &[0, 1, 2])
                    .unwrap();
            Ok((
                vec![WorkerOutput {
                    relative_name: "export/model.gltf".into(),
                    kind: ArtifactKind::ClusteredMesh,
                    declared_blob: blob_id_of(&bytes),
                    bytes,
                }],
                b"ok".to_vec(),
            ))
        }
    }

    #[test]
    fn signed_registry_and_all_os_profiles_reach_validated_cas_ids() {
        let signing = SigningKey::from_bytes(&[7; 32]);
        for os in [WorkerOs::Linux, WorkerOs::Windows, WorkerOs::MacOs] {
            let broker =
                WorkerBroker::open(registry(&signing), &signing.verifying_key(), os).unwrap();
            let result = broker.submit(&job(), &lock(), &FakeRunner).unwrap();
            assert_eq!(result.outputs.len(), 1);
        }
    }

    #[test]
    fn hostile_requests_fail_before_runner() {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let broker = WorkerBroker::open(
            registry(&signing),
            &signing.verifying_key(),
            WorkerOs::Linux,
        )
        .unwrap();
        for bad in [
            "../secret",
            "/etc/passwd",
            "C:\\secret",
            "a//b",
            "link/../escape",
        ] {
            let mut hostile = job();
            hostile.inputs[0].relative_name = bad.into();
            assert!(matches!(
                broker.submit(&hostile, &lock(), &FakeRunner),
                Err(WorkerError::Policy(_))
            ));
        }
        let mut injection = job();
        injection
            .arguments
            .insert("shell".into(), "curl attacker".into());
        assert!(broker.submit(&injection, &lock(), &FakeRunner).is_err());
        let mut network = job();
        network.endpoint = Some("https://attacker.invalid".into());
        assert!(broker.submit(&network, &lock(), &FakeRunner).is_err());
    }

    #[test]
    fn registry_tamper_archive_bombs_and_links_fail_closed() {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let mut tampered = registry(&signing);
        tampered.version = 2;
        assert!(tampered.verify(&signing.verifying_key()).is_err());
        assert!(validate_archive(10, &[("safe.bin".into(), 201, false)]).is_err());
        assert!(validate_archive(100, &[("link".into(), 1, true)]).is_err());
        assert!(validate_archive(100, &[("../escape".into(), 1, false)]).is_err());
    }

    struct ForgingRunner;

    impl WorkerRunner for ForgingRunner {
        fn run(
            &self,
            _: &WorkerSpec,
            _: &SandboxProfile,
            _: &WorkerJob,
        ) -> Result<(Vec<WorkerOutput>, Vec<u8>), WorkerError> {
            Ok((
                vec![WorkerOutput {
                    relative_name: "out/model.gltf".into(),
                    kind: ArtifactKind::ClusteredMesh,
                    bytes: vec![1],
                    declared_blob: BlobId::ZERO,
                }],
                Vec::new(),
            ))
        }
    }

    #[test]
    fn forged_manifest_is_rejected_after_worker_exit() {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let broker = WorkerBroker::open(
            registry(&signing),
            &signing.verifying_key(),
            WorkerOs::Linux,
        )
        .unwrap();
        assert!(matches!(
            broker.submit(&job(), &lock(), &ForgingRunner),
            Err(WorkerError::Output(_))
        ));
    }
}
