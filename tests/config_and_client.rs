#![warn(
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::exit,
    clippy::disallowed_methods,
    clippy::dbg_macro,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::undocumented_unsafe_blocks,
    clippy::as_conversions
)]
#![allow(
    // Transitive duplicate versions are outside our control.
    clippy::multiple_crate_versions,
    // Error behaviour is documented at module level, not via per-fn
    // `# Errors` sections; the public surface is consumed internally.
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    // Function length is governed by cognitive-complexity, not lines.
    clippy::too_many_lines,
    // Licence/keyword metadata is a maintainer decision, not a lint.
    clippy::cargo_common_metadata,
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::unreachable,
        clippy::disallowed_methods,
        clippy::future_not_send,
    )
)]

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

const CONFIG_TEST_CASE: &str = "PAPERLESS_CLI_CONFIG_TEST_CASE";

// Give each scenario its own process environment while tests run in parallel.
fn run_config_test(test: &str, case: &str, url: Option<&str>, token: Option<&str>) {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.args(["--exact", test]).env(CONFIG_TEST_CASE, case);
    for (name, value) in [("PAPERLESS_URL", url), ("PAPERLESS_TOKEN", token)] {
        if let Some(value) = value {
            child.env(name, value);
        } else {
            child.env_remove(name);
        }
    }
    let output = child.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("running 1 test"),
        "{}\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr),
    );
}

impl MockTransport {
    // Let fixtures pass inline JSON values directly into the response builder.
    #[allow(clippy::needless_pass_by_value)]
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
    if std::env::var_os(CONFIG_TEST_CASE).is_none() {
        run_config_test(
            "config_roundtrip_redacts_token_and_persists_session",
            "persisted",
            None,
            None,
        );
        return;
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
}

#[cfg(unix)]
#[test]
fn config_save_rejects_preexisting_temp_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target.txt");
    let config_path = temp.path().join("config.toml");
    std::fs::write(&target, b"keep this file").unwrap();
    symlink(&target, config_path.with_extension("tmp")).unwrap();

    let config = AppConfig::new(
        "https://paperless.example.com",
        "super-secret-token",
        OutputMode::Markdown,
    )
    .unwrap();
    let paths = AppPaths::new(&config_path, temp.path().join("session.toml"));

    assert!(save_config(&paths, &config).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep this file");
}

#[test]
fn masked_token_handles_multibyte_prefixes_without_panicking() {
    let config = AppConfig::new(
        "https://paperless.example.com/",
        "秘密令牌🙂abcdef",
        OutputMode::Markdown,
    )
    .unwrap();

    assert_eq!(config.masked_token(), "秘密令牌🙂abc...");
}

#[test]
fn load_config_allows_env_only_and_env_override() {
    let Some(case) = std::env::var_os(CONFIG_TEST_CASE) else {
        let test = "load_config_allows_env_only_and_env_override";
        run_config_test(
            test,
            "environment",
            Some("https://env.example.com"),
            Some("env-token"),
        );
        run_config_test(test, "override", Some("https://override.example.com"), None);
        return;
    };
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(
        temp.path().join("config.toml"),
        temp.path().join("session.toml"),
    );

    if case == "environment" {
        let env_only = load_config(&paths).unwrap();
        assert_eq!(env_only.base_url, "https://env.example.com");
        assert_eq!(env_only.token, "env-token");
        return;
    }

    let persisted = AppConfig::new(
        "https://persisted.example.com",
        "persisted-token",
        OutputMode::Markdown,
    )
    .unwrap();
    save_config(&paths, &persisted).unwrap();

    let merged = load_config(&paths).unwrap();
    assert_eq!(merged.base_url, "https://override.example.com");
    assert_eq!(merged.token, "persisted-token");
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
            path: file_path,
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
        json!({
            "results": [{"id": 2, "name": "important"}],
            "next": "https://paperless.example.com/api/tags/?page=2",
        }),
    );
    transport.push_json(
        200,
        "https://paperless.example.com/api/tags/?page=2",
        json!({"results": [{"id": 3, "name": "todo"}], "next": null}),
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
        &["important".to_string(), "2".to_string(), "3".to_string()],
        &["todo".to_string()],
    )
    .unwrap();

    assert_eq!(response["tags"], json!([1, 2]));
    let requests = transport.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[1].endpoint,
        Endpoint::Relative("tags/".to_string())
    );
    assert_eq!(
        requests[2].endpoint,
        Endpoint::Absolute("https://paperless.example.com/api/tags/?page=2".to_string())
    );
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
fn document_edit_with_numeric_tags_only_needs_document_permissions() {
    let transport = MockTransport::default();
    let document_url = "https://paperless.example.com/api/documents/42/";
    transport.push_json(200, document_url, json!({"id": 42, "tags": [1, 3]}));
    transport.push_json(200, document_url, json!({"id": 42, "tags": [1, 2]}));

    services::edit_document(
        &ApiClient::new(transport.clone()),
        42,
        &UpdateRequest::default(),
        &["2".to_string(), "2".to_string()],
        &["3".to_string()],
    )
    .unwrap();

    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].method, HttpMethod::Patch);
    assert_eq!(requests[1].json_body, Some(json!({"tags": [1, 2]})));
}

#[test]
fn document_edit_keeps_named_lookup_errors_and_never_patches_on_failure() {
    let tags_url = "https://paperless.example.com/api/tags/";
    for (status, payload) in [
        (200, json!([{"id": 3, "name": "TODO"}])),
        (403, json!({"detail": "Forbidden"})),
    ] {
        let transport = MockTransport::default();
        transport.push_json(
            200,
            "https://paperless.example.com/api/documents/42/",
            json!({"id": 42, "tags": [1, 3]}),
        );
        transport.push_json(status, tags_url, payload);

        let error = services::edit_document(
            &ApiClient::new(transport.clone()),
            42,
            &UpdateRequest::default(),
            &["2".to_string()],
            &["todo".to_string()],
        )
        .unwrap_err();

        if status == 403 {
            assert!(matches!(error, AppError::Http { status: 403, .. }));
        } else {
            assert!(error.to_string().contains("Unknown tag reference `todo`"));
        }
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| request.method == HttpMethod::Get));
    }
}

#[test]
fn tui_dashboard_uses_first_task_and_does_not_fetch_later_pages() {
    // API v9 and older servers can return a list. API v10 supports page_size.
    // The TasksViewSet orders both response formats newest first.
    let first = json!({"id": 12, "status": "PENDING"});
    for tasks in [
        json!([first, {"id": 10, "status": "SUCCESS"}]),
        json!({
            "results": [first],
            "next": "https://paperless.example.com/api/tasks/?page=2&page_size=1"
        }),
    ] {
        let transport = MockTransport::default();
        let config = AppConfig {
            base_url: "https://paperless.example.com".to_string(),
            token: "test-token".to_string(),
            preferred_output: OutputMode::Markdown,
        };
        transport.push_json(200, "https://paperless.example.com/api/status/", json!({}));
        transport.push_json(200, "https://paperless.example.com/api/tasks/", tasks);

        let dashboard =
            services::dashboard(&ApiClient::new(transport.clone()), &config, vec![]).unwrap();

        let latest_task = dashboard.latest_task.unwrap();
        assert_eq!(latest_task.id, Some(12));
        assert_eq!(latest_task.status, "PENDING");
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].query,
            vec![("page_size".to_string(), "1".to_string())]
        );
    }
}

#[test]
fn operational_overview_starts_all_independent_reads_before_any_response() {
    struct GatedTransport(Sender<(String, Sender<ResponseData>)>);

    impl Transport for GatedTransport {
        fn send(&self, request: RequestData) -> Result<ResponseData, AppError> {
            let Endpoint::Relative(path) = request.endpoint else {
                panic!("unexpected absolute request");
            };
            let (respond, response) = mpsc::channel();
            self.0.send((path, respond)).unwrap();
            response
                .recv_timeout(Duration::from_secs(3))
                .map_err(|error| AppError::Message(error.to_string()))
        }
    }

    let (requests, started) = mpsc::channel();
    let client = ApiClient::new(GatedTransport(requests));
    let config = AppConfig::new(
        "https://paperless.example.com",
        "test-token",
        OutputMode::Json,
    )
    .unwrap();
    let overview =
        std::thread::spawn(move || services::operational_overview_parallel(&client, &config));

    let mut pending = BTreeMap::new();
    for _ in 0..3 {
        let (path, respond) = started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(pending.insert(path, respond).is_none());
    }
    for (path, body) in [
        ("status/", json!({"version": "test"})),
        ("statistics/", json!({"documents_total": 7})),
        ("tasks/", json!([{"id": 12, "status": "SUCCESS"}])),
    ] {
        pending
            .remove(path)
            .unwrap()
            .send(ResponseData {
                status: 200,
                url: format!("https://paperless.example.com/api/{path}"),
                headers: BTreeMap::new(),
                body: serde_json::to_vec(&body).unwrap(),
            })
            .unwrap();
    }
    let data = overview.join().unwrap().unwrap();
    assert_eq!(data["status"]["response"]["version"], "test");
    assert_eq!(data["statistics"]["documents_total"], 7);
    assert_eq!(data["tasks"][0]["id"], 12);
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
