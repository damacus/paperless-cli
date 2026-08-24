use std::fs;
use std::process::Command;

use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use tempfile::tempdir;

fn base_env() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let session = dir.path().join("session.toml");
    (dir, config, session)
}

#[test]
fn help_lists_rust_tui_output_modes() {
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Rust TUI and LLM-friendly client"));
    assert!(stdout.contains("--output"));
    assert!(stdout.contains("--demo"));
    assert!(stdout.contains("login"));
    assert!(stdout.contains("config"));
    assert!(stdout.contains("pdf"));
    assert!(stdout.contains("dashboard"));
    assert!(stdout.contains("Check server health"));
}

#[test]
fn help_paths_make_no_http_requests_with_configured_url() {
    let server = MockServer::start();
    let requests = server.mock(|_when, then| {
        then.status(500);
    });
    let (_dir, config, session) = base_env();

    for args in [
        vec!["--help"],
        vec!["help", "status"],
        vec!["status", "--help"],
        vec!["help", "dashboard"],
        vec!["dashboard", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
            .env("PAPERLESS_CONFIG_PATH", &config)
            .env("PAPERLESS_SESSION_PATH", &session)
            .env("PAPERLESS_URL", server.base_url())
            .env("PAPERLESS_TOKEN", "env-token")
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    requests.assert_calls(0);
}

#[test]
fn document_list_requires_config() {
    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", config)
        .env("PAPERLESS_SESSION_PATH", session)
        .args(["document", "list"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("paperless login"));
}

#[test]
fn status_without_config_is_non_fatal_terminal_output() {
    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", config)
        .env("PAPERLESS_SESSION_PATH", session)
        .arg("status")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Status  NOT CONFIGURED"));
    assert!(stdout.contains("paperless login"));
    assert!(!stdout.contains("**"));
    assert!(!stdout.contains('`'));
}

#[test]
fn status_without_config_is_machine_readable_in_json_mode() {
    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", config)
        .env("PAPERLESS_SESSION_PATH", session)
        .args(["--output", "json", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("\"connected\": false"));
}

#[test]
fn status_only_calls_the_status_endpoint() {
    let server = MockServer::start();
    let status = server.mock(|when, then| {
        when.method(GET).path("/api/status/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"status":"ok","version":"2.18.4"}"#);
    });
    let statistics = server.mock(|when, then| {
        when.method(GET).path("/api/statistics/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"documents_total":6}"#);
    });
    let tasks = server.mock(|when, then| {
        when.method(GET).path("/api/tasks/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"[]"#);
    });

    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .env("PAPERLESS_URL", server.base_url())
        .env("PAPERLESS_TOKEN", "env-token")
        .arg("status")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains(" ____   _    ____  _____ ____"));
    assert!(stdout.contains("Paperless-ngx 2.18.4"));
    assert!(!stdout.contains("Needs attention"));
    assert!(!stdout.contains("**"));
    assert!(!stdout.contains('`'));
    assert!(!stdout.contains("statistics"));
    assert!(!stdout.contains("tasks"));
    status.assert_calls(1);
    statistics.assert_calls(0);
    tasks.assert_calls(0);
}

#[test]
fn dashboard_fetches_the_explicit_operational_overview() {
    let server = MockServer::start();
    let status = server.mock(|when, then| {
        when.method(GET).path("/api/status/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"status":"ok","version":"2.18.4"}"#);
    });
    let statistics = server.mock(|when, then| {
        when.method(GET).path("/api/statistics/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"documents_total":6}"#);
    });
    let tasks = server.mock(|when, then| {
        when.method(GET).path("/api/tasks/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"[{"id":901,"status":"SUCCESS"}]"#);
    });

    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .env("PAPERLESS_URL", server.base_url())
        .env("PAPERLESS_TOKEN", "env-token")
        .args(["--output", "json", "dashboard"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("\"statistics\""));
    assert!(stdout.contains("\"tasks\""));
    status.assert_calls(1);
    statistics.assert_calls(1);
    tasks.assert_calls(1);
}

#[test]
fn login_with_token_writes_config() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/status/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"status":"ok"}"#);
    });

    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args([
            "--output",
            "json",
            "login",
            "--url",
            &server.base_url(),
            "--token",
            "test-token",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("\"status\": \"ok\""));

    let written = fs::read_to_string(config).unwrap();
    assert!(written.contains("test-token"));
}

#[test]
fn login_with_username_and_password_fetches_token() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/api/token/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"token":"fresh-token"}"#);
    });
    server.mock(|when, then| {
        when.method(GET).path("/api/status/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"status":"ok"}"#);
    });

    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args([
            "--output",
            "json",
            "login",
            "--url",
            &server.base_url(),
            "--username",
            "admin",
            "--password",
            "secret",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("\"status\": \"ok\""));

    let written = fs::read_to_string(config).unwrap();
    assert!(written.contains("fresh-token"));
}

#[test]
fn login_falls_back_when_status_endpoint_returns_403() {
    let server = MockServer::start();
    // /api/status/ returns 403 (Paperless-ngx 3.x view_status restriction)
    server.mock(|when, then| {
        when.method(GET).path("/api/status/");
        then.status(403)
            .header("content-type", "text/html")
            .body("Insufficient permissions");
    });
    // Fallback: /api/documents/?page_size=1 succeeds
    server.mock(|when, then| {
        when.method(GET).path("/api/documents/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"count":1,"next":null,"previous":null,"results":[]}"#);
    });

    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args([
            "--output",
            "json",
            "login",
            "--url",
            &server.base_url(),
            "--token",
            "test-token",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "login should succeed even when /api/status/ returns 403, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("\"status\": \"ok\""));
}

#[test]
fn config_set_url_uses_env_token() {
    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .env("PAPERLESS_TOKEN", "env-token")
        .args(["config", "set-url", "https://paperless.example.com"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let written = fs::read_to_string(config).unwrap();
    assert!(written.contains("https://paperless.example.com"));
    assert!(written.contains("env-token"));
}

#[test]
fn config_set_token_uses_env_url() {
    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .env("PAPERLESS_URL", "https://paperless.example.com")
        .args(["config", "set-token", "fresh-token"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let written = fs::read_to_string(config).unwrap();
    assert!(written.contains("https://paperless.example.com"));
    assert!(written.contains("fresh-token"));
}

#[test]
fn document_content_reads_from_env_config() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/documents/42/");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"id":42,"title":"Invoice","content":"Hello\nWorld"}"#);
    });

    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .env("PAPERLESS_URL", server.base_url())
        .env("PAPERLESS_TOKEN", "env-token")
        .args(["document", "content", "42"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Hello"));
    assert!(stdout.contains("World"));
    assert!(!stdout.contains("title"));
}

#[test]
fn pdf_read_and_info_commands_work() {
    let (_dir, config, session) = base_env();
    let pdf_path = write_fixture_pdf();

    let read = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args(["pdf", "read", pdf_path.to_str().unwrap()])
        .output()
        .unwrap();
    let read_stdout = String::from_utf8_lossy(&read.stdout);
    assert!(read.status.success());
    assert!(read_stdout.contains("Hello from PDF"));

    let info = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args([
            "--output",
            "json",
            "pdf",
            "info",
            pdf_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let info_stdout = String::from_utf8_lossy(&info.stdout);
    assert!(info.status.success());
    assert!(info_stdout.contains("\"page_count\": 1"));
    assert!(info_stdout.contains("\"file_name\": \"sample.pdf\""));
}

#[test]
fn demo_status_is_available_without_config() {
    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", config)
        .env("PAPERLESS_SESSION_PATH", session)
        .args(["--demo", "--output", "json", "status"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("\"demo\": true"));
    assert!(stdout.contains("\"response\""));
    assert!(!stdout.contains("\"documents_total\""));
}

#[test]
fn demo_dashboard_includes_the_full_overview() {
    let (_dir, config, session) = base_env();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", config)
        .env("PAPERLESS_SESSION_PATH", session)
        .args(["--demo", "--output", "json", "dashboard"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("\"documents_total\": 6"));
    assert!(stdout.contains("\"tasks\""));
}

#[test]
fn demo_document_commands_return_fixture_data() {
    let (_dir, config, session) = base_env();

    let list = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args(["--demo", "document", "list"])
        .output()
        .unwrap();
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(list.status.success());
    assert!(list_stdout.contains("Project Delivery Invoice 25-26-010"));

    let content = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args(["--demo", "document", "content", "303"])
        .output()
        .unwrap();
    let content_stdout = String::from_utf8_lossy(&content.stdout);
    assert!(content.status.success());
    assert!(content_stdout.contains("Project delivery and advisory services."));
    assert!(!content_stdout.contains("\"title\""));
}

#[test]
fn demo_collection_commands_use_compact_terminal_output() {
    let (_dir, config, session) = base_env();
    let cases = [
        (
            vec!["--demo", "document", "list"],
            ["6 documents", "ID   DATE        TITLE"],
        ),
        (
            vec![
                "--demo",
                "document",
                "search",
                "invoice",
                "--page-size",
                "100",
            ],
            ["3 documents", "ID   DATE        TITLE"],
        ),
        (
            vec!["--demo", "search", "query", "invoice"],
            ["5 results", "DOCUMENT TYPES (1)"],
        ),
        (
            vec!["--demo", "tag", "list"],
            ["4 tags", "ID  NAME     INBOX"],
        ),
        (
            vec!["--demo", "correspondent", "list"],
            ["6 correspondents", "ID  NAME"],
        ),
        (
            vec!["--demo", "doctype", "list"],
            ["4 document types", "ID  NAME"],
        ),
        (vec!["--demo", "task", "list"], ["2 tasks", "ID   STATUS"]),
        (
            vec!["--demo", "search", "autocomplete", "invoice"],
            ["3 suggestions", "Consulting Invoice March 2026"],
        ),
    ];

    for (args, expected) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
            .env("PAPERLESS_CONFIG_PATH", &config)
            .env("PAPERLESS_SESSION_PATH", &session)
            .args(&args)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{args:?}: {stdout}");
        for fragment in expected {
            assert!(
                stdout.contains(fragment),
                "missing {fragment:?} for {args:?}: {stdout}"
            );
        }
        for leaked in ["**", "| ---", "__search_hit__", "null"] {
            assert!(
                !stdout.contains(leaked),
                "unexpected {leaked:?} for {args:?}: {stdout}"
            );
        }
    }
}

#[test]
fn demo_download_writes_fixture_file() {
    let (_dir, config, session) = base_env();
    let output_dir = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_paperless"))
        .env("PAPERLESS_CONFIG_PATH", &config)
        .env("PAPERLESS_SESSION_PATH", &session)
        .args([
            "--demo",
            "--output",
            "json",
            "document",
            "download",
            "303",
            "--output-dir",
            output_dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("\"status\": \"ok\""));

    let files = std::fs::read_dir(output_dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with(".pdf"));
}

fn write_fixture_pdf() -> std::path::PathBuf {
    let tempdir = tempdir().unwrap();
    let pdf_path = tempdir.path().join("sample.pdf");
    std::fs::write(&pdf_path, build_fixture_pdf()).unwrap();
    tempdir.keep().join("sample.pdf")
}

fn build_fixture_pdf() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();

    push_line(&mut bytes, "%PDF-1.4");
    push_line(&mut bytes, "%\u{00E2}\u{00E3}\u{00CF}\u{00D3}");
    push_object(
        &mut bytes,
        &mut offsets,
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    );
    push_object(
        &mut bytes,
        &mut offsets,
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
    );
    push_object(
        &mut bytes,
        &mut offsets,
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n",
    );
    push_object(
        &mut bytes,
        &mut offsets,
        "4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );
    let stream = b"BT\n/F1 24 Tf\n72 720 Td\n(Hello from PDF) Tj\nET\n";
    let content_object = format!(
        "5 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
        stream.len(),
        String::from_utf8_lossy(stream)
    );
    push_object(&mut bytes, &mut offsets, &content_object);
    push_object(
        &mut bytes,
        &mut offsets,
        "6 0 obj\n<< /Title (Invoice 2026) >>\nendobj\n",
    );

    let xref_offset = bytes.len();
    push_line(&mut bytes, "xref");
    push_line(&mut bytes, "0 7");
    push_line(&mut bytes, "0000000000 65535 f ");
    for offset in offsets {
        push_line(&mut bytes, &format!("{offset:010} 00000 n "));
    }
    push_line(&mut bytes, "trailer << /Root 1 0 R /Info 6 0 R /Size 7 >>");
    push_line(&mut bytes, &format!("startxref\n{xref_offset}"));
    push_line(&mut bytes, "%%EOF");
    bytes
}

fn push_object(bytes: &mut Vec<u8>, offsets: &mut Vec<usize>, object: &str) {
    offsets.push(bytes.len());
    bytes.extend_from_slice(object.as_bytes());
}

fn push_line(bytes: &mut Vec<u8>, line: &str) {
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
}
