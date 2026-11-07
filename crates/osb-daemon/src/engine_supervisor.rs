//! Engine lifecycle, capability negotiation, health monitoring, and crash recovery.

use crate::worker::{ManagedWorker, WorkerSpec};
use osb_core::error::{EngineError, Error, Result};
use osb_protocol::capability::Capability;
use osb_protocol::engine::{EngineId, EngineInfo, EngineState};
use osb_protocol::framing::FrameFlags;
use osb_protocol::message::{EngineRequest, EngineResponse};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Trait for lightweight in-process Rust engines.
pub trait EngineHandler: Send + Sync {
    /// Process a single request synchronously.
    fn handle(&self, request: EngineRequest) -> EngineResponse;
}

/// Automatic restart policy for supervised external workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPolicy {
    /// Maximum automatic restarts allowed after crashes before staying in `Error`.
    pub max_restarts: u32,
    /// Delay before respawning a crashed worker.
    pub backoff: Duration,
    /// Whether a request deadline timeout should also force a worker restart.
    pub restart_on_timeout: bool,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 3,
            backoff: Duration::from_millis(50),
            restart_on_timeout: false,
        }
    }
}

/// Per-engine operational counters exposed for observability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineMetricsSnapshot {
    /// Total processing requests dispatched to this engine.
    pub requests_total: u64,
    /// Total requests that ended in an error or crash.
    pub requests_failed: u64,
    /// Total requests that exceeded `processing_timeout`.
    pub timeouts_total: u64,
    /// Number of restarts performed for this engine.
    pub restarts_total: u32,
    /// Most recent health-check `Ping`/`Pong` round-trip latency in microseconds.
    pub last_ping_rtt_us: Option<u64>,
}

struct WorkerBackend {
    spec: WorkerSpec,
    policy: RestartPolicy,
    active: Option<ManagedWorker>,
}

enum EngineBackend {
    Stub,
    InProcess(Arc<dyn EngineHandler>),
    Worker(Box<WorkerBackend>),
}

struct EngineEntry {
    info: EngineInfo,
    state: EngineState,
    backend: EngineBackend,
    metrics: EngineMetricsSnapshot,
}

/// Manages engine registration, capability negotiation, UDS worker supervision, and health.
pub struct EngineSupervisor {
    engines: HashMap<EngineId, EngineEntry>,
}

impl EngineSupervisor {
    /// Create an empty engine supervisor.
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
        }
    }

    /// Register metadata-only engine info (useful for discovery/CLI or stub testing).
    pub fn register(&mut self, info: EngineInfo) {
        self.insert_entry(info, EngineBackend::Stub);
    }

    /// Register an in-process Rust engine handler.
    pub fn register_in_process(&mut self, info: EngineInfo, handler: Arc<dyn EngineHandler>) {
        self.insert_entry(info, EngineBackend::InProcess(handler));
    }

    /// Register a persistent external UDS worker process with automatic crash recovery.
    pub fn register_worker(&mut self, info: EngineInfo, spec: WorkerSpec, policy: RestartPolicy) {
        self.insert_entry(
            info,
            EngineBackend::Worker(Box::new(WorkerBackend {
                spec,
                policy,
                active: None,
            })),
        );
    }

    fn insert_entry(&mut self, info: EngineInfo, backend: EngineBackend) {
        let id = info.id.clone();
        info!(engine_id = %id, name = %info.name, "registering engine");
        self.engines.insert(
            id,
            EngineEntry {
                info,
                state: EngineState::Stopped,
                backend,
                metrics: EngineMetricsSnapshot::default(),
            },
        );
    }

    /// Get engine metadata by ID.
    pub fn get_info(&self, id: &EngineId) -> Option<&EngineInfo> {
        self.engines.get(id).map(|e| &e.info)
    }

    /// Get current lifecycle state by ID.
    pub fn get_state(&self, id: &EngineId) -> Option<EngineState> {
        self.engines.get(id).map(|e| e.state)
    }

    /// Get operational metrics snapshot by ID.
    pub fn get_metrics(&self, id: &EngineId) -> Option<EngineMetricsSnapshot> {
        self.engines.get(id).map(|e| e.metrics)
    }

    /// Get the OS PID of a running external worker, if applicable.
    pub fn worker_pid(&self, id: &EngineId) -> Option<u32> {
        let entry = self.engines.get(id)?;
        match &entry.backend {
            EngineBackend::Worker(wb) => wb.active.as_ref().and_then(ManagedWorker::pid),
            _ => None,
        }
    }

    /// List all registered engines.
    pub fn list_engines(&self) -> Vec<&EngineInfo> {
        self.engines.values().map(|e| &e.info).collect()
    }

    /// Find registered engines advertising all `capabilities`.
    pub fn find_by_capability(&self, capabilities: &[Capability]) -> Vec<&EngineInfo> {
        self.engines
            .values()
            .filter(|e| e.info.capabilities.has_all(capabilities))
            .map(|e| &e.info)
            .collect()
    }

    /// Negotiate an engine ID matching `required` capabilities and optional language constraints.
    ///
    /// Prefers engines currently in `EngineState::Ready` over stopped engines.
    pub fn negotiate_capability(
        &self,
        required: &[Capability],
        source_lang: Option<&str>,
        target_lang: Option<&str>,
    ) -> Result<EngineId> {
        let mut fallback: Option<EngineId> = None;

        for (id, entry) in &self.engines {
            if !entry.info.capabilities.has_all(required) {
                continue;
            }
            if !matches_language_requirements(&entry.info, required, source_lang, target_lang) {
                continue;
            }
            if entry.state == EngineState::Ready {
                return Ok(id.clone());
            }
            if fallback.is_none() && !entry.state.is_error() {
                fallback = Some(id.clone());
            }
        }

        fallback.ok_or_else(|| {
            Error::Engine(EngineError::capability_not_supported(
                "<supervisor>",
                format!("{required:?}"),
            ))
        })
    }

    /// Get the count of engines ready or actively processing.
    pub fn running_count(&self) -> usize {
        self.engines
            .values()
            .filter(|e| matches!(e.state, EngineState::Ready | EngineState::Processing))
            .count()
    }

    /// Start a specific engine (spawning its UDS worker and completing handshake if external).
    pub async fn start_engine(&mut self, id: &EngineId) -> Result<()> {
        let entry = self
            .engines
            .get_mut(id)
            .ok_or_else(|| Error::Engine(EngineError::not_found(id.as_str())))?;

        if entry.state == EngineState::Ready {
            return Ok(());
        }
        if !matches!(entry.state, EngineState::Stopped | EngineState::Error) {
            return Err(Error::Engine(EngineError::start_failed(
                id.as_str(),
                format!("cannot start engine from state {}", entry.state),
            )));
        }

        info!(engine_id = %id, "starting engine");
        entry.state = EngineState::Starting;

        match &mut entry.backend {
            EngineBackend::Stub | EngineBackend::InProcess(_) => {
                entry.state = EngineState::Ready;
                Ok(())
            }
            EngineBackend::Worker(wb) => match ManagedWorker::start(id.clone(), &wb.spec).await {
                Ok(worker) => {
                    let mut negotiated = worker.negotiated_info().clone();
                    negotiated.id = id.clone();
                    entry.info = negotiated;
                    wb.active = Some(worker);
                    entry.state = EngineState::Ready;
                    Ok(())
                }
                Err(err) => {
                    entry.state = EngineState::Error;
                    Err(err)
                }
            },
        }
    }

    /// Start all registered engines that are currently stopped.
    pub async fn start_all(&mut self) -> Result<()> {
        let ids: Vec<EngineId> = self
            .engines
            .iter()
            .filter(|(_, e)| e.state == EngineState::Stopped)
            .map(|(id, _)| id.clone())
            .collect();

        for id in ids {
            self.start_engine(&id).await?;
        }
        Ok(())
    }

    /// Gracefully stop an engine and terminate its worker process if running.
    pub async fn stop_engine(&mut self, id: &EngineId) -> Result<()> {
        let entry = self
            .engines
            .get_mut(id)
            .ok_or_else(|| Error::Engine(EngineError::not_found(id.as_str())))?;

        if entry.state == EngineState::Stopped {
            return Ok(());
        }

        info!(engine_id = %id, "stopping engine");
        entry.state = EngineState::ShuttingDown;

        if let EngineBackend::Worker(wb) = &mut entry.backend {
            if let Some(mut worker) = wb.active.take() {
                let _ = worker.stop().await;
            }
        }

        entry.state = EngineState::Stopped;
        Ok(())
    }

    /// Gracefully stop all engines.
    pub async fn stop_all(&mut self) -> Result<()> {
        let ids: Vec<EngineId> = self.engines.keys().cloned().collect();
        for id in ids {
            let _ = self.stop_engine(&id).await;
        }
        Ok(())
    }

    /// Explicitly restart an engine cleanly, incrementing its restart counter.
    pub async fn restart_engine(&mut self, id: &EngineId) -> Result<()> {
        let backoff = {
            let entry = self
                .engines
                .get(id)
                .ok_or_else(|| Error::Engine(EngineError::not_found(id.as_str())))?;
            match &entry.backend {
                EngineBackend::Worker(wb) => wb.policy.backoff,
                _ => Duration::ZERO,
            }
        };

        self.stop_engine(id).await?;
        if !backoff.is_zero() {
            tokio::time::sleep(backoff).await;
        }
        if let Some(entry) = self.engines.get_mut(id) {
            entry.metrics.restarts_total += 1;
        }
        self.start_engine(id).await
    }

    /// Perform a health check (`Ping`/`Pong` + OS process check) on `id`.
    ///
    /// Automatically attempts a supervised restart if a worker crash is detected
    /// and the engine's [`RestartPolicy`] has remaining budget.
    pub async fn check_health(&mut self, id: &EngineId, timeout: Duration) -> Result<Duration> {
        let outcome = {
            let entry = self
                .engines
                .get_mut(id)
                .ok_or_else(|| Error::Engine(EngineError::not_found(id.as_str())))?;

            if entry.state != EngineState::Ready {
                return Err(Error::Engine(EngineError::protocol(format!(
                    "engine '{id}' is not ready (state: {})",
                    entry.state
                ))));
            }

            match &mut entry.backend {
                EngineBackend::Stub | EngineBackend::InProcess(_) => Ok(Duration::ZERO),
                EngineBackend::Worker(wb) => {
                    let worker = wb.active.as_mut().ok_or_else(|| {
                        Error::Engine(EngineError::crashed(id.as_str(), "worker handle missing"))
                    })?;
                    worker.ping(timeout).await
                }
            }
        };

        match outcome {
            Ok(rtt) => {
                if let Some(entry) = self.engines.get_mut(id) {
                    entry.metrics.last_ping_rtt_us = Some(rtt.as_micros() as u64);
                }
                Ok(rtt)
            }
            Err(err) => {
                self.handle_failure_and_maybe_restart(id, &err).await;
                Err(err)
            }
        }
    }

    /// Dispatch a processing request with default flags.
    pub async fn send_request(
        &mut self,
        id: &EngineId,
        request: EngineRequest,
    ) -> Result<EngineResponse> {
        self.send_request_with_flags(id, request, FrameFlags::NONE, false)
            .await
    }

    /// Dispatch a processing request with non-blocking credit backpressure check.
    pub async fn try_send_request(
        &mut self,
        id: &EngineId,
        request: EngineRequest,
        flags: FrameFlags,
    ) -> Result<EngineResponse> {
        self.send_request_with_flags(id, request, flags, true).await
    }

    /// Dispatch a processing request with explicit [`FrameFlags`] and credit mode.
    pub async fn send_request_with_flags(
        &mut self,
        id: &EngineId,
        request: EngineRequest,
        flags: FrameFlags,
        non_blocking_credits: bool,
    ) -> Result<EngineResponse> {
        let outcome = {
            let entry = self
                .engines
                .get_mut(id)
                .ok_or_else(|| Error::Engine(EngineError::not_found(id.as_str())))?;

            if entry.state != EngineState::Ready {
                return Err(Error::Engine(EngineError::protocol(format!(
                    "engine '{id}' is not ready (state: {})",
                    entry.state
                ))));
            }

            if let Some(req_cap) = required_capability_for_request(&request) {
                if !entry.info.capabilities.has(&req_cap) {
                    return Err(Error::Engine(EngineError::capability_not_supported(
                        id.as_str(),
                        format!("{req_cap:?}"),
                    )));
                }
            }

            entry.metrics.requests_total += 1;
            entry.state = EngineState::Processing;

            let res = match &mut entry.backend {
                EngineBackend::Stub => Err(Error::Engine(EngineError::protocol(format!(
                    "engine '{id}' is registered as metadata-only stub"
                )))),
                EngineBackend::InProcess(handler) => Ok(handler.handle(request)),
                EngineBackend::Worker(wb) => {
                    let worker = wb.active.as_mut().ok_or_else(|| {
                        Error::Engine(EngineError::crashed(id.as_str(), "worker handle missing"))
                    })?;
                    if non_blocking_credits {
                        worker.try_send_request(request, flags).await
                    } else {
                        worker.send_request(request, flags).await
                    }
                }
            };

            entry.state = EngineState::Ready;
            res
        };

        if let Err(ref err) = outcome {
            if let Some(entry) = self.engines.get_mut(id) {
                entry.metrics.requests_failed += 1;
                if matches!(err, Error::Engine(EngineError::Timeout { .. })) {
                    entry.metrics.timeouts_total += 1;
                }
            }
            self.handle_failure_and_maybe_restart(id, err).await;
        }

        outcome
    }

    async fn handle_failure_and_maybe_restart(&mut self, id: &EngineId, err: &Error) {
        let should_restart = {
            let Some(entry) = self.engines.get_mut(id) else {
                return;
            };
            let EngineBackend::Worker(wb) = &mut entry.backend else {
                return;
            };

            let crashed = matches!(err, Error::Engine(EngineError::Crashed { .. }));
            let timed_out_restart = matches!(err, Error::Engine(EngineError::Timeout { .. }))
                && wb.policy.restart_on_timeout;

            if !crashed && !timed_out_restart {
                return;
            }

            entry.state = EngineState::Error;
            if let Some(mut dead) = wb.active.take() {
                let _ = dead.stop().await;
            }

            entry.metrics.restarts_total < wb.policy.max_restarts
        };

        if should_restart {
            warn!(engine_id = %id, error = %err, "worker failed; attempting supervised restart");
            if let Err(restart_err) = self.restart_engine(id).await {
                warn!(engine_id = %id, error = %restart_err, "supervised worker restart failed");
            }
        }
    }
}

impl Default for EngineSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

fn required_capability_for_request(request: &EngineRequest) -> Option<Capability> {
    match request {
        EngineRequest::ProcessStt {
            is_final: false, ..
        } => Some(Capability::StreamingStt),
        EngineRequest::ProcessStt { is_final: true, .. } => Some(Capability::Stt),
        EngineRequest::ProcessSpeechTranslation {
            is_final: false, ..
        } => Some(Capability::StreamingSpeechTranslation),
        EngineRequest::ProcessSpeechTranslation { is_final: true, .. } => {
            Some(Capability::SpeechTranslation)
        }
        EngineRequest::ProcessTranslation { .. } => Some(Capability::TextTranslation),
        EngineRequest::ProcessTts { .. } => Some(Capability::Tts),
        EngineRequest::ProcessS2s { .. } => Some(Capability::SpeechToSpeech),
        _ => None,
    }
}

fn matches_language_requirements(
    info: &EngineInfo,
    required: &[Capability],
    source_lang: Option<&str>,
    target_lang: Option<&str>,
) -> bool {
    let caps = &info.capabilities;

    if required.contains(&Capability::TextTranslation)
        || required.contains(&Capability::SpeechTranslation)
        || required.contains(&Capability::SpeechToSpeech)
    {
        if let (Some(src), Some(dst)) = (source_lang, target_lang) {
            if !caps.language_pairs().is_empty() && !caps.supports_translation(src, dst) {
                return false;
            }
        }
    }

    if required.contains(&Capability::Stt) || required.contains(&Capability::StreamingStt) {
        if let Some(src) = source_lang {
            if !caps.stt_languages().is_empty() && !caps.supports_stt_language(src) {
                return false;
            }
        }
    }

    if required.contains(&Capability::Tts) || required.contains(&Capability::StreamingTts) {
        if let Some(dst) = target_lang {
            if !caps.tts_languages().is_empty() && !caps.supports_tts_language(dst) {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use osb_engine_mock::MockEngine;
    use osb_protocol::capability::{CapabilitySet, LanguagePair};
    use osb_protocol::engine::EngineType;

    struct MockInProcessAdapter(MockEngine);

    impl EngineHandler for MockInProcessAdapter {
        fn handle(&self, request: EngineRequest) -> EngineResponse {
            self.0.handle_request(request)
        }
    }

    #[test]
    fn test_engine_registration_and_capability_negotiation() {
        let mut supervisor = EngineSupervisor::new();

        let mut caps = CapabilitySet::new();
        caps.add(Capability::SpeechTranslation)
            .add(Capability::StreamingSpeechTranslation)
            .add_language_pair(LanguagePair::pt_en());

        let info = EngineInfo::builder("whisper-translate")
            .name("Whisper Translate")
            .version("1.0.0")
            .engine_type(EngineType::Python)
            .capabilities(caps)
            .build();

        supervisor.register(info);

        assert!(supervisor
            .get_info(&EngineId::new("whisper-translate"))
            .is_some());
        assert_eq!(
            supervisor.get_state(&EngineId::new("whisper-translate")),
            Some(EngineState::Stopped)
        );

        let selected = supervisor
            .negotiate_capability(
                &[Capability::SpeechTranslation],
                Some("pt-BR"),
                Some("en-US"),
            )
            .unwrap();
        assert_eq!(selected.as_str(), "whisper-translate");

        let unsupported = supervisor.negotiate_capability(
            &[Capability::SpeechTranslation],
            Some("ja-JP"),
            Some("en-US"),
        );
        assert!(unsupported.is_err());
    }

    #[tokio::test]
    async fn test_engine_lifecycle_and_in_process_dispatch() {
        let mut supervisor = EngineSupervisor::new();
        let mock = MockEngine::new();
        let info = mock.info().clone();
        let id = info.id.clone();

        supervisor.register_in_process(info, Arc::new(MockInProcessAdapter(mock)));

        assert_eq!(supervisor.get_state(&id), Some(EngineState::Stopped));

        supervisor.start_engine(&id).await.unwrap();
        assert_eq!(supervisor.get_state(&id), Some(EngineState::Ready));
        assert_eq!(supervisor.running_count(), 1);

        let resp = supervisor
            .send_request(
                &id,
                EngineRequest::ProcessTranslation {
                    request_id: 1,
                    text: "Olá".to_string(),
                    source_lang: "pt-BR".to_string(),
                    target_lang: "en-US".to_string(),
                },
            )
            .await
            .unwrap();

        assert!(matches!(resp, EngineResponse::TranslationResult { .. }));
        assert_eq!(supervisor.get_metrics(&id).unwrap().requests_total, 1);

        supervisor.stop_engine(&id).await.unwrap();
        assert_eq!(supervisor.get_state(&id), Some(EngineState::Stopped));
    }
}
