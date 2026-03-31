use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use tokio::sync::mpsc;
use tower::ServiceExt;
use ulid::Ulid;
use url::Url;

use crate::{
    api::{AppState, router},
    config::Config,
    domain::{AudioCodec, ContainerFormat, FormatCompatibility, RawUploadKey, VideoCodec},
    media_probe::{FfprobeError, ProbedMediaMetadata, port::MockMediaProbe},
    repository::{VideoRecord, port::MockVideoRepository},
    storage::port::MockStorage,
};

fn test_config() -> Arc<Config> {
    Arc::new(Config::test())
}

fn build_app(
    repo: MockVideoRepository,
    storage: MockStorage,
    probe: MockMediaProbe,
) -> axum::Router {
    let (tx, _rx) = mpsc::channel(1); // dummy channel for AppState, not used in tests
    router(AppState::new(
        Arc::new(repo),
        Arc::new(storage),
        Arc::new(probe),
        test_config(),
        tx,
    ))
}

fn video_record(ulid: Ulid, status: &str, browser_compatible: bool) -> VideoRecord {
    VideoRecord {
        ulid,
        status: status.to_string(),
        raw_key: RawUploadKey::from(ulid),
        transmux_key: None,
        manifest_key: None,
        browser_compatible,
        transmux_required: false,
        transcode_required: !browser_compatible,
    }
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ── POST /api/upload-url ─────────────────────────────────────────────────────

#[tokio::test]
async fn create_upload_url_returns_upload_session() {
    let mut repo = MockVideoRepository::new();
    repo.expect_create_pending_video()
        .once()
        .returning(|_, _, _, _| Ok(()));

    let mut storage = MockStorage::new();
    storage
        .expect_create_upload_url()
        .once()
        .returning(|_, _| Ok(Url::parse("https://r2.example.com/upload/key").unwrap()));

    let response = build_app(repo, storage, MockMediaProbe::new())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/upload-url")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"content_type":"video/mp4","size_bytes":1000}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["ulid"].is_string());
    assert_eq!(body["upload_url"], "https://r2.example.com/upload/key");
}

// ── POST /api/upload-complete/{ulid} ─────────────────────────────────────────

#[tokio::test]
async fn mark_upload_complete_sets_browser_compatible_for_h264_aac_mp4() {
    let ulid = Ulid::new();

    let mut repo = MockVideoRepository::new();
    repo.expect_find_video_by_ulid()
        .once()
        .returning(move |_| Ok(Some(video_record(ulid, "pending_upload", false))));
    repo.expect_mark_uploaded_with_compatibility()
        .once()
        .withf(|_, compat| *compat == FormatCompatibility::BrowserCompatible)
        .returning(|_, _| Ok(true));

    let mut probe = MockMediaProbe::new();
    probe.expect_probe_url().once().returning(|_| {
        Ok(ProbedMediaMetadata {
            container_format: Some(ContainerFormat::Mp4),
            video_codec: Some(VideoCodec::H264),
            audio_codec: Some(AudioCodec::Aac),
        })
    });

    let response = build_app(repo, MockStorage::new(), probe)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/upload-complete/{ulid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn mark_upload_complete_sets_transcode_required_for_hevc() {
    let ulid = Ulid::new();

    let mut repo = MockVideoRepository::new();
    repo.expect_find_video_by_ulid()
        .once()
        .returning(move |_| Ok(Some(video_record(ulid, "pending_upload", false))));
    repo.expect_mark_uploaded_with_compatibility()
        .once()
        .withf(|_, compat| *compat == FormatCompatibility::TranscodeRequired)
        .returning(|_, _| Ok(true));

    let mut probe = MockMediaProbe::new();
    probe.expect_probe_url().once().returning(|_| {
        Ok(ProbedMediaMetadata {
            container_format: Some(ContainerFormat::Matroska),
            video_codec: Some(VideoCodec::Hevc),
            audio_codec: Some(AudioCodec::Flac),
        })
    });

    let response = build_app(repo, MockStorage::new(), probe)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/upload-complete/{ulid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn mark_upload_complete_returns_404_when_video_not_found() {
    let ulid = Ulid::new();

    let mut repo = MockVideoRepository::new();
    repo.expect_find_video_by_ulid()
        .once()
        .returning(|_| Ok(None));

    let response = build_app(repo, MockStorage::new(), MockMediaProbe::new())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/upload-complete/{ulid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mark_upload_complete_returns_500_when_probe_fails() {
    let ulid = Ulid::new();

    let mut repo = MockVideoRepository::new();
    repo.expect_find_video_by_ulid()
        .once()
        .returning(move |_| Ok(Some(video_record(ulid, "pending_upload", false))));

    let mut probe = MockMediaProbe::new();
    probe.expect_probe_url().once().returning(|_| {
        Err(FfprobeError::NonZeroExit {
            code: Some(1),
            stderr: "ffprobe failed".to_string(),
        })
    });

    let response = build_app(repo, MockStorage::new(), probe)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/upload-complete/{ulid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn mark_upload_complete_pushes_ulid_to_worker_channel() {
    let ulid = Ulid::new();

    let mut repo = MockVideoRepository::new();
    repo.expect_find_video_by_ulid()
        .once()
        .returning(move |_| Ok(Some(video_record(ulid, "pending_upload", false))));
    repo.expect_mark_uploaded_with_compatibility()
        .once()
        .returning(|_, _| Ok(true));

    let mut probe = MockMediaProbe::new();
    probe.expect_probe_url().once().returning(|_| {
        Ok(ProbedMediaMetadata {
            container_format: Some(ContainerFormat::Mp4),
            video_codec: Some(VideoCodec::H264),
            audio_codec: Some(AudioCodec::Aac),
        })
    });

    // Create a channel with buffer size 1 for testing
    let (worker_tx, mut worker_rx) = mpsc::channel(1);
    let app_state = AppState::new(
        Arc::new(repo),
        Arc::new(MockStorage::new()),
        Arc::new(probe),
        test_config(),
        worker_tx,
    );
    let app = router(app_state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/upload-complete/{ulid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Verify that the ulid was sent to the worker channel
    let received_ulid = worker_rx.recv().await.unwrap();
    assert_eq!(received_ulid, ulid);
}

// ── GET /api/video/{ulid} ────────────────────────────────────────────────────

#[tokio::test]
async fn get_video_metadata_returns_200_for_existing_video() {
    let ulid = Ulid::new();

    let mut repo = MockVideoRepository::new();
    repo.expect_find_video_by_ulid()
        .once()
        .returning(move |_| Ok(Some(video_record(ulid, "uploaded", true))));

    let response = build_app(repo, MockStorage::new(), MockMediaProbe::new())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&format!("/api/video/{ulid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ulid"].as_str().unwrap(), ulid.to_string());
    assert_eq!(body["browser_compatible"], true);
}

#[tokio::test]
async fn get_video_metadata_returns_404_when_video_not_found() {
    let ulid = Ulid::new();

    let mut repo = MockVideoRepository::new();
    repo.expect_find_video_by_ulid()
        .once()
        .returning(|_| Ok(None));

    let response = build_app(repo, MockStorage::new(), MockMediaProbe::new())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&format!("/api/video/{ulid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── TODO: upload-url ─────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "todo"]
async fn create_upload_url_returns_400_for_invalid_content_type() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn create_upload_url_returns_400_when_size_exceeds_max() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn create_upload_url_returns_500_when_storage_presign_fails() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn create_upload_url_returns_500_when_repo_insert_fails() {
    todo!()
}

// ── TODO: upload-complete ─────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "todo"]
async fn mark_upload_complete_sets_transmux_required_for_h264_in_mkv() {
    // Matroska container + H264 video + AAC audio → TransmuxRequired
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn mark_upload_complete_returns_404_when_mark_returns_false() {
    // find_video_by_ulid returns Some, but mark_uploaded_with_compatibility
    // returns false (row was concurrently deleted between the two calls)
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn mark_upload_complete_returns_500_when_repo_find_fails() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn mark_upload_complete_returns_500_when_repo_update_fails() {
    todo!()
}

// ── TODO: video metadata ───────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "todo"]
async fn get_video_metadata_raw_url_uses_cdn_domain() {
    // raw_url in response must be prefixed with PUBLIC_CDN_DOMAIN from config
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn get_video_metadata_includes_transmux_url_when_key_is_set() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn get_video_metadata_returns_500_when_repo_fails() {
    todo!()
}
