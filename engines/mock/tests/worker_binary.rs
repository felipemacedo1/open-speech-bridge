//! Integration test verifying the standalone `osb-engine-mock` binary over a Unix Domain Socket.

#![cfg(unix)]

use osb_protocol::framing::tokio_io::{read_response_frame_async, write_request_frame_async};
use osb_protocol::framing::{FrameFlags, FrameLimits, DEFAULT_MAX_PAYLOAD_BYTES};
use osb_protocol::message::{EngineRequest, EngineResponse};
use osb_protocol::PROTOCOL_VERSION;
use std::process::Stdio;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::process::Command;

#[tokio::test]
async fn test_standalone_mock_worker_binary_over_uds() {
    let sock_path =
        std::env::temp_dir().join(format!("osb-mock-bin-test-{}.sock", std::process::id()));
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }

    let listener = UnixListener::bind(&sock_path).expect("bind UDS");
    let bin = env!("CARGO_BIN_EXE_osb-engine-mock");

    let mut child = Command::new(bin)
        .arg("--socket")
        .arg(&sock_path)
        .arg("--gain")
        .arg("0.25")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn osb-engine-mock");

    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("accept timeout")
        .expect("accept UDS");

    let limits = FrameLimits::default();

    // 1. Handshake
    write_request_frame_async(
        &mut stream,
        &EngineRequest::Hello {
            protocol_version: PROTOCOL_VERSION.to_string(),
            max_in_flight: 4,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
        },
        FrameFlags::NONE,
        limits,
    )
    .await
    .unwrap();

    let (welcome, _) = read_response_frame_async(&mut stream, limits)
        .await
        .unwrap();
    assert!(matches!(welcome, EngineResponse::Welcome { .. }));

    // 2. ProcessS2s audio transform (gain = 0.25)
    write_request_frame_async(
        &mut stream,
        &EngineRequest::ProcessS2s {
            request_id: 1,
            audio: vec![0.8, -0.4],
            sample_rate: 16_000,
            source_lang: "pt-BR".to_string(),
            target_lang: "en-US".to_string(),
            is_final: true,
        },
        FrameFlags::NONE,
        limits,
    )
    .await
    .unwrap();

    let (s2s, flags) = read_response_frame_async(&mut stream, limits)
        .await
        .unwrap();
    assert!(flags.contains(FrameFlags::HAS_PCM_PAYLOAD));
    match s2s {
        EngineResponse::S2sResult { result, .. } => {
            assert_eq!(result.data().unwrap().audio, vec![0.2, -0.1]);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    // 3. Clean shutdown
    write_request_frame_async(
        &mut stream,
        &EngineRequest::Shutdown,
        FrameFlags::END_OF_STREAM,
        limits,
    )
    .await
    .unwrap();

    let (done, _) = read_response_frame_async(&mut stream, limits)
        .await
        .unwrap();
    assert!(matches!(done, EngineResponse::ShutdownComplete));

    let exit_status = tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .expect("child exit timeout")
        .expect("child wait");
    assert!(exit_status.success());

    let _ = std::fs::remove_file(&sock_path);
}
