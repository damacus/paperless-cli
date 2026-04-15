use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use paperless_cli::api::{ApiClient, Endpoint, HttpMethod, RequestData, ResponseData, Transport};
use paperless_cli::config::{
    load_config, load_session, save_config, save_session, AppConfig, AppPaths, OutputMode,
    SessionState,
};
use paperless_cli::error::AppError;
use paperless_cli::services::{sanitize_filename, DocumentQuery, UpdateRequest, UploadRequest};
use paperless_cli::{api::multipart_file_from_path, services};
use serde_json::json;
use tempfile::tempdir;

#[derive(Clone, Debug, Default)]
struct MockTransport {
    requests: Arc<Mutex<Vec<RequestData>>>,
    responses: Arc<Mutex<VecDeque<Result<ResponseData, AppError>>>>,
}

static ENV_LOCK: Mutex<()> = Mutex::new(());

impl MockTransport {
    fn push_json(&self, status: u16, url: &str, body: serde_json::Value) {
        self.responses.lock().unwrap().push_back(Ok(ResponseData {
            status,
            url: url.to_string(),
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        }));
    }

    fn push_response(&self, response: ResponseData) {
        self.responses.lock().unwrap().push_back(Ok(response));
    }

    fn requests(&self) -> Vec<RequestData> {
        self.requests.lock().unwrap().clone()
    }
}

impl Transport for MockTransport {
    fn send(&self, request: RequestData) -> Result<ResponseData, AppError> {
        self.requests.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(AppError::Message(
                    "missing mocked response for request".to_string(),
                ))
            })
    }
}

#[test]
fn config_roundtrip_redacts_token_and_persists_session() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::remove_var("PAPERLESS_URL");
        std::env::remove_var("PAPERLESS_TOKEN");
    }
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(
        temp.path().join("config.toml"),
        temp.path().join("session.toml"),
    );
    let config = AppConfig::new(
        "https://paperless.example.com/",
        "super-secret-token",
        OutputMode::Markdown,
    )
    .unwrap();

    save_config(&paths, &config).unwrap();
    let loaded = load_config(&paths).unwrap();
    assert_eq!(loaded.base_url, "https://paperless.example.com");
    assert_eq!(loaded.masked_token(), "super-se...");
    assert!(!format!("{loaded:?}").contains("super-secret-token"));

    let mut session = SessionState {
        last_query: "invoice".to_string(),
        selected_docs: vec![1, 2],
        ..Default::default()
    };
    session.push_history("documents list");
    save_session(&paths, &session).unwrap();

    let restored = load_session(&paths);
    assert_eq!(restored.last_query, "invoice");
    assert_eq!(restored.selected_docs, vec![1, 2]);
    assert_eq!(restored.history, vec!["documents list"]);

    unsafe {
        std::env::remove_var("PAPERLESS_URL");
        std::env::remove_var("PAPERLESS_TOKEN");
    }
}

#[test]
fn masked_token_handles_multibyte_prefixes_without_panicking() {
    let config = AppConfig::new(
        "https://paperless.example.com/",
        "秘密令牌🙂abcdef",
        OutputMode::Markdown,
    )
    .unwrap();

    assert_eq!(config.masked_token(), "秘密令牌🙂abc...")
}

#[test]
fn load_config_allows_env_only_and_env_override() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(
        temp.path().join("config.toml"),
        temp.path().join("session.toml"),
    );

    unsafe {
        std::env::set_var("PAPERLESS_URL", "https://env.example.com");
        std::env::set_var("PAPERLESS_TOKEN", "env-token");
    }
    let env_only = load_config(&paths).unwrap();
    assert_eq!(env_only.base_url, "https://env.example.com");
    assert_eq!(env_only.token, "env-token");

    let persisted = AppConfig::new(
        "https://persisted.example.com",
        "persisted-token",
        OutputMode::Markdown,
    )
    .unwrap();
    save_config(&paths, &persisted).unwrap();

    unsafe {
        std::env::set_var("PAPERLESS_URL", "https://override.example.com");
        std::env::remove_var("PAPERLESS_TOKEN");
    }
    let merged = load_config(&paths).unwrap();
    assert_eq!(merged.base_url, "https://override.example.com");
    assert_eq!(merged.token, "persisted-token");

    unsafe {
        std::env::remove_var("PAPERLESS_URL");
        std::env::remove_var("PAPERLESS_TOKEN");
    }
}

#[test]
fn document_query_matches_python_filters() {
    let query = DocumentQuery {
        query: Some("invoice".to_string()),
        tag: Some("urgent".to_string()),
        tag_id: Some(5),
        correspondent: Some("ACME".to_string()),
        correspondent_id: Some(7),
        document_type: Some("Invoice".to_string()),
        document_type_id: Some(9),
        created_after: Some("2024-01-01".to_string()),
        created_before: Some("2024-12-31".to_string()),
        order_by: "title".to_string(),
        page_size: 10,
        page: 2,
    };

    let pairs = query.to_query_pairs();
    assert!(pairs.contains(&("query".to_string(), "invoice".to_string())));
    assert!(pairs.contains(&("tags__name__icontains".to_string(), "urgent".to_string())));
    assert!(pairs.contains(&("tags__id__in".to_string(), "5".to_string())));
    assert!(pairs.contains(&("correspondent__id".to_string(), "7".to_string())));
    assert!(pairs.contains(&("document_type__id".to_string(), "9".to_string())));
    assert!(pairs.contains(&("created__date__gt".to_string(), "2024-01-01".to_string())));
    assert!(pairs.contains(&("created__date__lt".to_string(), "2024-12-31".to_string())));
}

#[test]
fn client_sends_auth_headers_and_paginate_follows_next_url() {
    let transport = MockTransport::default();
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/",
        json!({
            "count": 3,
            "next": "https://paperless.example.com/api/tags/?page=2&page_size=100",
            "results": [{"id": 1}, {"id": 2}],
        }),
    );
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/?page=2&page_size=100",
        json!({
            "count": 3,
            "next": null,
            "results": [{"id": 3}],
        }),
    );

    let client = ApiClient::new(transport.clone());
    let results = client.paginate("tags/", Vec::new()).unwrap();
    assert_eq!(results.len(), 3);

    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, HttpMethod::Get);
    assert_eq!(
        requests[0].endpoint,
        Endpoint::Relative("tags/".to_string())
    );
    assert_eq!(
        requests[1].endpoint,
        Endpoint::Absolute(
            "https://paperless.example.com/api/tags/?page=2&page_size=100".to_string()
        )
    );
}

#[test]
fn client_rejects_cross_origin_pagination_links() {
    let transport = MockTransport::default();
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/",
        json!({
            "count": 2,
            "next": "https://evil.example.net/api/tags/?page=2",
            "results": [{"id": 1}],
        }),
    );

    let client = ApiClient::new(transport);
    let error = client.paginate("tags/", Vec::new()).unwrap_err();
    match error {
        AppError::Message(message) => assert!(message.contains("cross-origin pagination")),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn client_rejects_same_host_pagination_links_with_different_ports() {
    let transport = MockTransport::default();
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/",
        json!({
            "count": 2,
            "next": "https://paperless.example.com:8443/api/tags/?page=2",
            "results": [{"id": 1}],
        }),
    );

    let client = ApiClient::new(transport);
    let error = client.paginate("tags/", Vec::new()).unwrap_err();
    match error {
        AppError::Message(message) => assert!(message.contains("cross-origin pagination")),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn client_normalizes_scheme_mismatched_pagination_links() {
    let transport = MockTransport::default();
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/",
        json!({
            "count": 3,
            "next": "http://paperless.example.com/api/tags/?page=2&page_size=100",
            "results": [{"id": 1}, {"id": 2}],
        }),
    );
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/?page=2&page_size=100",
        json!({
            "count": 3,
            "next": null,
            "results": [{"id": 3}],
        }),
    );

    let client = ApiClient::new(transport.clone());
    let results = client.paginate("tags/", Vec::new()).unwrap();
    assert_eq!(results.len(), 3);

    let requests = transport.requests();
    assert_eq!(
        requests[1].endpoint,
        Endpoint::Absolute(
            "https://paperless.example.com/api/tags/?page=2&page_size=100".to_string()
        )
    );
}

#[test]
fn client_maps_http_errors() {
    let transport = MockTransport::default();
    transport.push_json(
        401,
        "https://paperless.example.com/api/documents/",
        json!({"detail": "bad auth"}),
    );
    let client = ApiClient::new(transport);
    let error = client.get_json("documents/", Vec::new()).unwrap_err();
    match error {
        AppError::Http {
            status, message, ..
        } => {
            assert_eq!(status, 401);
            assert!(message.contains("Authentication failed"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn config_rejects_remote_plain_http() {
    let error = AppConfig::new(
        "http://paperless.example.com",
        "token",
        OutputMode::Markdown,
    )
    .unwrap_err();
    match error {
        AppError::InsecureRemoteUrl(url) => assert_eq!(url, "http://paperless.example.com"),
        other => panic!("unexpected error: {other:?}"),
    }

    let local = AppConfig::new("http://127.0.0.1:8000", "token", OutputMode::Markdown).unwrap();
    assert_eq!(local.base_url, "http://127.0.0.1:8000");
}

#[test]
fn upload_and_update_requests_encode_expected_payloads() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("invoice.pdf");
    std::fs::write(&file_path, b"%PDF fake").unwrap();

    let transport = MockTransport::default();
    transport.push_json(
        200,
        "https://paperless.test/api/documents/post_document/",
        json!({"task_id": "123"}),
    );
    transport.push_json(
        200,
        "https://paperless.test/api/documents/42/",
        json!({"id": 42, "title": "Updated"}),
    );
    let client = ApiClient::new(transport.clone());

    services::upload_document(
        &client,
        &UploadRequest {
            path: file_path.clone(),
            title: Some("Invoice".to_string()),
            correspondent_id: Some(9),
            document_type_id: Some(3),
            tag_ids: vec![1, 2],
        },
    )
    .unwrap();
    services::update_document(
        &client,
        42,
        &UpdateRequest {
            title: Some("Updated".to_string()),
            correspondent_id: Some(9),
            document_type_id: Some(3),
            tag_ids: Some(vec![1, 2]),
            created: Some("2024-01-01".to_string()),
            custom_fields: None,
        },
    )
    .unwrap();

    let requests = transport.requests();
    assert_eq!(requests[0].multipart_fields.len(), 5);
    assert_eq!(
        requests[0].multipart_file.as_ref().unwrap().file_name,
        "invoice.pdf"
    );
    assert_eq!(
        requests[1].json_body.as_ref().unwrap()["created"],
        json!("2024-01-01")
    );
}

#[test]
fn document_edit_resolves_exact_tag_names_and_merges_tags() {
    let transport = MockTransport::default();
    transport.push_json(
        200,
        "https://paperless.example.com/api/documents/42/",
        json!({
            "id": 42,
            "tags": [1, 3],
        }),
    );
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/",
        json!([
            {"id": 1, "name": "existing"},
            {"id": 2, "name": "important"},
            {"id": 3, "name": "todo"}
        ]),
    );
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/",
        json!([
            {"id": 1, "name": "existing"},
            {"id": 2, "name": "important"},
            {"id": 3, "name": "todo"}
        ]),
    );
    transport.push_json(
        200,
        "https://paperless.example.com/api/documents/42/",
        json!({"id": 42, "tags": [1, 2]}),
    );

    let client = ApiClient::new(transport.clone());
    let response = services::edit_document(
        &client,
        42,
        &UpdateRequest {
            title: Some("Updated title".to_string()),
            correspondent_id: None,
            document_type_id: None,
            tag_ids: None,
            created: None,
            custom_fields: None,
        },
        &["important".to_string()],
        &["todo".to_string()],
    )
    .unwrap();

    assert_eq!(response["tags"], json!([1, 2]));
    let requests = transport.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].method, HttpMethod::Patch);
    assert_eq!(
        requests[3].json_body.as_ref().unwrap()["tags"],
        json!([1, 2])
    );
    assert_eq!(
        requests[3].json_body.as_ref().unwrap()["title"],
        json!("Updated title")
    );
}

#[test]
fn download_sanitizes_filenames_and_rejects_missing_file_uploads() {
    let transport = MockTransport::default();
    let mut headers = BTreeMap::new();
    headers.insert(
        "content-disposition".to_string(),
        "attachment; filename=\"../../danger.pdf\"".to_string(),
    );
    transport.push_response(ResponseData {
        status: 200,
        url: "https://paperless.test/api/documents/9/download/".to_string(),
        headers,
        body: b"fake-pdf".to_vec(),
    });

    let client = ApiClient::new(transport);
    let temp = tempdir().unwrap();
    let saved = services::download_document(&client, 9, temp.path(), false).unwrap();
    assert_eq!(saved.file_name().unwrap().to_string_lossy(), "danger.pdf");
    assert_eq!(sanitize_filename("../../danger.pdf"), "danger.pdf");

    let missing = multipart_file_from_path(&PathBuf::from("missing.pdf")).unwrap_err();
    match missing {
        AppError::FileMissing(path) => assert!(path.ends_with("missing.pdf")),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn download_prefers_decoded_rfc5987_filenames() {
    let transport = MockTransport::default();
    let mut headers = BTreeMap::new();
    headers.insert(
        "content-disposition".to_string(),
        "attachment; filename=\"invoice.pdf\"; filename*=UTF-8''2026-03-26_Equal%20Experts.pdf"
            .to_string(),
    );
    transport.push_response(ResponseData {
        status: 200,
        url: "https://paperless.test/api/documents/10/download/".to_string(),
        headers,
        body: b"fake-pdf".to_vec(),
    });

    let client = ApiClient::new(transport);
    let temp = tempdir().unwrap();
    let saved = services::download_document(&client, 10, temp.path(), false).unwrap();
    assert_eq!(
        saved.file_name().unwrap().to_string_lossy(),
        "2026-03-26_Equal_Experts.pdf"
    );
}
