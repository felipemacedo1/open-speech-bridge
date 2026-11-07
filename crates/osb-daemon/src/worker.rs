//! Supervised persistent external engine worker lifecycle over Unix Domain Sockets.

use crate::ipc::{EngineRpcClient, IpcClientConfig, DEFAULT_MAX_IN_FLIGHT};
use osb_core::error::{EngineError, Error, Result};
use osb_protocol::engine::{EngineId, EngineInfo};
use osb_protocol::framing::{FrameFlags, FrameLimits};
use osb_protocol::message::{EngineRequest, EngineResponse};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tracing::{debug, info, warn};

static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(1);

/// How the daemon establishes a Unix Domain Socket transport to an external worker.
#[derive(Debug, Clone)]
pub enum WorkerTransport {
    /// Bind a private UDS listener first, then spawn a persistent child subprocess.
    Spawn {
        /// Executable path or program name.
        program: PathBuf,
        /// Command-line arguments passed before `--socket <path>` (if enabled).
        args: Vec<String>,
        /// Extra environment variables (`OSB_ENGINE_SOCKET` is always injected).
        env: Vec<(String, String)>,
        /// Append `--socket <path>` to `args` automatically.
        pass_socket_arg: bool,
        /// Optional custom directory for private socket files (`0o700`).
        socket_dir: Option<PathBuf>,
    },

    /// Connect to an already-listening external UDS worker.
    Connect {
        /// Filesystem path of the worker's Unix Domain Socket.
        socket_path: PathBuf,
    },
}

/// Complete specification for launching or attaching to a persistent worker.
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// Transport creation mode.
    pub transport: WorkerTransport,
    /// JSON configuration payload sent in `EngineRequest::Initialize`.
    pub init_config: serde_json::Value,
    /// Maximum allowed time for process spawn + UDS accept + handshake.
    pub startup_timeout: Duration,
    /// Maximum allowed time per processing request before cancellation.
    pub processing_timeout: Duration,
    /// Bounded concurrent in-flight request credit window.
    pub max_in_flight: usize,
    /// Binary wire frame size limits.
    pub frame_limits: FrameLimits,
}

impl WorkerSpec {
    /// Create a managed child-process worker spec passing `--socket <path>`.
    pub fn spawn(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            transport: WorkerTransport::Spawn {
                program: program.into(),
                args: args.into_iter().map(Into::into).collect(),
                env: Vec::new(),
                pass_socket_arg: true,
                socket_dir: None,
            },
            init_config: serde_json::Value::Null,
            startup_timeout: Duration::from_secs(10),
            processing_timeout: Duration::from_secs(10),
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            frame_limits: FrameLimits::default(),
        }
    }

    /// Create a worker spec that attaches to an existing Unix Domain Socket.
    pub fn connect(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            transport: WorkerTransport::Connect {
                socket_path: socket_path.into(),
            },
            init_config: serde_json::Value::Null,
            startup_timeout: Duration::from_secs(5),
            processing_timeout: Duration::from_secs(10),
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            frame_limits: FrameLimits::default(),
        }
    }

    /// Set the JSON `Initialize` configuration sent after `Hello`/`Welcome`.
    pub fn with_init_config(mut self, config: serde_json::Value) -> Self {
        self.init_config = config;
        self
    }

    /// Set startup timeout.
    pub fn with_startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = timeout;
        self
    }

    /// Set per-request processing deadline.
    pub fn with_processing_timeout(mut self, timeout: Duration) -> Self {
        self.processing_timeout = timeout;
        self
    }

    /// Set bounded in-flight concurrency window.
    pub fn with_max_in_flight(mut self, max_in_flight: usize) -> Self {
        self.max_in_flight = max_in_flight.max(1);
        self
    }

    /// Control whether `--socket <path>` is appended to spawned child arguments.
    ///
    /// Even when `false`, `OSB_ENGINE_SOCKET=<path>` is always provided in the child's environment.
    pub fn with_pass_socket_arg(mut self, enabled: bool) -> Self {
        if let WorkerTransport::Spawn {
            pass_socket_arg, ..
        } = &mut self.transport
        {
            *pass_socket_arg = enabled;
        }
        self
    }

    /// Add an environment variable to a spawned worker process.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let WorkerTransport::Spawn { env, .. } = &mut self.transport {
            env.push((key.into(), value.into()));
        }
        self
    }
}

/// Active handle to a connected, initialized external engine worker.
pub struct ManagedWorker {
    engine_id: EngineId,
    negotiated_info: EngineInfo,
    client: EngineRpcClient,
    child: Option<Child>,
    owned_socket_path: Option<PathBuf>,
}

impl ManagedWorker {
    /// Start or connect to the worker described by `spec` and complete the protocol handshake.
    #[cfg(unix)]
    pub async fn start(engine_id: EngineId, spec: &WorkerSpec) -> Result<Self> {
        let ipc_cfg = IpcClientConfig {
            engine_id: engine_id.clone(),
            frame_limits: spec.frame_limits,
            max_in_flight: spec.max_in_flight,
            request_timeout: spec.processing_timeout,
        };

        let started_at = Instant::now();
        let (stream, mut child, owned_socket_path) = match &spec.transport {
            WorkerTransport::Spawn {
                program,
                args,
                env,
                pass_socket_arg,
                socket_dir,
            } => {
                spawn_and_accept_uds(
                    &engine_id,
                    program,
                    args,
                    env,
                    *pass_socket_arg,
                    socket_dir.as_deref(),
                    spec.startup_timeout,
                )
                .await?
            }
            WorkerTransport::Connect { socket_path } => {
                let stream =
                    connect_with_backoff(&engine_id, socket_path, spec.startup_timeout).await?;
                (stream, None, None)
            }
        };

        let remaining = spec
            .startup_timeout
            .checked_sub(started_at.elapsed())
            .unwrap_or(Duration::from_millis(50));

        let client = EngineRpcClient::new(stream, ipc_cfg);
        let negotiated_info = match client.handshake(spec.init_config.clone(), remaining).await {
            Ok(info) => info,
            Err(err) => {
                cleanup_failed_start(&mut child, owned_socket_path.as_deref()).await;
                return Err(err);
            }
        };

        info!(
            engine_id = %engine_id,
            worker_name = %negotiated_info.name,
            version = %negotiated_info.version,
            "persistent UDS engine worker initialized"
        );

        Ok(Self {
            engine_id,
            negotiated_info,
            client,
            child,
            owned_socket_path,
        })
    }

    /// Non-Unix fallback stub returning an explicit platform error.
    #[cfg(not(unix))]
    pub async fn start(engine_id: EngineId, _spec: &WorkerSpec) -> Result<Self> {
        Err(Error::Engine(EngineError::start_failed(
            engine_id.as_str(),
            "Unix Domain Socket workers require a Unix platform",
        )))
    }

    /// Capabilities and metadata advertised during the `Welcome` handshake.
    pub fn negotiated_info(&self) -> &EngineInfo {
        &self.negotiated_info
    }

    /// OS process ID if this worker was spawned as a managed child subprocess.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    /// Verify OS process liveness (if spawned) and UDS transport state.
    pub fn check_liveness(&mut self) -> Result<()> {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(Error::Engine(EngineError::crashed(
                        self.engine_id.as_str(),
                        format!("worker process exited unexpectedly with {status}"),
                    )));
                }
                Ok(None) => {}
                Err(io_err) => {
                    return Err(Error::Engine(EngineError::crashed(
                        self.engine_id.as_str(),
                        format!("failed to poll worker process status: {io_err}"),
                    )));
                }
            }
        }

        if !self.client.is_connected() {
            return Err(Error::Engine(EngineError::crashed(
                self.engine_id.as_str(),
                "worker UDS transport disconnected",
            )));
        }

        Ok(())
    }

    /// Run a health-check `Ping`/`Pong` exchange after verifying process liveness.
    pub async fn ping(&mut self, timeout: Duration) -> Result<Duration> {
        self.check_liveness()?;
        let rtt = self.client.ping(timeout).await;
        if rtt.is_err() {
            self.check_liveness()?;
        }
        rtt
    }

    /// Send a correlated request, enforcing bounded in-flight credits and deadline.
    pub async fn send_request(
        &mut self,
        request: EngineRequest,
        flags: FrameFlags,
    ) -> Result<EngineResponse> {
        self.check_liveness()?;
        let res = self.client.send_request(request, flags).await;
        if res.is_err() {
            // Prefer reporting the concrete OS exit status if the child died mid-request.
            self.check_liveness()?;
        }
        res
    }

    /// Non-blocking credit check + request dispatch.
    pub async fn try_send_request(
        &mut self,
        request: EngineRequest,
        flags: FrameFlags,
    ) -> Result<EngineResponse> {
        self.check_liveness()?;
        let res = self.client.try_send_request(request, flags).await;
        if res.is_err() {
            self.check_liveness()?;
        }
        res
    }

    /// Explicitly cancel an in-flight request by ID.
    pub async fn cancel_request(&self, request_id: u64) -> Result<()> {
        self.client.cancel_request(request_id).await
    }

    /// Gracefully shut down the worker over IPC, terminate the child if needed, and unlink its socket.
    pub async fn stop(&mut self) -> Result<()> {
        let _ = self.client.shutdown(Duration::from_secs(2)).await;

        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                Ok(Ok(status)) => {
                    debug!(engine_id = %self.engine_id, ?status, "worker process exited cleanly");
                }
                _ => {
                    warn!(engine_id = %self.engine_id, "worker did not exit within grace period; killing");
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            }
        }

        self.remove_socket_file();
        Ok(())
    }

    fn remove_socket_file(&mut self) {
        if let Some(path) = self.owned_socket_path.take() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

impl Drop for ManagedWorker {
    fn drop(&mut self) {
        self.remove_socket_file();
    }
}

#[cfg(unix)]
fn allocate_private_socket_path(custom_dir: Option<&Path>) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let dir = match custom_dir {
        Some(d) => d.to_path_buf(),
        None => std::env::temp_dir().join(format!("osb-uds-{}", std::process::id())),
    };

    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;

    let seq = SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
    let sock_path = dir.join(format!("w-{seq}.sock"));
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }
    Ok(sock_path)
}

#[cfg(unix)]
async fn spawn_and_accept_uds(
    engine_id: &EngineId,
    program: &Path,
    args: &[String],
    env: &[(String, String)],
    pass_socket_arg: bool,
    socket_dir: Option<&Path>,
    startup_timeout: Duration,
) -> Result<(tokio::net::UnixStream, Option<Child>, Option<PathBuf>)> {
    use std::os::unix::fs::PermissionsExt;

    let socket_path = allocate_private_socket_path(socket_dir)?;
    let listener = tokio::net::UnixListener::bind(&socket_path).map_err(|err| {
        Error::Engine(EngineError::start_failed(
            engine_id.as_str(),
            format!("failed to bind UDS {}: {err}", socket_path.display()),
        ))
    })?;
    let _ = tokio::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600)).await;

    let mut cmd = Command::new(program);
    cmd.args(args);
    if pass_socket_arg {
        cmd.arg("--socket").arg(&socket_path);
    }
    cmd.env("OSB_ENGINE_SOCKET", &socket_path)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            let _ = tokio::fs::remove_file(&socket_path).await;
            return Err(Error::Engine(EngineError::start_failed(
                engine_id.as_str(),
                format!("failed to spawn {}: {err}", program.display()),
            )));
        }
    };

    let accept_outcome = tokio::time::timeout(startup_timeout, async {
        tokio::select! {
            accepted = listener.accept() => Ok(accepted),
            exited = child.wait() => Err(exited),
        }
    })
    .await;

    match accept_outcome {
        Ok(Ok(Ok((stream, _addr)))) => Ok((stream, Some(child), Some(socket_path))),
        Ok(Ok(Err(io_err))) => {
            cleanup_failed_start(&mut Some(child), Some(&socket_path)).await;
            Err(Error::Engine(EngineError::start_failed(
                engine_id.as_str(),
                format!("UDS accept failed: {io_err}"),
            )))
        }
        Ok(Err(wait_res)) => {
            let _ = tokio::fs::remove_file(&socket_path).await;
            let detail = match wait_res {
                Ok(status) => format!("worker exited before UDS connect ({status})"),
                Err(io_err) => format!("worker wait failed ({io_err})"),
            };
            Err(Error::Engine(EngineError::start_failed(
                engine_id.as_str(),
                detail,
            )))
        }
        Err(_elapsed) => {
            cleanup_failed_start(&mut Some(child), Some(&socket_path)).await;
            Err(Error::Engine(EngineError::timeout(
                engine_id.as_str(),
                startup_timeout.as_millis() as u64,
            )))
        }
    }
}

#[cfg(unix)]
async fn connect_with_backoff(
    engine_id: &EngineId,
    socket_path: &Path,
    startup_timeout: Duration,
) -> Result<tokio::net::UnixStream> {
    let deadline = Instant::now() + startup_timeout;
    let mut delay = Duration::from_millis(10);

    loop {
        match tokio::net::UnixStream::connect(socket_path).await {
            Ok(stream) => return Ok(stream),
            Err(last_err) => {
                if Instant::now() >= deadline {
                    return Err(Error::Engine(EngineError::start_failed(
                        engine_id.as_str(),
                        format!(
                            "failed to connect to UDS {} within {}ms: {last_err}",
                            socket_path.display(),
                            startup_timeout.as_millis()
                        ),
                    )));
                }
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_millis(100));
            }
        }
    }
}

#[cfg(unix)]
async fn cleanup_failed_start(child: &mut Option<Child>, socket_path: Option<&Path>) {
    if let Some(c) = child.as_mut() {
        let _ = c.kill().await;
        let _ = c.wait().await;
    }
    if let Some(p) = socket_path {
        let _ = tokio::fs::remove_file(p).await;
    }
}
