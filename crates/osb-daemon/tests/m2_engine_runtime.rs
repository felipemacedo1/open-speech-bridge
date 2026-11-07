//! Milestone 2 integration tests: UDS IPC, binary PCM framing, health checks,
//! backpressure/timeouts, and OS subprocess crash recovery.

#![cfg(unix)]

use osb_core::error::{EngineError, Error};
use osb_daemon::{EngineSupervisor, RestartPolicy, Runtime, WorkerSpec};
use osb_engine_mock::{connect_and_serve_uds, listen_and_serve_uds, MockEngine, MockEngineConfig};
use osb_protocol::capability::Capability;
use osb_protocol::engine::{EngineId, EngineInfo, EngineState, EngineType};
use osb_protocol::framing::{FrameLimits, FramingError};
use osb_protocol::message::{AudioFrameMeta, EngineRequest, EngineResponse};
use std::path::PathBuf;
use std::time::Duration;

/// Child-process entry point invoked only when `OSB_TEST_CHILD_WORKER=1` is set.
#[test]
fn __mock_worker_child_entry() {
    if std::env::var("OSB_TEST_CHILD_WORKER").as_deref() != Ok("1") {
        return;
    }

    let socket_path = PathBuf::from(
        std::env::var("OSB_ENGINE_SOCKET").expect("OSB_ENGINE_SOCKET must be set for child worker"),
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let engine = MockEngine::new();
    let res = rt.block_on(connect_and_serve_uds(
        &socket_path,
        engine.clone(),
        FrameLimits::default(),
    ));

    if matches!(res, Err(FramingError::UnexpectedEof))
        && engine.config().crash_on_request_id.is_some()
    {
        std::process::exit(101);
    }
    res.expect("child mock worker serve loop");
}

fn child_worker_spec(init_config: serde_json::Value) -> WorkerSpec {
    let exe = std::env::current_exe().expect("test binary path");
    WorkerSpec::spawn(exe, ["--exact", "__mock_worker_child_entry", "--nocapture"])
        .with_pass_socket_arg(false)
        .with_env("OSB_TEST_CHILD_WORKER", "1")
        .with_init_config(init_config)
        .with_startup_timeout(Duration::from_secs(5))
        .with_processing_timeout(Duration::from_secs(2))
}

#[tokio::test]
async fn test_uds_worker_subprocess_handshake_and_binary_pcm_transform() {
    let runtime = Runtime::with_defaults();
    let id = EngineId::new("mock-uds");

    {
        let mut sup = runtime.supervisor().lock().await;
        let placeholder = EngineInfo::builder(id.clone())
            .name("Mock Placeholder")
            .engine_type(EngineType::External)
            .build();
        sup.register_worker(
            placeholder,
            child_worker_spec(serde_json::json!({ "s2s_gain": 0.5 })),
            RestartPolicy::default(),
        );
    }

    runtime.start().await.unwrap();

    {
        let mut sup = runtime.supervisor().lock().await;
        assert_eq!(sup.get_state(&id), Some(EngineState::Ready));
        assert!(sup.worker_pid(&id).is_some());

        // Capability negotiation populated from worker's Welcome handshake
        let negotiated = sup
            .negotiate_capability(
                &[Capability::SpeechTranslation, Capability::SpeechToSpeech],
                Some("pt-BR"),
                Some("en-US"),
            )
            .unwrap();
        assert_eq!(negotiated, id);

        // Health monitoring via Ping/Pong
        let rtt = sup.check_health(&id, Duration::from_secs(1)).await.unwrap();
        assert!(rtt < Duration::from_secs(1));
        assert!(sup.get_metrics(&id).unwrap().last_ping_rtt_us.is_some());

        // Direct speech translation with AudioFrameMeta over binary PCM
        let st_resp = sup
            .send_request(
                &id,
                EngineRequest::ProcessSpeechTranslation {
                    request_id: 10,
                    audio: vec![0.35; 320],
                    sample_rate: 16_000,
                    source_lang: "pt-BR".to_string(),
                    target_lang: "en-US".to_string(),
                    is_final: true,
                    meta: Some(AudioFrameMeta {
                        session_id: 1,
                        utterance_id: 1,
                        chunk_id: 0,
                        timestamp_us: 5_000,
                        channels: 1,
                    }),
                },
            )
            .await
            .unwrap();

        match st_resp {
            EngineResponse::SpeechTranslationResult { request_id, result } => {
                assert_eq!(request_id, 10);
                assert!(result.data().unwrap().text.contains("pt-BR->en-US"));
            }
            other => panic!("unexpected speech translation response: {other:?}"),
        }

        // S2S audio transformation over binary PCM
        let s2s_resp = sup
            .send_request(
                &id,
                EngineRequest::ProcessS2s {
                    request_id: 11,
                    audio: vec![0.8, -0.4, 0.2, -1.0],
                    sample_rate: 16_000,
                    source_lang: "pt-BR".to_string(),
                    target_lang: "en-US".to_string(),
                    is_final: true,
                },
            )
            .await
            .unwrap();

        match s2s_resp {
            EngineResponse::S2sResult { request_id, result } => {
                assert_eq!(request_id, 11);
                assert_eq!(result.data().unwrap().audio, vec![0.4, -0.2, 0.1, -0.5]);
            }
            other => panic!("unexpected S2S response: {other:?}"),
        }
    }

    runtime.stop().await.unwrap();
    let sup = runtime.supervisor().lock().await;
    assert_eq!(sup.get_state(&id), Some(EngineState::Stopped));
}

#[tokio::test]
async fn test_supervisor_recovers_from_worker_process_crash_and_enforces_max_restarts() {
    let mut supervisor = EngineSupervisor::new();
    let id = EngineId::new("crash-resilient-mock");
    let info = EngineInfo::builder(id.clone())
        .name("Crash Test Worker")
        .engine_type(EngineType::External)
        .build();

    let spec = child_worker_spec(serde_json::json!({
        "s2s_gain": 0.5,
        "crash_on_request_id": 99
    }));
    let policy = RestartPolicy {
        max_restarts: 1,
        backoff: Duration::from_millis(20),
        restart_on_timeout: false,
    };

    supervisor.register_worker(info, spec, policy);
    supervisor.start_engine(&id).await.unwrap();

    let pid_before = supervisor.worker_pid(&id).expect("initial worker PID");

    // Normal request succeeds
    let ok_resp = supervisor
        .send_request(
            &id,
            EngineRequest::ProcessStt {
                request_id: 1,
                audio: vec![0.4; 160],
                sample_rate: 16_000,
                is_final: true,
            },
        )
        .await
        .unwrap();
    assert!(matches!(ok_resp, EngineResponse::SttResult { .. }));

    // Request 99 triggers real OS child process crash (exit code 101)
    let crash_err = supervisor
        .send_request(
            &id,
            EngineRequest::ProcessStt {
                request_id: 99,
                audio: vec![0.4; 160],
                sample_rate: 16_000,
                is_final: true,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        crash_err,
        Error::Engine(EngineError::Crashed { .. })
    ));

    // Supervisor automatically restarted the worker within budget (restarts_total == 1)
    assert_eq!(supervisor.get_state(&id), Some(EngineState::Ready));
    let pid_after = supervisor.worker_pid(&id).expect("restarted worker PID");
    assert_ne!(pid_before, pid_after);
    assert_eq!(supervisor.get_metrics(&id).unwrap().restarts_total, 1);

    // Subsequent request on recovered worker succeeds seamlessly
    let recovered_resp = supervisor
        .send_request(
            &id,
            EngineRequest::ProcessStt {
                request_id: 2,
                audio: vec![0.4; 160],
                sample_rate: 16_000,
                is_final: true,
            },
        )
        .await
        .unwrap();
    assert!(matches!(recovered_resp, EngineResponse::SttResult { .. }));

    // Second crash exceeds max_restarts (1) -> stays in Error state
    let second_crash = supervisor
        .send_request(
            &id,
            EngineRequest::ProcessStt {
                request_id: 99,
                audio: vec![0.4; 160],
                sample_rate: 16_000,
                is_final: true,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        second_crash,
        Error::Engine(EngineError::Crashed { .. })
    ));
    assert_eq!(supervisor.get_state(&id), Some(EngineState::Error));
    assert_eq!(supervisor.get_metrics(&id).unwrap().restarts_total, 1);

    supervisor.stop_all().await.unwrap();
}

#[tokio::test]
async fn test_uds_connect_mode_and_processing_timeout() {
    let sock_path =
        std::env::temp_dir().join(format!("osb-attach-test-{}.sock", std::process::id()));
    let sock_clone = sock_path.clone();

    let server_task = tokio::spawn(async move {
        let engine = MockEngine::with_config(MockEngineConfig {
            simulated_delay_ms: 250,
            s2s_gain: 0.5,
            crash_on_request_id: None,
        });
        let _ = listen_and_serve_uds(&sock_clone, engine, FrameLimits::default()).await;
    });

    let mut supervisor = EngineSupervisor::new();
    let id = EngineId::new("attached-mock");
    let info = EngineInfo::builder(id.clone())
        .name("Attached Worker")
        .engine_type(EngineType::External)
        .build();

    let spec = WorkerSpec::connect(&sock_path)
        .with_init_config(serde_json::json!({ "simulated_delay_ms": 250 }))
        .with_startup_timeout(Duration::from_secs(2))
        .with_processing_timeout(Duration::from_millis(60));

    supervisor.register_worker(info, spec, RestartPolicy::default());
    supervisor.start_engine(&id).await.unwrap();

    let err = supervisor
        .send_request(
            &id,
            EngineRequest::ProcessStt {
                request_id: 55,
                audio: vec![0.3; 160],
                sample_rate: 16_000,
                is_final: true,
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(err, Error::Engine(EngineError::Timeout { .. })));
    assert_eq!(supervisor.get_metrics(&id).unwrap().timeouts_total, 1);

    supervisor.stop_all().await.unwrap();
    let _ = server_task.await;
}
