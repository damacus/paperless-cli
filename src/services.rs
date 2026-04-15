use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::api::{fetch_token, multipart_file_from_path, ApiClient, Transport};
use crate::config::{
    load_config, load_session, normalize_url, save_config, save_session, AppConfig, AppPaths,
    OutputMode, SessionState,
};
use crate::error::AppError;
use crate::security::{AuditState, SecurityFinding};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DocumentQuery {
    pub query: Option<String>,
    pub tag: Option<String>,
    pub tag_id: Option<u64>,
    pub correspondent: Option<String>,
    pub correspondent_id: Option<u64>,
    pub document_type: Option<String>,
    pub document_type_id: Option<u64>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub order_by: String,
    pub page_size: u64,
    pub page: u64,
}

impl DocumentQuery {
    pub fn new() -> Self {
        Self {
            order_by: "-created".to_string(),
            page_size: 25,
            page: 1,
            ..Self::default()
        }
    }

    pub fn to_query_pairs(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        if let Some(query) = &self.query {
            pairs.push(("query".to_string(), query.clone()));
        }
        if let Some(tag) = &self.tag {
            pairs.push(("tags__name__icontains".to_string(), tag.clone()));
        }
        if let Some(tag_id) = self.tag_id {
            pairs.push(("tags__id__in".to_string(), tag_id.to_string()));
        }
        if let Some(correspondent) = &self.correspondent {
            pairs.push((
                "correspondent__name__icontains".to_string(),
                correspondent.clone(),
            ));
        }
        if let Some(correspondent_id) = self.correspondent_id {
            pairs.push((
                "correspondent__id".to_string(),
                correspondent_id.to_string(),
            ));
        }
        if let Some(document_type) = &self.document_type {
            pairs.push((
                "document_type__name__icontains".to_string(),
                document_type.clone(),
            ));
        }
        if let Some(document_type_id) = self.document_type_id {
            pairs.push((
                "document_type__id".to_string(),
                document_type_id.to_string(),
            ));
        }
        if let Some(created_after) = &self.created_after {
            pairs.push(("created__date__gt".to_string(), created_after.clone()));
        }
        if let Some(created_before) = &self.created_before {
            pairs.push(("created__date__lt".to_string(), created_before.clone()));
        }
        pairs.push(("ordering".to_string(), self.order_by.clone()));
        pairs.push(("page_size".to_string(), self.page_size.to_string()));
        pairs.push(("page".to_string(), self.page.to_string()));
        pairs
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UploadRequest {
    pub path: PathBuf,
    pub title: Option<String>,
    pub correspondent_id: Option<u64>,
    pub document_type_id: Option<u64>,
    pub tag_ids: Vec<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UpdateRequest {
    pub title: Option<String>,
    pub correspondent_id: Option<u64>,
    pub document_type_id: Option<u64>,
    pub tag_ids: Option<Vec<u64>>,
    pub created: Option<String>,
    pub custom_fields: Option<Value>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TagUpdateRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub inbox: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OutputEnvelope {
    pub mode: String,
    pub command: String,
    pub data: Value,
    pub security: Vec<SecurityFinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardSnapshot {
    pub project: Value,
    pub documents: Vec<DocumentSummary>,
    pub latest_task: Option<TaskSummary>,
    pub security: Vec<SecurityFinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DocumentSummary {
    pub id: u64,
    pub title: String,
    pub created: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DocumentInspector {
    pub id: u64,
    pub title: String,
    pub text: String,
    pub metadata: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskSummary {
    pub id: Option<u64>,
    pub status: String,
    pub note: String,
}

pub fn init_connection<T: Transport>(
    client_factory: impl Fn(AppConfig) -> Result<ApiClient<T>, AppError>,
    paths: &AppPaths,
    base_url: &str,
    token: Option<String>,
    username: Option<String>,
    password: Option<String>,
    preferred_output: OutputMode,
) -> Result<AppConfig, AppError> {
    let normalized = normalize_url(base_url)?;
    let token = match token {
        Some(token) => token,
        None => {
            let username = username.ok_or(AppError::MissingCredentials)?;
            let password = password.ok_or(AppError::MissingCredentials)?;
            fetch_token(&normalized, &username, &password)?
        }
    };

    let config = AppConfig::new(normalized, token, preferred_output)?;
    let client = client_factory(config.clone())?;
    client.ping(&config.base_url)?;
    save_config(paths, &config)?;
    Ok(config)
}

pub fn connection_info<T: Transport>(
    client: &ApiClient<T>,
    config: &AppConfig,
    paths: &AppPaths,
) -> Result<Value, AppError> {
    let statistics = client
        .get_json("statistics/", Vec::new())
        .unwrap_or_else(|error| {
            json!({
                "error": error.to_string(),
            })
        });

    Ok(json!({
        "url": config.base_url,
        "token": config.masked_token(),
        "config_path": paths.config_path,
        "statistics": statistics,
    }))
}

pub fn load_runtime(paths: &AppPaths) -> Result<(AppConfig, SessionState), AppError> {
    let config = load_config(paths)?;
    let session = load_session(paths);
    Ok((config, session))
}

pub fn persist_session(paths: &AppPaths, session: &SessionState) -> Result<(), AppError> {
    save_session(paths, session)
}

pub fn ping<T: Transport>(client: &ApiClient<T>, config: &AppConfig) -> Result<Value, AppError> {
    client.ping(&config.base_url)
}

pub fn list_documents<T: Transport>(
    client: &ApiClient<T>,
    query: &DocumentQuery,
) -> Result<Value, AppError> {
    client.get_json("documents/", query.to_query_pairs())
}

pub fn list_all_document_summaries<T: Transport>(
    client: &ApiClient<T>,
    query: &DocumentQuery,
) -> Result<Vec<DocumentSummary>, AppError> {
    let items = client.paginate("documents/", query.to_query_pairs())?;
    Ok(items.iter().map(document_summary_from_value).collect())
}

pub fn document_summaries_from_response(value: &Value) -> Vec<DocumentSummary> {
    value
        .get("results")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(document_summary_from_value).collect())
        .unwrap_or_default()
}

pub fn next_page_url_from_response(value: &Value) -> Option<String> {
    value
        .get("next")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub fn get_document<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
) -> Result<Value, AppError> {
    client.get_json(&format!("documents/{document_id}/"), Vec::new())
}

pub fn get_document_content<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
) -> Result<Value, AppError> {
    let document = get_document(client, document_id)?;
    Ok(json!({
        "id": document_id,
        "title": document.get("title").and_then(Value::as_str).unwrap_or("Untitled"),
        "content": document_text_representation(&document),
    }))
}

pub fn get_document_inspector<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
) -> Result<DocumentInspector, AppError> {
    let document = get_document(client, document_id)?;
    Ok(document_inspector_from_value(&document))
}

pub fn search_documents<T: Transport>(
    client: &ApiClient<T>,
    mut query: DocumentQuery,
    search: String,
) -> Result<Value, AppError> {
    query.query = Some(search);
    list_documents(client, &query)
}

pub fn upload_document<T: Transport>(
    client: &ApiClient<T>,
    request: &UploadRequest,
) -> Result<Value, AppError> {
    let mut fields = Vec::new();
    if let Some(title) = &request.title {
        fields.push(("title".to_string(), title.clone()));
    }
    if let Some(correspondent_id) = request.correspondent_id {
        fields.push(("correspondent".to_string(), correspondent_id.to_string()));
    }
    if let Some(document_type_id) = request.document_type_id {
        fields.push(("document_type".to_string(), document_type_id.to_string()));
    }
    for tag_id in &request.tag_ids {
        fields.push(("tags".to_string(), tag_id.to_string()));
    }

    let file = multipart_file_from_path(&request.path)?;
    client.post_multipart("documents/post_document/", fields, file)
}

pub fn update_document<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
    request: &UpdateRequest,
) -> Result<Value, AppError> {
    let mut patch = Map::new();
    if let Some(title) = &request.title {
        patch.insert("title".to_string(), json!(title));
    }
    if let Some(correspondent_id) = request.correspondent_id {
        patch.insert("correspondent".to_string(), json!(correspondent_id));
    }
    if let Some(document_type_id) = request.document_type_id {
        patch.insert("document_type".to_string(), json!(document_type_id));
    }
    if let Some(tag_ids) = &request.tag_ids {
        patch.insert("tags".to_string(), json!(tag_ids));
    }
    if let Some(created) = &request.created {
        patch.insert("created".to_string(), json!(created));
    }
    if let Some(custom_fields) = &request.custom_fields {
        patch.insert("custom_fields".to_string(), custom_fields.clone());
    }

    if patch.is_empty() {
        return Err(AppError::NoFieldsToUpdate);
    }

    client.patch_json(&format!("documents/{document_id}/"), Value::Object(patch))
}

pub fn edit_document<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
    request: &UpdateRequest,
    add_tags: &[String],
    remove_tags: &[String],
) -> Result<Value, AppError> {
    if add_tags.is_empty() && remove_tags.is_empty() {
        return update_document(client, document_id, request);
    }

    let document = get_document(client, document_id)?;
    let existing_tags = document
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| tags.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut resolved_tags = existing_tags;
    for tag_id in resolve_tag_inputs(client, add_tags)? {
        if !resolved_tags.contains(&tag_id) {
            resolved_tags.push(tag_id);
        }
    }

    let removed = resolve_tag_inputs(client, remove_tags)?;
    resolved_tags.retain(|tag_id| !removed.contains(tag_id));

    let mut merged = request.clone();
    merged.tag_ids = Some(resolved_tags);
    update_document(client, document_id, &merged)
}

pub fn delete_document<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
) -> Result<(), AppError> {
    client.delete(&format!("documents/{document_id}/"))
}

pub fn download_document<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
    output_dir: &Path,
    original: bool,
) -> Result<PathBuf, AppError> {
    let params = if original {
        vec![("original".to_string(), "true".to_string())]
    } else {
        Vec::new()
    };
    download_asset(client, document_id, "download", output_dir, params)
}

pub fn download_preview<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
    output_dir: &Path,
) -> Result<PathBuf, AppError> {
    download_asset(client, document_id, "preview", output_dir, Vec::new())
}

pub fn download_thumbnail<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
    output_dir: &Path,
) -> Result<PathBuf, AppError> {
    download_asset(client, document_id, "thumb", output_dir, Vec::new())
}

pub fn list_tags<T: Transport>(client: &ApiClient<T>) -> Result<Value, AppError> {
    Ok(Value::Array(client.paginate("tags/", Vec::new())?))
}

pub fn get_tag<T: Transport>(client: &ApiClient<T>, tag_id: u64) -> Result<Value, AppError> {
    client.get_json(&format!("tags/{tag_id}/"), Vec::new())
}

pub fn create_tag<T: Transport>(
    client: &ApiClient<T>,
    name: String,
    color: String,
    inbox: bool,
) -> Result<Value, AppError> {
    client.post_json(
        "tags/",
        json!({
            "name": name,
            "color": color,
            "is_inbox_tag": inbox,
        }),
    )
}

pub fn update_tag<T: Transport>(
    client: &ApiClient<T>,
    tag_id: u64,
    request: &TagUpdateRequest,
) -> Result<Value, AppError> {
    let mut patch = Map::new();
    if let Some(name) = &request.name {
        patch.insert("name".to_string(), json!(name));
    }
    if let Some(color) = &request.color {
        patch.insert("color".to_string(), json!(color));
    }
    if let Some(inbox) = request.inbox {
        patch.insert("is_inbox_tag".to_string(), json!(inbox));
    }

    if patch.is_empty() {
        return Err(AppError::NoFieldsToUpdate);
    }

    client.patch_json(&format!("tags/{tag_id}/"), Value::Object(patch))
}

pub fn delete_tag<T: Transport>(client: &ApiClient<T>, tag_id: u64) -> Result<(), AppError> {
    client.delete(&format!("tags/{tag_id}/"))
}

pub fn list_correspondents<T: Transport>(client: &ApiClient<T>) -> Result<Value, AppError> {
    Ok(Value::Array(
        client.paginate("correspondents/", Vec::new())?,
    ))
}

pub fn get_correspondent<T: Transport>(
    client: &ApiClient<T>,
    correspondent_id: u64,
) -> Result<Value, AppError> {
    client.get_json(&format!("correspondents/{correspondent_id}/"), Vec::new())
}

pub fn create_correspondent<T: Transport>(
    client: &ApiClient<T>,
    name: String,
    matcher: String,
) -> Result<Value, AppError> {
    client.post_json(
        "correspondents/",
        json!({
            "name": name,
            "match": matcher,
            "matching_algorithm": 0,
        }),
    )
}

pub fn delete_correspondent<T: Transport>(
    client: &ApiClient<T>,
    correspondent_id: u64,
) -> Result<(), AppError> {
    client.delete(&format!("correspondents/{correspondent_id}/"))
}

pub fn list_document_types<T: Transport>(client: &ApiClient<T>) -> Result<Value, AppError> {
    Ok(Value::Array(
        client.paginate("document_types/", Vec::new())?,
    ))
}

pub fn get_document_type<T: Transport>(
    client: &ApiClient<T>,
    document_type_id: u64,
) -> Result<Value, AppError> {
    client.get_json(&format!("document_types/{document_type_id}/"), Vec::new())
}

pub fn create_document_type<T: Transport>(
    client: &ApiClient<T>,
    name: String,
    matcher: String,
) -> Result<Value, AppError> {
    client.post_json(
        "document_types/",
        json!({
            "name": name,
            "match": matcher,
            "matching_algorithm": 0,
        }),
    )
}

pub fn delete_document_type<T: Transport>(
    client: &ApiClient<T>,
    document_type_id: u64,
) -> Result<(), AppError> {
    client.delete(&format!("document_types/{document_type_id}/"))
}

pub fn list_tasks<T: Transport>(client: &ApiClient<T>) -> Result<Value, AppError> {
    Ok(Value::Array(client.paginate("tasks/", Vec::new())?))
}

pub fn get_task<T: Transport>(client: &ApiClient<T>, task_id: u64) -> Result<Value, AppError> {
    client.get_json(&format!("tasks/{task_id}/"), Vec::new())
}

pub fn query_search<T: Transport>(
    client: &ApiClient<T>,
    query: String,
    page_size: u64,
    page: u64,
) -> Result<Value, AppError> {
    client.get_json(
        "search/",
        vec![
            ("query".to_string(), query),
            ("page_size".to_string(), page_size.to_string()),
            ("page".to_string(), page.to_string()),
        ],
    )
}

pub fn autocomplete_search<T: Transport>(
    client: &ApiClient<T>,
    term: String,
    limit: u64,
) -> Result<Value, AppError> {
    client.get_json(
        "search/autocomplete/",
        vec![
            ("term".to_string(), term),
            ("limit".to_string(), limit.to_string()),
        ],
    )
}

pub fn bulk_download<T: Transport>(
    client: &ApiClient<T>,
    document_ids: &[u64],
    output_dir: &Path,
    original: bool,
) -> Result<Value, AppError> {
    let mut results = Vec::new();
    for document_id in document_ids {
        match download_document(client, *document_id, output_dir, original) {
            Ok(path) => results.push(json!({
                "doc_id": document_id,
                "status": "ok",
                "path": path,
            })),
            Err(error) => results.push(json!({
                "doc_id": document_id,
                "status": "error",
                "error": error.to_string(),
            })),
        }
    }
    Ok(Value::Array(results))
}

pub fn bulk_download_zip<T: Transport>(
    client: &ApiClient<T>,
    document_ids: &[u64],
    output_path: &Path,
    content: &str,
) -> Result<PathBuf, AppError> {
    let response = client.post_json_raw(
        "documents/bulk_download/",
        json!({
            "documents": document_ids,
            "content": content,
        }),
    )?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, &response.body)?;
    Ok(output_path.to_path_buf())
}

pub fn status<T: Transport>(client: &ApiClient<T>, config: &AppConfig) -> Result<Value, AppError> {
    let ping = ping(client, config)?;
    let statistics = client.get_json("statistics/", Vec::new())?;
    let tasks = list_tasks(client)?;
    Ok(json!({
        "project": ping,
        "statistics": statistics,
        "tasks": tasks,
    }))
}

pub fn dashboard<T: Transport>(
    client: &ApiClient<T>,
    config: &AppConfig,
    security: Vec<SecurityFinding>,
) -> Result<DashboardSnapshot, AppError> {
    let project = status(client, config)?;
    let documents = Vec::new();
    let latest_task = latest_task_summary(&list_tasks(client)?);

    Ok(DashboardSnapshot {
        project,
        documents,
        latest_task,
        security,
    })
}

pub fn document_summary_from_value(document: &Value) -> DocumentSummary {
    DocumentSummary {
        id: document
            .get("id")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        title: document
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled")
            .to_string(),
        created: document
            .get("created")
            .or_else(|| document.get("created_date"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
    }
}

pub fn document_inspector_from_value(document: &Value) -> DocumentInspector {
    let id = document
        .get("id")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let title = document
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_string();
    let text = document_text_representation(document);
    let mut metadata = Vec::new();

    if let Some(created) = document
        .get("created")
        .or_else(|| document.get("created_date"))
        .and_then(Value::as_str)
    {
        metadata.push(format!("created {created}"));
    }
    if let Some(filename) = document
        .get("original_file_name")
        .or_else(|| document.get("archived_file_name"))
        .and_then(Value::as_str)
    {
        metadata.push(format!("file {filename}"));
    }
    if let Some(page_count) = document.get("page_count").and_then(Value::as_u64) {
        metadata.push(format!("pages {page_count}"));
    }
    if let Some(mime) = document.get("mime_type").and_then(Value::as_str) {
        metadata.push(format!("mime {mime}"));
    }
    if let Some(tags) = document.get("tags").and_then(Value::as_array) {
        metadata.push(format!("tags {}", tags.len()));
    }

    DocumentInspector {
        id,
        title,
        text,
        metadata,
    }
}

pub fn document_text_representation(document: &Value) -> String {
    for key in ["content", "text", "document_text", "body"] {
        if let Some(text) = document.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    let mut lines = Vec::new();
    if let Some(title) = document.get("title").and_then(Value::as_str) {
        lines.push(title.to_string());
    }
    if let Some(filename) = document
        .get("original_file_name")
        .or_else(|| document.get("archived_file_name"))
        .and_then(Value::as_str)
    {
        lines.push(format!("file: {filename}"));
    }
    if let Some(note) = document.get("note").and_then(Value::as_str) {
        lines.push(note.to_string());
    }

    if lines.is_empty() {
        "No text content available for this document.".to_string()
    } else {
        lines.join("\n")
    }
}

pub fn latest_task_summary(tasks: &Value) -> Option<TaskSummary> {
    tasks
        .as_array()
        .and_then(|items| items.first())
        .map(|task| TaskSummary {
            id: task.get("id").and_then(Value::as_u64),
            status: task
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            note: task
                .get("task_file_name")
                .or_else(|| task.get("type"))
                .or_else(|| task.get("result"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
}

fn resolve_tag_inputs<T: Transport>(
    client: &ApiClient<T>,
    references: &[String],
) -> Result<Vec<u64>, AppError> {
    if references.is_empty() {
        return Ok(Vec::new());
    }

    let tags = list_tags(client)?;
    let tag_array = tags
        .as_array()
        .ok_or_else(|| AppError::Message("Expected tag list to be an array".to_string()))?;
    let mut resolved = Vec::new();

    for reference in references {
        if let Ok(tag_id) = reference.parse::<u64>() {
            resolved.push(tag_id);
            continue;
        }

        let maybe_id = tag_array.iter().find_map(|tag| {
            let name = tag.get("name").and_then(Value::as_str)?;
            if name == reference {
                tag.get("id").and_then(Value::as_u64)
            } else {
                None
            }
        });

        match maybe_id {
            Some(tag_id) => resolved.push(tag_id),
            None => {
                return Err(AppError::Message(format!(
                    "Unknown tag reference `{reference}`. Use an exact tag name or numeric ID."
                )))
            }
        }
    }

    Ok(resolved)
}

pub fn audit_state(
    paths: &AppPaths,
    config: Option<&AppConfig>,
    last_download: Option<String>,
) -> AuditState {
    AuditState::new(
        config.map(|config| config.base_url.clone()),
        paths.config_permissions_restricted(),
        last_download,
    )
}

fn download_asset<T: Transport>(
    client: &ApiClient<T>,
    document_id: u64,
    asset: &str,
    output_dir: &Path,
    params: Vec<(String, String)>,
) -> Result<PathBuf, AppError> {
    let response = client.get_raw(&format!("documents/{document_id}/{asset}/"), params)?;
    std::fs::create_dir_all(output_dir)?;

    let filename = response
        .headers
        .get("content-disposition")
        .and_then(|header| extract_download_filename(header))
        .map(|value| sanitize_filename(&value))
        .unwrap_or_else(|| format!("document_{document_id}_{asset}.bin"));
    let output_path = output_dir.join(filename);
    std::fs::write(&output_path, response.body)?;
    Ok(output_path)
}

fn extract_download_filename(content_disposition: &str) -> Option<String> {
    let mut fallback = None;

    for part in content_disposition.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("filename*=") {
            if let Some(decoded) = decode_rfc5987_filename(value).filter(|value| !value.is_empty())
            {
                return Some(decoded);
            }
        } else if fallback.is_none() {
            fallback = trimmed
                .strip_prefix("filename=")
                .map(|value| value.trim_matches('"').to_string())
                .filter(|value| !value.is_empty());
        }
    }

    fallback
}

fn decode_rfc5987_filename(value: &str) -> Option<String> {
    let trimmed = value.trim_matches('"');
    let encoded = trimmed
        .split_once("''")
        .map(|(_, encoded)| encoded)
        .unwrap_or(trimmed);
    percent_decode(encoded)
}

fn percent_decode(value: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.as_bytes().iter().copied();

    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next()?;
            let low = bytes.next()?;
            let pair = [high, low];
            let hex = std::str::from_utf8(&pair).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
        } else {
            decoded.push(byte);
        }
    }

    String::from_utf8(decoded).ok()
}

pub fn sanitize_filename(name: &str) -> String {
    let leaf = Path::new(name)
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("download.bin");

    let sanitized = leaf
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "download.bin".to_string()
    } else {
        sanitized
    }
}
