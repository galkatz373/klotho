//! Standard editor AI service tying indexing, models, tools, transactions, and evidence together.

use std::path::Path;
use std::time::Instant;

use klotho_canon::Canon;
use klotho_core::Hash;
use klotho_schema::{SchemaCatalog, generate};

use crate::agent::{
    AgentScheduler, AiProgress, CreativeRequest, RequestId, RequestState, request_hash,
};
use crate::context::{ContextBuilder, ContextRequest};
use crate::diff::SemanticDiff;
use crate::error::AiError;
use crate::evaluation::EvaluationBroker;
use crate::index::SemanticProjectIndex;
use crate::memory::ProjectMemory;
use crate::model::{BackendId, BackendRequest, ModelRouter};
use crate::policy::{ExecutionPolicy, SecretStore};
use crate::store::TransactionStore;
use crate::tools::{ToolCall, ToolEnvironment, ToolRegistry, ToolResult};

/// Review-facing candidate. It describes meaning and evidence; it cannot merge.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReviewPackage {
    /// Semantic diff.
    pub diff: SemanticDiff,
    /// Sealed evidence hashes registered for this change/project.
    pub evidence: Vec<Hash>,
}

/// Built-in Klotho authoring intelligence. This type belongs only to the studio
/// workspace; the engine/game workspace has no dependency path to it.
pub struct KlothoAi {
    /// Generated authoring schema.
    pub catalog: SchemaCatalog,
    /// Exact structural project index.
    pub project: SemanticProjectIndex,
    /// Provider-neutral model router.
    pub models: ModelRouter,
    /// Request and semantic-ownership scheduler.
    pub agents: AgentScheduler,
    /// Closed typed tool registry.
    pub tools: ToolRegistry,
    /// Isolated authoring transactions.
    pub transactions: TransactionStore,
    /// Trusted evidence broker.
    pub evaluation: EvaluationBroker,
    /// Human-approved persistent memory.
    pub memory: ProjectMemory,
    /// Host-owned credentials, inaccessible to tools/context.
    pub secrets: SecretStore,
    /// Capability and payload caps.
    pub policy: ExecutionPolicy,
}

impl KlothoAi {
    /// Open the standard service using the generated base catalog.
    pub fn new(workspace: &Path, base: &Path) -> Result<Self, AiError> {
        Self::with_catalog(workspace, base, generate(&Canon::default()))
    }

    /// Open with a project-specific generated catalog.
    pub fn with_catalog(
        workspace: &Path,
        base: &Path,
        catalog: SchemaCatalog,
    ) -> Result<Self, AiError> {
        let transactions = TransactionStore::open(workspace, base)?;
        let snapshot = transactions.base_snapshot()?;
        let project = SemanticProjectIndex::build(&snapshot, catalog.version)?;
        let memory = ProjectMemory::open(&workspace.join("approved-memory.ron"))?;
        Ok(Self {
            catalog,
            project,
            models: ModelRouter::default(),
            agents: AgentScheduler::default(),
            tools: ToolRegistry,
            transactions,
            evaluation: EvaluationBroker::default(),
            memory,
            secrets: SecretStore::default(),
            policy: ExecutionPolicy::default(),
        })
    }

    /// Enqueue a request. Work begins when [`Self::drive`] is called.
    pub fn request(&mut self, request: CreativeRequest) -> Result<RequestId, AiError> {
        self.agents.enqueue(request)
    }

    /// Execute one bounded request stage synchronously. A crashed backend leaves
    /// the transaction isolated and the request resumable.
    pub fn drive(&mut self, id: RequestId, now_ms: u64) -> Result<AiProgress, AiError> {
        let record = self
            .agents
            .get(id)
            .cloned()
            .ok_or(AiError::UnknownRequest(id))?;
        if !self.agents.dependencies_ready(id)? {
            return Err(AiError::RequestState(
                "dependencies are not candidates".into(),
            ));
        }
        if !matches!(record.state, RequestState::Queued | RequestState::Running) {
            return Err(AiError::RequestState(format!("{:?}", record.state)));
        }
        if let Some(row) = self.agents.get_mut(id) {
            row.state = RequestState::Running;
            row.active_since_ms.get_or_insert(now_ms);
        }

        let scope = record.request.scope.clone();
        let context = ContextBuilder::compile(
            &self.project,
            &self.catalog,
            &self.memory,
            &ContextRequest {
                anchors: scope
                    .anchors
                    .iter()
                    .chain(scope.modules.iter())
                    .copied()
                    .collect(),
                max_entries: 512,
                include_memory: true,
            },
        );
        let backend_request = BackendRequest {
            request: id,
            prompt: record.request.text.clone(),
            context,
            capability: record.request.model_capability,
            max_tokens: record.request.budget.tokens,
        };
        let encoded_request = klotho_ir::to_ron(&backend_request)
            .map_err(|error| AiError::BackendProtocol(error.to_string()))?;
        if encoded_request.len() as u64 > self.policy.max_request_bytes {
            self.fail_request(id, AiError::BackendPayload);
            return Err(AiError::BackendPayload);
        }
        let started = Instant::now();
        let invoke = self.models.invoke(
            &backend_request,
            &record.request.disclosure,
            record.request.preferred_backend.as_ref(),
        );
        let (backend, response) = match invoke {
            Ok(value) => value,
            Err(AiError::Timeout) => {
                if let Some(row) = self.agents.get_mut(id) {
                    row.state = RequestState::TimedOut;
                    row.summary = "backend timed out; transaction is resumable".into();
                }
                return self.poll(id);
            }
            Err(error) => {
                if let Some(row) = self.agents.get_mut(id) {
                    row.state = RequestState::Failed(error.to_string());
                }
                self.agents.release(id);
                return Err(error);
            }
        };

        let encoded_response = klotho_ir::to_ron(&response)
            .map_err(|error| AiError::BackendProtocol(error.to_string()))?;
        if encoded_response.len() as u64 > self.policy.max_response_bytes {
            self.fail_request(id, AiError::BackendPayload);
            return Err(AiError::BackendPayload);
        }

        let wall_ms = u64::try_from(started.elapsed().as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        let artifacts = u32::try_from(
            response
                .operations
                .iter()
                .filter(|operation| matches!(operation, crate::ops::AuthorOp::BindAsset { .. }))
                .count(),
        )
        .unwrap_or(u32::MAX);
        if response.output_tokens > record.request.budget.tokens
            || response.cost_micro_usd > record.request.budget.micro_usd
            || wall_ms > record.request.budget.wall_ms
            || record.request.budget.tool_calls < 3
            || artifacts > record.request.budget.artifacts
        {
            self.fail_request(id, AiError::RequestBudget);
            return Err(AiError::RequestBudget);
        }
        let profile = self
            .policy
            .profiles
            .get(&record.request.role)
            .cloned()
            .ok_or(AiError::CapabilityDenied(
                crate::policy::Capability::ChangeCreate,
            ))?;
        let created = ToolRegistry::execute(
            &profile,
            ToolCall::ChangeCreate {
                base: self.project.project_hash,
                scope: scope.clone(),
                budget: record.request.transaction_budget.clone(),
            },
            self.tool_env(),
        )?;
        let ToolResult::Created {
            transaction,
            change,
        } = created
        else {
            unreachable!("closed tool result")
        };
        let apply = ToolRegistry::execute(
            &profile,
            ToolCall::ChangeApply {
                transaction,
                operations: response.operations,
            },
            self.tool_env(),
        );
        if let Err(error) = apply {
            let _ = self.transactions.cancel(transaction);
            self.fail_request(id, error.clone());
            return Err(error);
        }
        let validation = ToolRegistry::execute(
            &profile,
            ToolCall::ValidateRun { transaction },
            self.tool_env(),
        );
        let validation = match validation {
            Ok(value) => value,
            Err(error) => {
                let _ = self.transactions.cancel(transaction);
                self.fail_request(id, error.clone());
                return Err(error);
            }
        };
        if let ToolResult::Diagnostics(diagnostics) = validation {
            if !diagnostics.is_empty() {
                let error = AiError::RequestState(diagnostics[0].to_string());
                self.fail_request(id, error.clone());
                return Err(error);
            }
        }
        if let Some(row) = self.agents.get_mut(id) {
            row.state = RequestState::Candidate;
            row.transaction = Some(transaction);
            row.change = Some(change);
            row.backend = Some(backend);
            row.usage.wall_ms = wall_ms;
            row.usage.tokens = response.output_tokens;
            row.usage.micro_usd = response.cost_micro_usd;
            row.usage.tool_calls = 3;
            row.usage.artifacts = artifacts;
            row.summary = response.summary;
            if row.usage.exceeds(&row.request.budget) {
                row.state = RequestState::Failed("request budget exhausted".into());
            }
        }
        self.poll(id)
    }

    /// Poll without advancing work.
    pub fn poll(&self, id: RequestId) -> Result<AiProgress, AiError> {
        let row = self.agents.get(id).ok_or(AiError::UnknownRequest(id))?;
        Ok(AiProgress {
            request: id,
            state: row.state.clone(),
            usage: row.usage.clone(),
            change: row.change,
            summary: row.summary.clone(),
        })
    }

    /// Pause queued/running work without discarding its transaction.
    pub fn pause(&mut self, id: RequestId) -> Result<(), AiError> {
        let row = self.agents.get_mut(id).ok_or(AiError::UnknownRequest(id))?;
        if matches!(row.state, RequestState::Queued | RequestState::Running) {
            row.state = RequestState::Paused;
            Ok(())
        } else {
            Err(AiError::RequestState(format!("{:?}", row.state)))
        }
    }

    /// Resume paused/timed-out work, optionally switching to another registered backend.
    pub fn resume(&mut self, id: RequestId, backend: Option<BackendId>) -> Result<(), AiError> {
        let row = self.agents.get_mut(id).ok_or(AiError::UnknownRequest(id))?;
        if !matches!(row.state, RequestState::Paused | RequestState::TimedOut) {
            return Err(AiError::RequestState(format!("{:?}", row.state)));
        }
        row.request.preferred_backend = backend;
        row.state = RequestState::Queued;
        row.active_since_ms = None;
        Ok(())
    }

    /// Cancel request and isolated transaction. The live project is untouched.
    pub fn cancel(&mut self, id: RequestId) -> Result<(), AiError> {
        let transaction = self
            .agents
            .get(id)
            .ok_or(AiError::UnknownRequest(id))?
            .transaction;
        if let Some(transaction) = transaction {
            self.transactions.cancel(transaction)?;
        }
        if let Some(row) = self.agents.get_mut(id) {
            row.state = RequestState::Cancelled;
        }
        self.agents.release(id);
        Ok(())
    }

    /// Review a candidate by change id. Acceptance/merge belongs to Distaff KAI-08.
    pub fn review(&self, change: crate::ids::ChangeId) -> Result<ReviewPackage, AiError> {
        let row = self
            .agents
            .find_change(change)
            .ok_or_else(|| AiError::RequestState("unknown candidate change".into()))?;
        let transaction = row
            .transaction
            .ok_or_else(|| AiError::RequestState("candidate has no transaction".into()))?;
        Ok(ReviewPackage {
            diff: self.transactions.diff(transaction)?,
            evidence: Vec::new(),
        })
    }

    fn tool_env(&mut self) -> ToolEnvironment<'_> {
        ToolEnvironment {
            transactions: &mut self.transactions,
            project: &self.project,
            catalog: &self.catalog,
            memory: &self.memory,
            evaluation: &self.evaluation,
        }
    }

    fn fail_request(&mut self, id: RequestId, error: AiError) {
        if let Some(row) = self.agents.get_mut(id) {
            row.state = RequestState::Failed(error.to_string());
        }
        self.agents.release(id);
    }

    /// Request provenance hash for integrations.
    pub fn request_hash(&self, id: RequestId) -> Result<Hash, AiError> {
        let row = self.agents.get(id).ok_or(AiError::UnknownRequest(id))?;
        request_hash(&row.request)
    }
}
