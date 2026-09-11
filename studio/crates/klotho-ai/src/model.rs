//! Provider-neutral, size-capped model protocol and routing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::{Diagnostic, from_ron, to_ron};
use klotho_prove::hash_bytes;

use crate::agent::RequestId;
use crate::context::CompiledContext;
use crate::error::AiError;
use crate::ops::AuthorOp;
use crate::policy::{ContextClass, DisclosurePolicy};

/// Model capability used by the router.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    /// Planning and structured authoring.
    Reasoning,
    /// Image understanding.
    Vision,
    /// Image synthesis.
    Image,
    /// Geometry synthesis or critique.
    Geometry,
    /// Animation synthesis or critique.
    Animation,
    /// Audio or speech.
    Audio,
    /// Advisory critique.
    Critique,
}

/// Stable configured backend identity.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BackendId(pub String);

impl From<&str> for BackendId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Isolation boundary used for execution.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// Separate local process or service.
    Local,
    /// Approved remote service.
    Remote,
    /// Deterministic CI implementation.
    Fake,
}

/// Immutable backend registration.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSpec {
    /// Stable id.
    pub id: BackendId,
    /// Process/service boundary.
    pub kind: BackendKind,
    /// Exact backend/model lock hash.
    pub model_hash: Hash,
    /// Hash of sampling, tool, and context parameters.
    pub parameters_hash: Hash,
    /// Supported modalities.
    pub capabilities: BTreeSet<ModelCapability>,
    /// Secret-store key used by the host adapter, if needed.
    pub credential_key: Option<String>,
}

/// Schema-validated backend request.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendRequest {
    /// Request identity.
    pub request: RequestId,
    /// Untrusted natural-language request.
    pub prompt: String,
    /// Hash-bound project context.
    pub context: CompiledContext,
    /// Required model capability.
    pub capability: ModelCapability,
    /// Maximum output tokens.
    pub max_tokens: u64,
    /// Structured counterexamples from the trusted validator/evaluator. Empty
    /// for the initial proposal and populated only for a bounded repair.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

/// Schema-validated model response. Only typed operations cross the boundary.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendResponse {
    /// Proposed semantic operations.
    pub operations: Vec<AuthorOp>,
    /// Short review-facing note; never source of truth.
    pub summary: String,
    /// Accounted output tokens.
    pub output_tokens: u64,
    /// Accounted provider cost in millionths of a US dollar.
    pub cost_micro_usd: u64,
}

/// One immutable invocation record.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendRecord {
    /// Request.
    pub request: RequestId,
    /// Backend id.
    pub backend: BackendId,
    /// Locked model hash.
    pub model_hash: Hash,
    /// Exact invocation parameters.
    pub parameters_hash: Hash,
    /// Hash of encoded input.
    pub input_hash: Hash,
    /// Hash of encoded output, if completed.
    pub output_hash: Option<Hash>,
    /// Provider-reported/accounted output tokens.
    pub output_tokens: u64,
    /// Provider-reported/accounted cost.
    pub cost_micro_usd: u64,
    /// Terminal status.
    pub status: BackendStatus,
}

/// Invocation outcome.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendStatus {
    /// Valid response returned.
    Completed,
    /// Backend timed out.
    TimedOut,
    /// Transport or schema failure.
    Failed,
}

/// Isolated model implementation.
pub trait ModelBackend {
    /// Registration record.
    fn spec(&self) -> &BackendSpec;
    /// Invoke with already policy-filtered input.
    fn invoke(&mut self, request: &BackendRequest) -> Result<BackendResponse, AiError>;
}

/// Byte transport implemented by a local process client or approved remote SDK.
pub trait BackendTransport {
    /// Exchange one framed payload. Implementations own process/network details.
    fn exchange(&mut self, payload: &[u8]) -> Result<Vec<u8>, String>;
}

/// Local/remote protocol adapter. RON framing is deny-unknown-fields and capped
/// on both sides; transport errors cannot mutate a transaction.
pub struct ProtocolAdapter<T> {
    spec: BackendSpec,
    transport: T,
    max_request_bytes: u64,
    max_response_bytes: u64,
}

impl<T> ProtocolAdapter<T> {
    /// Construct an isolated adapter.
    #[must_use]
    pub fn new(
        spec: BackendSpec,
        transport: T,
        max_request_bytes: u64,
        max_response_bytes: u64,
    ) -> Self {
        Self {
            spec,
            transport,
            max_request_bytes,
            max_response_bytes,
        }
    }
}

impl<T: BackendTransport> ModelBackend for ProtocolAdapter<T> {
    fn spec(&self) -> &BackendSpec {
        &self.spec
    }

    fn invoke(&mut self, request: &BackendRequest) -> Result<BackendResponse, AiError> {
        let encoded = to_ron(request).map_err(|e| AiError::BackendProtocol(e.to_string()))?;
        if encoded.len() as u64 > self.max_request_bytes {
            return Err(AiError::BackendPayload);
        }
        let response = self
            .transport
            .exchange(encoded.as_bytes())
            .map_err(AiError::BackendTransport)?;
        if response.len() as u64 > self.max_response_bytes {
            return Err(AiError::BackendPayload);
        }
        let text =
            std::str::from_utf8(&response).map_err(|e| AiError::BackendProtocol(e.to_string()))?;
        from_ron(text).map_err(|e| AiError::BackendProtocol(e.to_string()))
    }
}

/// Deterministic, queue-backed backend for CI.
pub struct DeterministicFakeBackend {
    spec: BackendSpec,
    responses: VecDeque<Result<BackendResponse, AiError>>,
}

impl DeterministicFakeBackend {
    /// Create a fake reasoning backend.
    #[must_use]
    pub fn new(id: &str, responses: Vec<Result<BackendResponse, AiError>>) -> Self {
        Self {
            spec: BackendSpec {
                id: BackendId::from(id),
                kind: BackendKind::Fake,
                model_hash: hash_bytes(id.as_bytes()),
                parameters_hash: hash_bytes(b"deterministic-fake-v1"),
                capabilities: [ModelCapability::Reasoning].into_iter().collect(),
                credential_key: None,
            },
            responses: responses.into(),
        }
    }
}

impl ModelBackend for DeterministicFakeBackend {
    fn spec(&self) -> &BackendSpec {
        &self.spec
    }

    fn invoke(&mut self, _request: &BackendRequest) -> Result<BackendResponse, AiError> {
        self.responses
            .pop_front()
            .unwrap_or_else(|| Err(AiError::BackendTransport("fake queue exhausted".into())))
    }
}

/// Capability router and immutable invocation ledger.
#[derive(Default)]
pub struct ModelRouter {
    backends: BTreeMap<BackendId, Box<dyn ModelBackend>>,
    records: Vec<BackendRecord>,
}

impl ModelRouter {
    /// Register or replace a backend by stable id.
    pub fn register(&mut self, backend: impl ModelBackend + 'static) {
        self.backends
            .insert(backend.spec().id.clone(), Box::new(backend));
    }

    /// Registered backend specs in stable id order.
    #[must_use]
    pub fn specs(&self) -> Vec<BackendSpec> {
        self.backends.values().map(|b| b.spec().clone()).collect()
    }

    /// Invocation ledger.
    #[must_use]
    pub fn records(&self) -> &[BackendRecord] {
        &self.records
    }

    /// Route and invoke. `preferred` is honored only if policy and capability permit it.
    pub fn invoke(
        &mut self,
        request: &BackendRequest,
        disclosure: &DisclosurePolicy,
        preferred: Option<&BackendId>,
    ) -> Result<(BackendId, BackendResponse), AiError> {
        let id = self.select(request.capability, disclosure, preferred)?;
        let backend = self.backends.get_mut(&id).ok_or(AiError::NoBackend)?;
        let input = to_ron(request).map_err(|e| AiError::BackendProtocol(e.to_string()))?;
        let input_hash = hash_bytes(input.as_bytes());
        let result = backend.invoke(request);
        let (status, output_hash) = match &result {
            Ok(output) => {
                let bytes = to_ron(output).map_err(|e| AiError::BackendProtocol(e.to_string()))?;
                (BackendStatus::Completed, Some(hash_bytes(bytes.as_bytes())))
            }
            Err(AiError::Timeout) => (BackendStatus::TimedOut, None),
            Err(_) => (BackendStatus::Failed, None),
        };
        self.records.push(BackendRecord {
            request: request.request,
            backend: id.clone(),
            model_hash: backend.spec().model_hash,
            parameters_hash: backend.spec().parameters_hash,
            input_hash,
            output_hash,
            output_tokens: result.as_ref().map_or(0, |output| output.output_tokens),
            cost_micro_usd: result.as_ref().map_or(0, |output| output.cost_micro_usd),
            status,
        });
        result.map(|response| (id, response))
    }

    fn select(
        &self,
        capability: ModelCapability,
        disclosure: &DisclosurePolicy,
        preferred: Option<&BackendId>,
    ) -> Result<BackendId, AiError> {
        let permitted = |backend: &dyn ModelBackend| {
            backend.spec().capabilities.contains(&capability)
                && (backend.spec().kind != BackendKind::Remote
                    || [
                        ContextClass::Schema,
                        ContextClass::ProjectStructure,
                        ContextClass::ApprovedMemory,
                        ContextClass::RequestText,
                    ]
                    .into_iter()
                    .all(|class| disclosure.permits_remote(class)))
        };
        if let Some(id) = preferred {
            if self
                .backends
                .get(id)
                .is_some_and(|backend| permitted(backend.as_ref()))
            {
                return Ok(id.clone());
            }
        }
        self.backends
            .iter()
            .find(|(_, backend)| permitted(backend.as_ref()))
            .map(|(id, _)| id.clone())
            .ok_or(AiError::NoBackend)
    }
}
