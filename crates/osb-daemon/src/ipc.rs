//! Bounded asynchronous IPC client over `OSBP` v1 framed streams.
//!
//! Runs exclusively on the daemon's Tokio control/orchestration plane and never
//! touches real-time PipeWire audio callbacks.

use osb_core::error::{EngineError, Error, Result};
use osb_protocol::engine::{EngineId, EngineInfo};
use osb_protocol::framing::tokio_io::{read_response_frame_async, write_request_frame_async};
use osb_protocol::framing::{FrameFlags, FrameLimits, FramingError};
use osb_protocol::message::{is_version_compatible, EngineRequest, EngineResponse};
use osb_protocol::PROTOCOL_VERSION;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::task::AbortHandle;

/// Default bounded in-flight request credit window per worker connection.
pub const DEFAULT_MAX_IN_FLIGHT: usize = 4;

type PendingMap = HashMap<u64, oneshot::Sender<Result<EngineResponse>>>;
type BoxedWriter = Pin<Box<dyn AsyncWrite + Send>>;

/// Configuration for [`EngineRpcClient`].
#[derive(Debug, Clone)]
pub struct IpcClientConfig {
    /// Engine identifier used for error attribution and diagnostics.
    pub engine_id: EngineId,
    /// Upper bounds on control header and binary PCM payload sizes.
    pub frame_limits: FrameLimits,
    /// Maximum concurrent in-flight processing requests (bounded backpressure).
    pub max_in_flight: usize,
    /// Per-request deadline before automatic cancellation.
    pub request_timeout: Duration,
}

impl IpcClientConfig {
    /// Create configuration with sensible bounded defaults for `engine_id`.
    pub fn new(engine_id: impl Into<EngineId>) -> Self {
        Self {
            engine_id: engine_id.into(),
            frame_limits: FrameLimits::default(),
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            request_timeout: Duration::from_secs(10),
        }
    }
}

/// Multiplexed, bounded RPC client speaking `OSBP` v1 frames over a byte stream.
pub struct EngineRpcClient {
    config: IpcClientConfig,
    negotiated_max_payload: AtomicU32,
    writer: Arc<Mutex<BoxedWriter>>,
    in_flight_sem: Arc<Semaphore>,
    max_permits: usize,
    pending: Arc<Mutex<PendingMap>>,
    control_rx: Mutex<mpsc::Receiver<Result<EngineResponse>>>,
    closed: Arc<AtomicBool>,
    reader_abort: AbortHandle,
}

impl EngineRpcClient {
    /// Wrap a connected bidirectional async stream (such as a `UnixStream`).
    pub fn new<S>(stream: S, config: IpcClientConfig) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (reader, writer) = tokio::io::split(stream);
        let max_permits = config.max_in_flight.max(1);
        let negotiated_max_payload = AtomicU32::new(config.frame_limits.max_payload_bytes);
        let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
        let (control_tx, control_rx) = mpsc::channel(16);
        let closed = Arc::new(AtomicBool::new(false));

        let reader_handle = tokio::spawn(reader_demux_loop(
            reader,
            config.engine_id.clone(),
            config.frame_limits,
            Arc::clone(&pending),
            control_tx,
            Arc::clone(&closed),
        ));

        Self {
            config,
            negotiated_max_payload,
            writer: Arc::new(Mutex::new(Box::pin(writer))),
            in_flight_sem: Arc::new(Semaphore::new(max_permits)),
            max_permits,
            pending,
            control_rx: Mutex::new(control_rx),
            closed,
            reader_abort: reader_handle.abort_handle(),
        }
    }

    /// Check whether the underlying transport stream is still open.
    pub fn is_connected(&self) -> bool {
        !self.closed.load(Ordering::Acquire)
    }

    /// Number of currently active in-flight processing requests.
    pub fn in_flight_count(&self) -> usize {
        self.max_permits
            .saturating_sub(self.in_flight_sem.available_permits())
    }

    /// Effective frame limits after handshake negotiation.
    pub fn effective_limits(&self) -> FrameLimits {
        FrameLimits {
            max_header_bytes: self.config.frame_limits.max_header_bytes,
            max_payload_bytes: self.negotiated_max_payload.load(Ordering::Acquire),
        }
    }

    /// Perform versioned `Hello`/`Welcome` negotiation followed by `Initialize`.
    pub async fn handshake(
        &self,
        init_config: serde_json::Value,
        startup_timeout: Duration,
    ) -> Result<EngineInfo> {
        let mut ctrl_rx = self.control_rx.lock().await;

        let hello = EngineRequest::Hello {
            protocol_version: PROTOCOL_VERSION.to_string(),
            max_in_flight: self.max_permits as u32,
            max_payload_bytes: self.config.frame_limits.max_payload_bytes,
        };
        self.write_frame(&hello, FrameFlags::NONE).await?;

        let welcome = self
            .recv_control_locked(&mut ctrl_rx, startup_timeout)
            .await?;
        let info = match welcome {
            EngineResponse::Welcome {
                protocol_version,
                info,
                max_payload_bytes,
                ..
            } => {
                if !is_version_compatible(PROTOCOL_VERSION, &protocol_version) {
                    return Err(Error::Engine(EngineError::protocol(format!(
                        "incompatible worker protocol version '{protocol_version}' (runtime is '{PROTOCOL_VERSION}')"
                    ))));
                }
                let clamped = max_payload_bytes
                    .min(self.config.frame_limits.max_payload_bytes)
                    .max(1024);
                self.negotiated_max_payload
                    .store(clamped, Ordering::Release);
                info
            }
            EngineResponse::Error { code, message, .. } => {
                return Err(Error::Engine(EngineError::start_failed(
                    self.config.engine_id.as_str(),
                    format!("{code}: {message}"),
                )));
            }
            other => {
                return Err(Error::Engine(EngineError::protocol(format!(
                    "expected Welcome during handshake, got {other:?}"
                ))));
            }
        };

        let init = EngineRequest::Initialize {
            config: init_config,
        };
        self.write_frame(&init, FrameFlags::NONE).await?;

        match self
            .recv_control_locked(&mut ctrl_rx, startup_timeout)
            .await?
        {
            EngineResponse::Initialized { .. } => Ok(info),
            EngineResponse::Error { code, message, .. } => {
                Err(Error::Engine(EngineError::start_failed(
                    self.config.engine_id.as_str(),
                    format!("{code}: {message}"),
                )))
            }
            other => Err(Error::Engine(EngineError::protocol(format!(
                "expected Initialized response, got {other:?}"
            )))),
        }
    }

    /// Measure worker health and round-trip IPC latency via `Ping`/`Pong`.
    pub async fn ping(&self, timeout: Duration) -> Result<Duration> {
        let mut ctrl_rx = self.control_rx.lock().await;
        let sent_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let started = Instant::now();

        self.write_frame(
            &EngineRequest::Ping {
                timestamp_ms: sent_epoch_ms,
            },
            FrameFlags::NONE,
        )
        .await?;

        match self.recv_control_locked(&mut ctrl_rx, timeout).await? {
            EngineResponse::Pong { timestamp_ms, .. } if timestamp_ms == sent_epoch_ms => {
                Ok(started.elapsed())
            }
            EngineResponse::Pong { .. } => Err(Error::Engine(EngineError::protocol(
                "mismatched Pong timestamp",
            ))),
            other => Err(Error::Engine(EngineError::protocol(format!(
                "expected Pong, got {other:?}"
            )))),
        }
    }

    /// Non-blocking credit check: fails immediately with backpressure error if window is full.
    pub async fn try_send_request(
        &self,
        request: EngineRequest,
        flags: FrameFlags,
    ) -> Result<EngineResponse> {
        let permit = Arc::clone(&self.in_flight_sem)
            .try_acquire_owned()
            .map_err(|_| {
                Error::Engine(EngineError::protocol(format!(
                    "backpressure: engine '{}' reached max_in_flight ({})",
                    self.config.engine_id, self.max_permits
                )))
            })?;
        self.dispatch_with_permit(permit, request, flags).await
    }

    /// Send a correlated processing request, applying bounded credit wait and deadline cancellation.
    pub async fn send_request(
        &self,
        request: EngineRequest,
        flags: FrameFlags,
    ) -> Result<EngineResponse> {
        let timeout = self.config.request_timeout;
        let permit = tokio::time::timeout(timeout, Arc::clone(&self.in_flight_sem).acquire_owned())
            .await
            .map_err(|_| {
                Error::Engine(EngineError::timeout(
                    self.config.engine_id.as_str(),
                    timeout.as_millis() as u64,
                ))
            })?
            .map_err(|_| {
                Error::Engine(EngineError::crashed(
                    self.config.engine_id.as_str(),
                    "IPC semaphore closed",
                ))
            })?;

        self.dispatch_with_permit(permit, request, flags).await
    }

    /// Explicitly cancel an in-flight request on the worker.
    pub async fn cancel_request(&self, request_id: u64) -> Result<()> {
        self.write_frame(&EngineRequest::Cancel { request_id }, FrameFlags::NONE)
            .await
    }

    /// Request graceful shutdown and await `ShutdownComplete` up to `timeout`.
    pub async fn shutdown(&self, timeout: Duration) -> Result<()> {
        if !self.is_connected() {
            return Ok(());
        }
        let mut ctrl_rx = self.control_rx.lock().await;
        self.write_frame(&EngineRequest::Shutdown, FrameFlags::END_OF_STREAM)
            .await?;

        match self.recv_control_locked(&mut ctrl_rx, timeout).await {
            Ok(EngineResponse::ShutdownComplete) => {
                self.closed.store(true, Ordering::Release);
                Ok(())
            }
            Ok(other) => Err(Error::Engine(EngineError::protocol(format!(
                "unexpected response during shutdown: {other:?}"
            )))),
            Err(err) => Err(err),
        }
    }

    async fn dispatch_with_permit(
        &self,
        _permit: OwnedSemaphorePermit,
        request: EngineRequest,
        flags: FrameFlags,
    ) -> Result<EngineResponse> {
        if !self.is_connected() {
            return Err(Error::Engine(EngineError::crashed(
                self.config.engine_id.as_str(),
                "worker connection is closed",
            )));
        }

        let Some(request_id) = request.request_id() else {
            return Err(Error::Engine(EngineError::protocol(
                "send_request requires a request variant carrying request_id",
            )));
        };

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id, tx);

        if let Err(err) = self.write_frame(&request, flags).await {
            self.pending.lock().await.remove(&request_id);
            return Err(err);
        }

        let timeout = self.config.request_timeout;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_dropped)) => Err(Error::Engine(EngineError::crashed(
                self.config.engine_id.as_str(),
                "worker disconnected while request was in flight",
            ))),
            Err(_elapsed) => {
                self.pending.lock().await.remove(&request_id);
                let _ = self
                    .write_frame(&EngineRequest::Cancel { request_id }, FrameFlags::NONE)
                    .await;
                Err(Error::Engine(EngineError::timeout(
                    self.config.engine_id.as_str(),
                    timeout.as_millis() as u64,
                )))
            }
        }
    }

    async fn recv_control_locked(
        &self,
        ctrl_rx: &mut mpsc::Receiver<Result<EngineResponse>>,
        timeout: Duration,
    ) -> Result<EngineResponse> {
        match tokio::time::timeout(timeout, ctrl_rx.recv()).await {
            Ok(Some(res)) => res,
            Ok(None) => Err(Error::Engine(EngineError::crashed(
                self.config.engine_id.as_str(),
                "worker closed IPC channel",
            ))),
            Err(_) => Err(Error::Engine(EngineError::timeout(
                self.config.engine_id.as_str(),
                timeout.as_millis() as u64,
            ))),
        }
    }

    async fn write_frame(&self, request: &EngineRequest, flags: FrameFlags) -> Result<()> {
        if !self.is_connected() {
            return Err(Error::Engine(EngineError::crashed(
                self.config.engine_id.as_str(),
                "worker stream closed",
            )));
        }

        let limits = self.effective_limits();
        let mut writer = self.writer.lock().await;
        write_request_frame_async(&mut *writer, request, flags, limits)
            .await
            .map_err(|err| map_write_error(&self.config.engine_id, &self.closed, err))
    }
}

impl Drop for EngineRpcClient {
    fn drop(&mut self) {
        self.reader_abort.abort();
    }
}

fn map_write_error(engine_id: &EngineId, closed: &AtomicBool, err: FramingError) -> Error {
    match err {
        FramingError::Io(io_err) => {
            closed.store(true, Ordering::Release);
            Error::Engine(EngineError::crashed(
                engine_id.as_str(),
                format!("IPC write failed: {io_err}"),
            ))
        }
        FramingError::UnexpectedEof => {
            closed.store(true, Ordering::Release);
            Error::Engine(EngineError::crashed(
                engine_id.as_str(),
                "worker closed socket",
            ))
        }
        other => Error::Engine(EngineError::protocol(other.to_string())),
    }
}

async fn reader_demux_loop<R>(
    mut reader: R,
    engine_id: EngineId,
    limits: FrameLimits,
    pending: Arc<Mutex<PendingMap>>,
    control_tx: mpsc::Sender<Result<EngineResponse>>,
    closed: Arc<AtomicBool>,
) where
    R: AsyncRead + Unpin,
{
    loop {
        match read_response_frame_async(&mut reader, limits).await {
            Ok((response, _flags)) => {
                route_incoming_response(response, &pending, &control_tx).await;
            }
            Err(err) => {
                closed.store(true, Ordering::Release);
                let reason = match err {
                    FramingError::UnexpectedEof => "worker closed IPC stream (EOF)".to_string(),
                    FramingError::Io(io_err) => format!("worker IPC I/O error: {io_err}"),
                    protocol_err => {
                        let _ = control_tx.try_send(Err(Error::Engine(EngineError::protocol(
                            protocol_err.to_string(),
                        ))));
                        format!("worker protocol violation: {protocol_err}")
                    }
                };
                fail_all_pending(&engine_id, &reason, &pending).await;
                break;
            }
        }
    }
}

async fn route_incoming_response(
    response: EngineResponse,
    pending: &Mutex<PendingMap>,
    control_tx: &mpsc::Sender<Result<EngineResponse>>,
) {
    if let Some(req_id) = response.request_id() {
        if let Some(waiter) = pending.lock().await.remove(&req_id) {
            let _ = waiter.send(Ok(response));
        }
    } else {
        let _ = control_tx.send(Ok(response)).await;
    }
}

async fn fail_all_pending(engine_id: &EngineId, reason: &str, pending: &Mutex<PendingMap>) {
    let mut map = pending.lock().await;
    for (_, waiter) in map.drain() {
        let _ = waiter.send(Err(Error::Engine(EngineError::crashed(
            engine_id.as_str(),
            reason,
        ))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osb_engine_mock::{serve_stream, MockEngine, MockEngineConfig};

    #[tokio::test]
    async fn test_rpc_handshake_ping_and_binary_pcm_roundtrip() {
        let (client_io, server_io) = tokio::io::duplex(256 * 1024);
        let mock = MockEngine::new();
        tokio::spawn(async move {
            let _ = serve_stream(server_io, mock, FrameLimits::default()).await;
        });

        let client = EngineRpcClient::new(client_io, IpcClientConfig::new("mock"));
        let info = client
            .handshake(serde_json::Value::Null, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(info.id.as_str(), "mock");

        let rtt = client.ping(Duration::from_secs(1)).await.unwrap();
        assert!(rtt < Duration::from_secs(1));

        let resp = client
            .send_request(
                EngineRequest::ProcessS2s {
                    request_id: 7,
                    audio: vec![0.6, -0.2],
                    sample_rate: 16_000,
                    source_lang: "pt-BR".to_string(),
                    target_lang: "en-US".to_string(),
                    is_final: true,
                },
                FrameFlags::NONE,
            )
            .await
            .unwrap();

        match resp {
            EngineResponse::S2sResult { request_id, result } => {
                assert_eq!(request_id, 7);
                assert_eq!(result.data().unwrap().audio, vec![0.3, -0.1]);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        client.shutdown(Duration::from_secs(1)).await.unwrap();
    }

    #[tokio::test]
    async fn test_rpc_backpressure_and_timeout_cancellation() {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let mock = MockEngine::with_config(MockEngineConfig {
            simulated_delay_ms: 250,
            s2s_gain: 0.5,
            crash_on_request_id: None,
        });
        tokio::spawn(async move {
            let _ = serve_stream(server_io, mock, FrameLimits::default()).await;
        });

        let mut cfg = IpcClientConfig::new("mock");
        cfg.max_in_flight = 1;
        cfg.request_timeout = Duration::from_millis(60);
        let client = Arc::new(EngineRpcClient::new(client_io, cfg));

        client
            .handshake(
                serde_json::json!({"simulated_delay_ms": 250}),
                Duration::from_secs(1),
            )
            .await
            .unwrap();

        let c1 = Arc::clone(&client);
        let slow_task = tokio::spawn(async move {
            c1.send_request(
                EngineRequest::ProcessStt {
                    request_id: 1,
                    audio: vec![0.2; 160],
                    sample_rate: 16_000,
                    is_final: true,
                },
                FrameFlags::NONE,
            )
            .await
        });

        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Second non-blocking request must be rejected immediately by credit window (max_in_flight = 1).
        let bp_err = client
            .try_send_request(
                EngineRequest::ProcessStt {
                    request_id: 2,
                    audio: vec![0.2; 160],
                    sample_rate: 16_000,
                    is_final: true,
                },
                FrameFlags::NONE,
            )
            .await
            .unwrap_err();
        assert!(bp_err.to_string().contains("backpressure"));

        // First request times out cleanly at 60ms and triggers Cancel.
        let timeout_err = slow_task.await.unwrap().unwrap_err();
        assert!(matches!(
            timeout_err,
            Error::Engine(EngineError::Timeout { .. })
        ));
    }
}
