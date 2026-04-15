use anyhow::Result;
use clap::ValueEnum;
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Json,
    #[default]
    Markdown,
}

pub fn render<T: Serialize>(value: &T, format: OutputFormat) -> Result<String> {
    let json_value = serde_json::to_value(value)?;
    match format {
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json_value)?),
        OutputFormat::Markdown => Ok(render_markdown(&json_value)),
    }
}

fn render_markdown(value: &Value) -> String {
    let mut lines = Vec::new();
    render_value(value, 1, None, &mut lines);
    lines.join("\n").trim().to_string() + "\n"
}

fn render_value(value: &Value, depth: usize, label: Option<&str>, lines: &mut Vec<String>) {
    match value {
        Value::Object(object) => render_object(object, depth, label, lines),
        Value::Array(items) => render_array(items, depth, label, lines),
        scalar => {
            if let Some(key) = label {
                lines.push(format!("- {}: {}", key, scalar_to_string(scalar)));
            } else {
                lines.push(scalar_to_string(scalar));
            }
        }
    }
}

fn render_object(
    object: &Map<String, Value>,
    depth: usize,
    label: Option<&str>,
    lines: &mut Vec<String>,
) {
    if let Some(title) = label {
        lines.push(format!("{} {}", "#".repeat(depth), title));
    }

    for (key, value) in object {
        match value {
            Value::Object(_) | Value::Array(_) => {
                render_value(value, depth + 1, Some(key), lines);
            }
            scalar => {
                lines.push(format!("- {}: {}", key, scalar_to_string(scalar)));
            }
        }
    }
}

fn render_array(items: &[Value], depth: usize, label: Option<&str>, lines: &mut Vec<String>) {
    if let Some(title) = label {
        lines.push(format!("{} {}", "#".repeat(depth), title));
    }

    for (index, item) in items.iter().enumerate() {
        match item {
            Value::Object(object) => {
                let summary = object
                    .get("title")
                    .or_else(|| object.get("name"))
                    .or_else(|| object.get("status"))
                    .map(scalar_to_string)
                    .unwrap_or_else(|| format!("item {}", index + 1));
                lines.push(format!("## {}", summary));
                for (key, value) in object {
                    if matches!(value, Value::Object(_) | Value::Array(_)) {
                        render_value(value, depth + 2, Some(key), lines);
                    } else {
                        lines.push(format!("- {}: {}", key, scalar_to_string(value)));
                    }
                }
            }
            scalar => lines.push(format!("- {}", scalar_to_string(scalar))),
        }
    }
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(string) => string.clone(),
        _ => value.to_string(),
    }
}
