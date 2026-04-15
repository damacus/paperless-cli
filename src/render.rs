use serde_json::Value;

use crate::config::OutputMode;
use crate::error::AppError;
use crate::services::OutputEnvelope;

pub fn render_output(mode: OutputMode, envelope: &OutputEnvelope) -> Result<String, AppError> {
    match mode {
        OutputMode::Json => Ok(serde_json::to_string_pretty(envelope)?),
        OutputMode::Markdown => Ok(render_markdown(envelope)),
        OutputMode::Tui => Err(AppError::Message(
            "TUI output is only available in interactive mode.".to_string(),
        )),
    }
}

pub fn render_markdown(envelope: &OutputEnvelope) -> String {
    if matches!(
        envelope.command.as_str(),
        "documents get" | "documents content" | "pdf read"
    ) {
        if let Some(text) = extract_document_text(&envelope.data) {
            return format!("{text}\n");
        }
    }

    let mut lines = Vec::new();
    render_value_as_markdown(&mut lines, &envelope.data, 0);

    if !envelope.security.is_empty() {
        lines.push(String::new());
        lines.push("## Security".to_string());
        lines.push(String::new());
        for finding in &envelope.security {
            lines.push(format!(
                "- `{}` {}: {}",
                severity_label(finding.severity.as_ref()),
                finding.title,
                finding.detail
            ));
            lines.push(format!("  Remediation: {}", finding.remediation));
        }
    }

    lines.join("\n")
}

fn extract_document_text(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    let candidate = ["content", "text", "document_text", "body"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))?;
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn render_value_as_markdown(lines: &mut Vec<String>, value: &Value, depth: usize) {
    let indent = "  ".repeat(depth);
    match value {
        Value::Null => lines.push(format!("{indent}- null")),
        Value::Bool(boolean) => lines.push(format!("{indent}- `{boolean}`")),
        Value::Number(number) => lines.push(format!("{indent}- `{number}`")),
        Value::String(text) => lines.push(format!("{indent}- {}", text)),
        Value::Array(items) => {
            if items.is_empty() {
                lines.push(format!("{indent}- []"));
                return;
            }

            if let Some(table) = try_render_table(items) {
                lines.extend(table);
                return;
            }

            for item in items {
                render_value_as_markdown(lines, item, depth + 1);
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                lines.push(format!("{indent}- {{}}"));
                return;
            }

            if let Some(results) = map.get("results").and_then(Value::as_array) {
                for (key, nested) in map {
                    if key == "results" {
                        continue;
                    }

                    match nested {
                        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                            let inline = nested
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| nested.to_string());
                            lines.push(format!("{indent}- **{key}**: {inline}"));
                        }
                        _ => {
                            lines.push(format!("{indent}- **{key}**:"));
                            render_value_as_markdown(lines, nested, depth + 1);
                        }
                    }
                }

                lines.push(format!("{indent}- **results**:"));
                if let Some(table) = try_render_table(results) {
                    lines.extend(table);
                } else {
                    render_value_as_markdown(lines, &Value::Array(results.clone()), depth + 1);
                }
                return;
            }

            for (key, nested) in map {
                match nested {
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                        let inline = nested
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| nested.to_string());
                        lines.push(format!("{indent}- **{key}**: {inline}"));
                    }
                    _ => {
                        lines.push(format!("{indent}- **{key}**:"));
                        render_value_as_markdown(lines, nested, depth + 1);
                    }
                }
            }
        }
    }
}

fn try_render_table(items: &[Value]) -> Option<Vec<String>> {
    let first = items.first()?.as_object()?;
    let preferred = ["id", "title", "name", "created", "status"];
    let mut headers = preferred
        .iter()
        .filter(|key| first.contains_key(**key))
        .map(|key| key.to_string())
        .collect::<Vec<_>>();

    for key in first.keys() {
        if !headers.iter().any(|existing| existing == key) {
            headers.push(key.clone());
        }
        if headers.len() >= 6 {
            break;
        }
    }

    if headers.is_empty() {
        return None;
    }

    let mut lines = vec![
        format!("| {} |", headers.join(" | ")),
        format!(
            "| {} |",
            headers
                .iter()
                .map(|_| "---")
                .collect::<Vec<_>>()
                .join(" | ")
        ),
    ];

    for item in items {
        let row = item.as_object()?;
        let values = headers
            .iter()
            .map(|header| {
                row.get(header)
                    .map(|value| match value {
                        Value::String(text) => text.replace('|', "\\|"),
                        _ => value.to_string().replace('|', "\\|"),
                    })
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        lines.push(format!("| {} |", values.join(" | ")));
    }

    Some(lines)
}

fn severity_label(severity: &str) -> &'static str {
    match severity {
        "critical" => "CRITICAL",
        "high" => "HIGH",
        "medium" => "MEDIUM",
        _ => "LOW",
    }
}

trait SeverityName {
    fn as_ref(&self) -> &str;
}

impl SeverityName for crate::security::Severity {
    fn as_ref(&self) -> &str {
        match self {
            crate::security::Severity::Critical => "critical",
            crate::security::Severity::High => "high",
            crate::security::Severity::Medium => "medium",
            crate::security::Severity::Low => "low",
        }
    }
}
