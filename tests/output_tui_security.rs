use std::sync::{Arc, Mutex};
use std::time::Duration;

use paperless_cli::config::OutputMode;
use paperless_cli::render::render_output;
use paperless_cli::security::{AuditState, SecurityAgentProfile, SecurityAuditor, Severity};
use paperless_cli::services::{
    document_text_representation, DashboardSnapshot, DocumentInspector, DocumentSummary,
    OutputEnvelope, TaskSummary,
};
use paperless_cli::tui::{draw, TuiApp};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;

#[test]
fn markdown_and_json_output_are_llm_friendly() {
    let envelope = OutputEnvelope {
        mode: "markdown".to_string(),
        command: "documents list".to_string(),
        data: json!({
            "count": 2,
            "results": [
                {"id": 1, "title": "Invoice A", "created": "2024-01-01"},
                {"id": 2, "title": "Invoice B", "created": "2024-01-02"}
            ]
        }),
        security: vec![],
    };

    let markdown = render_output(OutputMode::Markdown, &envelope).unwrap();
    assert!(markdown.contains("| id | title | created |"));
    assert!(!markdown.contains("## Security"));
    assert!(!markdown.contains("Mode:"));

    let json = render_output(OutputMode::Json, &envelope).unwrap();
    assert!(json.contains("\"command\": \"documents list\""));
    assert!(json.contains("\"security\": []"));
}

#[test]
fn documents_get_markdown_prefers_document_text_only() {
    let envelope = OutputEnvelope {
        mode: "markdown".to_string(),
        command: "documents get".to_string(),
        data: json!({
            "id": 42,
            "title": "Invoice",
            "content": "Line one\nLine two"
        }),
        security: vec![],
    };

    let markdown = render_output(OutputMode::Markdown, &envelope).unwrap();
    assert_eq!(markdown, "Line one\nLine two\n");
}

#[test]
fn status_terminal_output_uses_ascii_art_and_aligned_columns() {
    let envelope = OutputEnvelope {
        mode: "markdown".to_string(),
        command: "status".to_string(),
        data: json!({
            "status": "ok",
            "url": "https://paperless.example.com",
            "response": {
                "database": {
                    "error": null,
                    "migration_status": {
                        "latest_migration": "documents.1075_workflowaction_order",
                        "unapplied_migrations": []
                    },
                    "status": "OK",
                    "type": "sqlite",
                    "url": "/data/local/data/db.sqlite3"
                },
                "install_type": "kubernetes",
                "pngx_version": "2.20.15",
                "server_os": "Linux-aarch64",
                "storage": {
                    "available": 11047809318912_u64,
                    "total": 11984892329984_u64
                },
                "tasks": {
                    "celery_error": "Error connecting to celery, check logs for more detail.",
                    "celery_status": "ERROR",
                    "celery_url": "celery@paperless-secret-pod",
                    "classifier_error": null,
                    "classifier_last_trained": "2026-07-16T08:05:04.176750Z",
                    "classifier_status": "OK",
                    "index_error": null,
                    "index_last_modified": "2026-07-16T00:00:04.495783+01:00",
                    "index_status": "OK",
                    "redis_error": null,
                    "redis_status": "OK",
                    "redis_url": "redis://internal.example:6379",
                    "sanity_check_error": null,
                    "sanity_check_last_run": "2026-07-11T23:31:51.188118Z",
                    "sanity_check_status": "OK"
                }
            }
        }),
        security: vec![],
    };

    let terminal = render_output(OutputMode::Markdown, &envelope).unwrap();
    assert!(terminal.contains(" ____   _    ____  _____ ____"));
    assert!(terminal.contains("Paperless-ngx 2.20.15  |  Kubernetes"));
    assert!(terminal.contains("| Database     | OK     | SQLite; migrations current"));
    assert!(terminal.contains(
        "| Celery       | ERROR  | Error: Error connecting to celery, check logs for more detail."
    ));
    assert!(terminal.contains("| Classifier   | OK     | Trained 2026-07-16 08:05 UTC"));
    assert!(terminal.contains("Storage    10.0 TiB available of 10.9 TiB (92.2% free)"));
    assert!(terminal.contains("Migration  documents.1075_workflowaction_order"));
    assert!(!terminal.contains("Needs attention"));
    assert!(!terminal.contains("https://paperless.example.com"));
    assert!(!terminal.contains("**"));
    assert!(!terminal.contains('`'));
    assert!(!terminal.contains("redis://"));
    assert!(!terminal.contains("celery@"));
    assert!(!terminal.contains("null"));
    assert!(!terminal.contains("11047809318912"));

    let lines = terminal.lines().collect::<Vec<_>>();
    let header_index = lines
        .iter()
        .position(|line| line.starts_with("| COMPONENT"))
        .unwrap();
    let mut table_rows = vec![lines[header_index]];
    for line in &lines[header_index + 2..] {
        if line.starts_with('+') {
            break;
        }
        table_rows.push(line);
    }
    let expected_boundaries = table_rows[0]
        .char_indices()
        .filter_map(|(index, character)| (character == '|').then_some(index))
        .collect::<Vec<_>>();
    assert!(table_rows.iter().all(|row| {
        row.char_indices()
            .filter_map(|(index, character)| (character == '|').then_some(index))
            .collect::<Vec<_>>()
            == expected_boundaries
    }));

    let json = render_output(OutputMode::Json, &envelope).unwrap();
    assert!(json.contains("\"redis_url\": \"redis://internal.example:6379\""));
    assert!(json.contains("\"available\": 11047809318912"));
}

#[test]
fn document_text_representation_falls_back_cleanly() {
    let text = document_text_representation(&json!({
        "title": "Invoice",
        "original_file_name": "invoice.pdf"
    }));
    assert_eq!(text, "Invoice\nfile: invoice.pdf");
}

#[test]
fn tui_draws_documents_text_first_and_metadata_low() {
    let snapshot = DashboardSnapshot {
        project: json!({"status": "ok"}),
        documents: vec![
            DocumentSummary {
                id: 1,
                title: "Invoice A".to_string(),
                created: "2024-01-01".to_string(),
            },
            DocumentSummary {
                id: 2,
                title: "Invoice B".to_string(),
                created: "2024-01-02".to_string(),
            },
        ],
        latest_task: Some(TaskSummary {
            id: Some(1),
            status: "SUCCESS".to_string(),
            note: "consume.pdf".to_string(),
        }),
        security: vec![],
    };

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = TuiApp::from_snapshot(snapshot);
    app.loading_documents = false;
    app.loading_inspector = false;
    app.inspector_cache.insert(
        1,
        DocumentInspector {
            id: 1,
            title: "Invoice A".to_string(),
            text: "Line one\nLine two".to_string(),
            metadata: vec!["created 2024-01-01".to_string(), "pages 1".to_string()],
        },
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let contents = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(contents.contains("paperless-cli"));
    assert!(contents.contains("Documents"));
    assert!(contents.contains("Invoice A"));
    assert!(contents.contains("Line one"));
    assert!(app
        .selected_metadata()
        .join(" ")
        .contains("latest task SUCCESS"));
    assert!(app.selected_metadata().join(" ").contains("security clear"));
}

#[test]
fn security_reviewer_uses_gpt_54_and_polls_findings() {
    let profile = SecurityAgentProfile::security_reviewer();
    assert_eq!(profile.model, "gpt-5.4");

    let auditor = SecurityAuditor::new(profile, Duration::from_millis(10));
    let state = Arc::new(Mutex::new(AuditState::new(
        Some("http://paperless.example.com".to_string()),
        false,
        None,
    )));
    let receiver = auditor.spawn(state.clone());
    let findings = receiver.recv_timeout(Duration::from_millis(50)).unwrap();

    assert!(findings
        .iter()
        .any(|finding| finding.severity == Severity::High));
    assert!(findings
        .iter()
        .any(|finding| finding.title.contains("plain HTTP")));

    state.lock().unwrap().base_url = Some("https://paperless.example.com".to_string());
    state.lock().unwrap().config_permissions_restricted = true;
    let cleaned = receiver.recv_timeout(Duration::from_millis(50)).unwrap();
    assert!(cleaned.is_empty());
}

#[test]
fn security_reviewer_allows_ipv6_loopback_http() {
    let profile = SecurityAgentProfile::security_reviewer();
    let findings = SecurityAuditor::new(profile, Duration::from_millis(10)).review_once(
        &AuditState::new(Some("http://[::1]:8000".to_string()), true, None),
    );

    assert!(findings
        .iter()
        .all(|finding| !finding.title.contains("plain HTTP")));
}
