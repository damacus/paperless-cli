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
