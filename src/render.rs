use serde_json::Value;

use crate::config::OutputMode;
use crate::error::AppError;
use crate::services::OutputEnvelope;

#[derive(Clone, Copy)]
enum CellStyle {
    Value,
    YesNo,
}

#[derive(Clone, Copy)]
struct CollectionColumn {
    header: &'static str,
    keys: &'static [&'static str],
    style: CellStyle,
}

const DOCUMENT_COLUMNS: &[CollectionColumn] = &[
    CollectionColumn {
        header: "ID",
        keys: &["id"],
        style: CellStyle::Value,
    },
    CollectionColumn {
        header: "DATE",
        keys: &["created", "created_date"],
        style: CellStyle::Value,
    },
    CollectionColumn {
        header: "TITLE",
        keys: &["title"],
        style: CellStyle::Value,
    },
];

const TAG_COLUMNS: &[CollectionColumn] = &[
    CollectionColumn {
        header: "ID",
        keys: &["id"],
        style: CellStyle::Value,
    },
    CollectionColumn {
        header: "NAME",
        keys: &["name"],
        style: CellStyle::Value,
    },
    CollectionColumn {
        header: "INBOX",
        keys: &["is_inbox_tag"],
        style: CellStyle::YesNo,
    },
];

const NAMED_RESOURCE_COLUMNS: &[CollectionColumn] = &[
    CollectionColumn {
        header: "ID",
        keys: &["id"],
        style: CellStyle::Value,
    },
    CollectionColumn {
        header: "NAME",
        keys: &["name"],
        style: CellStyle::Value,
    },
];

const TASK_COLUMNS: &[CollectionColumn] = &[
    CollectionColumn {
        header: "ID",
        keys: &["task_id", "id"],
        style: CellStyle::Value,
    },
    CollectionColumn {
        header: "STATUS",
        keys: &["status"],
        style: CellStyle::Value,
    },
    CollectionColumn {
        header: "FILE",
        keys: &["task_file_name", "file_name"],
        style: CellStyle::Value,
    },
];

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

    let mut lines =
        if let Some(lines) = render_collection_terminal(&envelope.command, &envelope.data) {
            lines
        } else if envelope.command == "status" {
            render_status_terminal(&envelope.data).unwrap_or_else(|| fallback_lines(&envelope.data))
        } else {
            fallback_lines(&envelope.data)
        };

    if !envelope.security.is_empty() {
        lines.push(String::new());
        lines.push("SECURITY".to_string());
        lines.push("--------".to_string());
        for finding in &envelope.security {
            lines.push(format!(
                "{:<8} {}: {}",
                severity_label(finding.severity.as_ref()),
                finding.title,
                finding.detail
            ));
            lines.push(format!("         Remediation: {}", finding.remediation));
        }
    }

    lines.join("\n")
}

fn fallback_lines(data: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    render_value_as_markdown(&mut lines, data, 0);
    lines
}

fn render_collection_terminal(command: &str, data: &Value) -> Option<Vec<String>> {
    match command {
        "documents list" | "documents search" | "search query" => {
            render_tabular_collection(data, "document", "documents", DOCUMENT_COLUMNS)
        }
        "tags list" => render_tabular_collection(data, "tag", "tags", TAG_COLUMNS),
        "correspondents list" => render_tabular_collection(
            data,
            "correspondent",
            "correspondents",
            NAMED_RESOURCE_COLUMNS,
        ),
        "document-types list" => render_tabular_collection(
            data,
            "document type",
            "document types",
            NAMED_RESOURCE_COLUMNS,
        ),
        "tasks list" => render_tabular_collection(data, "task", "tasks", TASK_COLUMNS),
        "search autocomplete" => render_suggestions(data),
        _ => None,
    }
}

fn render_tabular_collection(
    data: &Value,
    singular: &str,
    plural: &str,
    columns: &[CollectionColumn],
) -> Option<Vec<String>> {
    let (items, total) = collection_items(data)?;
    let shown = items.len();
    let mut lines = vec![collection_summary(shown, total, singular, plural)];

    if let Some(corrected_query) = data
        .get("corrected_query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        lines.push(format!("Corrected query: {}", single_line(corrected_query)));
    }

    if items.is_empty() {
        return Some(lines);
    }

    let rows = items
        .iter()
        .map(|item| {
            let object = item.as_object()?;
            Some(
                columns
                    .iter()
                    .map(|column| collection_cell(object, *column))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let headers = columns
        .iter()
        .map(|column| column.header.to_string())
        .collect::<Vec<_>>();

    lines.push(String::new());
    lines.extend(render_text_table(&headers, &rows));
    Some(lines)
}

fn collection_items(data: &Value) -> Option<(&[Value], u64)> {
    match data {
        Value::Array(items) => Some((items, items.len() as u64)),
        Value::Object(object) => {
            let items = object.get("results")?.as_array()?;
            let total = object
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(items.len() as u64);
            Some((items, total))
        }
        _ => None,
    }
}

fn collection_summary(shown: usize, total: u64, singular: &str, plural: &str) -> String {
    if shown == 0 {
        return format!("No {plural} found.");
    }
    if shown as u64 != total {
        return format!("{shown} shown of {total} {plural}");
    }
    if total == 1 {
        format!("1 {singular}")
    } else {
        format!("{total} {plural}")
    }
}

fn collection_cell(object: &serde_json::Map<String, Value>, column: CollectionColumn) -> String {
    let value = column.keys.iter().find_map(|key| object.get(*key));
    match (column.style, value) {
        (CellStyle::YesNo, Some(Value::Bool(true))) => "yes".to_string(),
        (CellStyle::YesNo, Some(Value::Bool(false))) => "no".to_string(),
        (_, Some(Value::String(text))) => {
            let text = single_line(text);
            if text.is_empty() {
                "-".to_string()
            } else {
                text
            }
        }
        (_, Some(Value::Number(number))) => number.to_string(),
        (_, Some(Value::Bool(boolean))) => boolean.to_string(),
        _ => "-".to_string(),
    }
}

fn render_text_table(headers: &[String], rows: &[Vec<String>]) -> Vec<String> {
    let widths = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.chars().count())
                .chain(std::iter::once(header.chars().count()))
                .max()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();

    let mut lines = vec![render_text_row(headers, &widths)];
    let separator = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>();
    lines.push(render_text_row(&separator, &widths));
    lines.extend(rows.iter().map(|row| render_text_row(row, &widths)));
    lines
}

fn render_text_row(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            if index + 1 == cells.len() {
                cell.clone()
            } else {
                format!("{cell:<width$}", width = widths[index])
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn render_suggestions(data: &Value) -> Option<Vec<String>> {
    let suggestions = data.as_array()?;
    if suggestions.is_empty() {
        return Some(vec!["No suggestions found.".to_string()]);
    }

    let mut lines = vec![format!(
        "{} {}",
        suggestions.len(),
        if suggestions.len() == 1 {
            "suggestion"
        } else {
            "suggestions"
        }
    )];
    lines.push(String::new());
    lines.extend(suggestions.iter().map(|suggestion| {
        suggestion
            .as_str()
            .map(single_line)
            .filter(|suggestion| !suggestion.is_empty())
            .unwrap_or_else(|| "-".to_string())
    }));
    Some(lines)
}

fn render_status_terminal(data: &Value) -> Option<Vec<String>> {
    let status = data.as_object()?;
    let mut lines = vec![
        " ____   _    ____  _____ ____  _     _____ ____ ____".to_string(),
        "|  _ \\ / \\  |  _ \\| ____|  _ \\| |   | ____/ ___/ ___|".to_string(),
        "| |_) / _ \\ | |_) |  _| | |_) | |   |  _| \\___ \\___ \\".to_string(),
        "|  __/ ___ \\|  __/| |___|  _ <| |___| |___ ___) |__) |".to_string(),
        "|_| /_/   \\_\\_|   |_____|_| \\_\\_____|_____|____/____/".to_string(),
        String::new(),
    ];

    let Some(response) = status.get("response").and_then(Value::as_object) else {
        if status.get("connected").and_then(Value::as_bool) == Some(false) {
            lines.push("Status  NOT CONFIGURED".to_string());
            if let Some(message) = status.get("message").and_then(Value::as_str) {
                lines.push(format!("Action  {}", message.replace('`', "")));
            }
            return Some(lines);
        }
        return None;
    };

    let mut identity = Vec::new();
    if let Some(version) = response
        .get("pngx_version")
        .or_else(|| response.get("version"))
        .and_then(Value::as_str)
    {
        identity.push(format!("Paperless-ngx {version}"));
    }
    if let Some(install_type) = response.get("install_type").and_then(Value::as_str) {
        identity.push(display_name(install_type));
    }
    if !identity.is_empty() {
        lines.push(identity.join("  |  "));
    }

    let mut components = Vec::new();
    if let Some(database) = response.get("database").and_then(Value::as_object) {
        let mut details = Vec::new();
        if let Some(database_type) = database.get("type").and_then(Value::as_str) {
            details.push(display_name(database_type));
        }
        if let Some(migrations) = database.get("migration_status").and_then(Value::as_object) {
            if let Some(unapplied) = migrations
                .get("unapplied_migrations")
                .and_then(Value::as_array)
            {
                details.push(match unapplied.len() {
                    0 => "migrations current".to_string(),
                    1 => "1 unapplied migration".to_string(),
                    count => format!("{count} unapplied migrations"),
                });
            }
        }
        push_status_component(
            &mut components,
            "Database",
            database.get("status"),
            details,
            database.get("error"),
        );
    }

    if let Some(tasks) = response.get("tasks").and_then(Value::as_object) {
        push_status_component(
            &mut components,
            "Celery",
            tasks.get("celery_status"),
            Vec::new(),
            tasks.get("celery_error"),
        );
        push_status_component(
            &mut components,
            "Redis",
            tasks.get("redis_status"),
            Vec::new(),
            tasks.get("redis_error"),
        );
        push_status_component(
            &mut components,
            "Classifier",
            tasks.get("classifier_status"),
            detail_with_timestamp(
                "Trained",
                tasks.get("classifier_last_trained").and_then(Value::as_str),
            ),
            tasks.get("classifier_error"),
        );
        push_status_component(
            &mut components,
            "Search index",
            tasks.get("index_status"),
            detail_with_timestamp(
                "Updated",
                tasks.get("index_last_modified").and_then(Value::as_str),
            ),
            tasks.get("index_error"),
        );
        push_status_component(
            &mut components,
            "Sanity check",
            tasks.get("sanity_check_status"),
            detail_with_timestamp(
                "Last run",
                tasks.get("sanity_check_last_run").and_then(Value::as_str),
            ),
            tasks.get("sanity_check_error"),
        );
    }

    if !components.is_empty() {
        lines.push(String::new());
        lines.extend(render_status_table(&components));
    }

    let mut facts = Vec::new();
    if let Some(storage) = response.get("storage").and_then(Value::as_object) {
        if let (Some(available), Some(total)) = (
            storage.get("available").and_then(Value::as_u64),
            storage.get("total").and_then(Value::as_u64),
        ) {
            let percentage = if total == 0 {
                String::new()
            } else {
                format!(" ({:.1}% free)", available as f64 / total as f64 * 100.0)
            };
            facts.push((
                "Storage".to_string(),
                format!(
                    "{} available of {}{percentage}",
                    format_bytes(available),
                    format_bytes(total)
                ),
            ));
        }
    }
    if let Some(latest_migration) = response
        .get("database")
        .and_then(Value::as_object)
        .and_then(|database| database.get("migration_status"))
        .and_then(Value::as_object)
        .and_then(|migrations| migrations.get("latest_migration"))
        .and_then(Value::as_str)
    {
        facts.push(("Migration".to_string(), latest_migration.to_string()));
    }
    if let Some(server_os) = response.get("server_os").and_then(Value::as_str) {
        facts.push(("Server".to_string(), server_os.to_string()));
    }

    if !facts.is_empty() {
        lines.push(String::new());
        let label_width = facts
            .iter()
            .map(|(label, _)| label.chars().count())
            .max()
            .unwrap_or_default();
        lines.extend(facts.into_iter().map(|(label, value)| {
            format!("{label:<label_width$}  {value}", label_width = label_width)
        }));
    }

    Some(lines)
}

fn render_status_table(rows: &[(String, String, String)]) -> Vec<String> {
    let component_width = rows
        .iter()
        .map(|(component, _, _)| component.chars().count())
        .chain(std::iter::once("COMPONENT".len()))
        .max()
        .unwrap_or_default();
    let status_width = rows
        .iter()
        .map(|(_, status, _)| status.chars().count())
        .chain(std::iter::once("STATUS".len()))
        .max()
        .unwrap_or_default();
    let details_width = rows
        .iter()
        .map(|(_, _, details)| details.chars().count())
        .chain(std::iter::once("DETAILS".len()))
        .max()
        .unwrap_or_default();

    let border = format!(
        "+-{:-<component_width$}-+-{:-<status_width$}-+-{:-<details_width$}-+",
        "",
        "",
        "",
        component_width = component_width,
        status_width = status_width,
        details_width = details_width,
    );
    let mut lines = vec![
        border.clone(),
        format!(
            "| {:<component_width$} | {:<status_width$} | {:<details_width$} |",
            "COMPONENT",
            "STATUS",
            "DETAILS",
            component_width = component_width,
            status_width = status_width,
            details_width = details_width,
        ),
        border.clone(),
    ];

    for (component, status, details) in rows {
        lines.push(format!(
            "| {:<component_width$} | {:<status_width$} | {:<details_width$} |",
            single_line(component),
            single_line(status),
            single_line(details),
            component_width = component_width,
            status_width = status_width,
            details_width = details_width,
        ));
    }
    lines.push(border);
    lines
}

fn push_status_component(
    components: &mut Vec<(String, String, String)>,
    name: &str,
    status: Option<&Value>,
    mut details: Vec<String>,
    error: Option<&Value>,
) {
    let error = error.filter(|value| non_null(value));
    if status.is_none() && error.is_none() {
        return;
    }

    if let Some(error) = error {
        details.push(format!("Error: {}", display_value(error)));
    }
    let status = status
        .and_then(Value::as_str)
        .map(|status| status.to_ascii_uppercase())
        .unwrap_or_else(|| "ERROR".to_string());
    components.push((
        name.to_string(),
        status,
        if details.is_empty() {
            "-".to_string()
        } else {
            details.join("; ")
        },
    ));
}

fn detail_with_timestamp(label: &str, timestamp: Option<&str>) -> Vec<String> {
    timestamp
        .map(|timestamp| vec![format!("{label} {}", format_timestamp(timestamp))])
        .unwrap_or_default()
}

fn format_timestamp(timestamp: &str) -> String {
    let Some((date, time)) = timestamp.split_once('T') else {
        return timestamp.to_string();
    };
    let (clock, zone) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, Some("UTC"))
    } else if let Some(index) = time.find(['+', '-']) {
        (&time[..index], Some(&time[index..]))
    } else {
        (time, None)
    };
    let clock = clock.split('.').next().unwrap_or(clock);
    let clock = clock.get(..5).unwrap_or(clock);
    match zone {
        Some(zone) => format!("{date} {clock} {zone}"),
        None => format!("{date} {clock}"),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn display_name(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "sqlite" => "SQLite".to_string(),
        "postgres" | "postgresql" => "PostgreSQL".to_string(),
        "kubernetes" => "Kubernetes".to_string(),
        "docker" => "Docker".to_string(),
        _ => {
            let mut characters = value.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        }
    }
}

fn display_value(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn non_null(value: &Value) -> bool {
    !value.is_null()
}

fn single_line(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "/")
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
