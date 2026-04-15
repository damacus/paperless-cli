use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::api::{Endpoint, HttpMethod, RequestData, ResponseData, Transport};
use crate::config::{AppConfig, OutputMode};
use crate::error::AppError;

const DEMO_BASE_URL: &str = "https://demo.paperless.local";

#[derive(Clone, Default)]
pub struct DemoTransport;

impl DemoTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Transport for DemoTransport {
    fn send(&self, request: RequestData) -> Result<ResponseData, AppError> {
        let (path, query) = match request.endpoint {
            Endpoint::Relative(path) => (path, request.query),
            Endpoint::Absolute(url) => {
                let parsed = reqwest::Url::parse(&url)
                    .map_err(|_| AppError::Message(format!("Invalid demo URL: {url}")))?;
                let path = parsed
                    .path()
                    .trim_start_matches("/api/")
                    .trim_start_matches('/')
                    .to_string();
                let query = parsed
                    .query_pairs()
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect::<Vec<_>>();
                (path, query)
            }
        };

        dispatch_demo_request(request.method, &path, &query, request.json_body)
    }
}

pub fn demo_config(output: OutputMode) -> AppConfig {
    AppConfig::new(DEMO_BASE_URL, "demo-token-value", output)
        .expect("demo config should always be valid")
}

fn dispatch_demo_request(
    method: HttpMethod,
    path: &str,
    query: &[(String, String)],
    json_body: Option<Value>,
) -> Result<ResponseData, AppError> {
    let normalized = path.trim_matches('/');
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    match (method, segments.as_slice()) {
        (HttpMethod::Get, ["status"]) => ok_json(
            normalized,
            json!({
                "version": "demo-2026.4",
                "demo": true,
            }),
        ),
        (HttpMethod::Get, ["statistics"]) => ok_json(
            normalized,
            json!({
                "documents_total": demo_documents().len(),
                "documents_inbox": 2,
                "storage_size": "42 MB",
                "demo": true,
            }),
        ),
        (HttpMethod::Get, ["documents"]) => paginated_documents(query),
        (HttpMethod::Get, ["documents", id]) => document_detail(id),
        (HttpMethod::Get, ["documents", id, "download"]) => {
            document_binary(id, "download", "application/pdf")
        }
        (HttpMethod::Get, ["documents", id, "preview"]) => {
            document_binary(id, "preview", "application/pdf")
        }
        (HttpMethod::Get, ["documents", id, "thumb"]) => document_binary(id, "thumb", "image/png"),
        (HttpMethod::Post, ["documents", "post_document"]) => ok_json(
            normalized,
            json!({
                "task_id": "demo-task-upload-001",
                "status": "queued",
                "demo": true,
            }),
        ),
        (HttpMethod::Patch, ["documents", id]) | (HttpMethod::Put, ["documents", id]) => {
            patch_document(id, json_body)
        }
        (HttpMethod::Delete, ["documents", _id]) => empty_response(normalized),
        (HttpMethod::Get, ["tags"]) => ok_json(normalized, Value::Array(demo_tags())),
        (HttpMethod::Get, ["tags", id]) => item_detail(id, demo_tags(), normalized),
        (HttpMethod::Post, ["tags"]) => create_tag_response(json_body),
        (HttpMethod::Patch, ["tags", id]) => {
            patch_named_item(id, demo_tags(), normalized, json_body)
        }
        (HttpMethod::Delete, ["tags", _id]) => empty_response(normalized),
        (HttpMethod::Get, ["correspondents"]) => {
            ok_json(normalized, Value::Array(demo_correspondents()))
        }
        (HttpMethod::Get, ["correspondents", id]) => {
            item_detail(id, demo_correspondents(), normalized)
        }
        (HttpMethod::Post, ["correspondents"]) => create_named_matcher(normalized, json_body),
        (HttpMethod::Delete, ["correspondents", _id]) => empty_response(normalized),
        (HttpMethod::Get, ["document_types"]) => {
            ok_json(normalized, Value::Array(demo_document_types()))
        }
        (HttpMethod::Get, ["document_types", id]) => {
            item_detail(id, demo_document_types(), normalized)
        }
        (HttpMethod::Post, ["document_types"]) => create_named_matcher(normalized, json_body),
        (HttpMethod::Delete, ["document_types", _id]) => empty_response(normalized),
        (HttpMethod::Get, ["tasks"]) => ok_json(normalized, Value::Array(demo_tasks())),
        (HttpMethod::Get, ["tasks", id]) => item_detail(id, demo_tasks(), normalized),
        (HttpMethod::Get, ["search"]) => search_documents(query),
        (HttpMethod::Get, ["search", "autocomplete"]) => autocomplete(query),
        (HttpMethod::Post, ["documents", "bulk_download"]) => ok_binary(
            normalized,
            vec![0x50, 0x4b, 0x03, 0x04, b'D', b'E', b'M', b'O'],
            "application/zip",
            Some("attachment; filename=\"paperless-demo-export.zip\"".to_string()),
        ),
        _ => Err(AppError::Http {
            status: 404,
            url: demo_url(normalized),
            message: format!("Demo endpoint not implemented: {normalized}"),
        }),
    }
}

fn paginated_documents(query: &[(String, String)]) -> Result<ResponseData, AppError> {
    let all = filter_documents(query);
    let page = query_value(query, "page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    let page_size = query_value(query, "page_size")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(25)
        .max(1);
    let start = (page - 1) * page_size;
    let end = (start + page_size).min(all.len());
    let items = if start < all.len() {
        all[start..end].to_vec()
    } else {
        Vec::new()
    };
    let next = (end < all.len()).then(|| {
        format!(
            "{}/api/documents/?page={}&page_size={}",
            DEMO_BASE_URL,
            page + 1,
            page_size
        )
    });

    ok_json(
        &format!("documents/?page={page}&page_size={page_size}"),
        json!({
            "count": all.len(),
            "next": next,
            "previous": (page > 1).then(|| format!("{}/api/documents/?page={}&page_size={}", DEMO_BASE_URL, page - 1, page_size)),
            "results": items,
        }),
    )
}

fn document_detail(id: &str) -> Result<ResponseData, AppError> {
    let id = parse_id(id)?;
    let document = demo_documents()
        .into_iter()
        .find(|document| document.get("id").and_then(Value::as_u64) == Some(id))
        .ok_or_else(|| AppError::Http {
            status: 404,
            url: demo_url(&format!("documents/{id}/")),
            message: "Resource not found.".to_string(),
        })?;
    ok_json(&format!("documents/{id}/"), document)
}

fn document_binary(id: &str, asset: &str, mime: &str) -> Result<ResponseData, AppError> {
    let id = parse_id(id)?;
    let document = demo_documents()
        .into_iter()
        .find(|document| document.get("id").and_then(Value::as_u64) == Some(id))
        .ok_or_else(|| AppError::Http {
            status: 404,
            url: demo_url(&format!("documents/{id}/{asset}/")),
            message: "Resource not found.".to_string(),
        })?;
    let title = document
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("demo-document");
    let filename = if asset == "thumb" {
        format!("{}.png", slugify(title))
    } else {
        format!("{}.pdf", slugify(title))
    };
    let bytes = if asset == "thumb" {
        b"PNG DEMO".to_vec()
    } else {
        b"%PDF-1.4\n% DEMO\n".to_vec()
    };
    ok_binary(
        &format!("documents/{id}/{asset}/"),
        bytes,
        mime,
        Some(format!("attachment; filename=\"{filename}\"")),
    )
}

fn patch_document(id: &str, json_body: Option<Value>) -> Result<ResponseData, AppError> {
    let id = parse_id(id)?;
    let mut document = demo_documents()
        .into_iter()
        .find(|document| document.get("id").and_then(Value::as_u64) == Some(id))
        .ok_or_else(|| AppError::Http {
            status: 404,
            url: demo_url(&format!("documents/{id}/")),
            message: "Resource not found.".to_string(),
        })?;

    if let Some(Value::Object(patch)) = json_body {
        for (key, value) in patch {
            document[key] = value;
        }
    }

    ok_json(&format!("documents/{id}/"), document)
}

fn item_detail(id: &str, items: Vec<Value>, path: &str) -> Result<ResponseData, AppError> {
    let id = parse_id(id)?;
    let item = items
        .into_iter()
        .find(|item| item.get("id").and_then(Value::as_u64) == Some(id))
        .ok_or_else(|| AppError::Http {
            status: 404,
            url: demo_url(path),
            message: "Resource not found.".to_string(),
        })?;
    ok_json(path, item)
}

fn create_tag_response(json_body: Option<Value>) -> Result<ResponseData, AppError> {
    let body = json_body.unwrap_or_else(|| json!({}));
    ok_json(
        "tags/",
        json!({
            "id": 999,
            "name": body.get("name").and_then(Value::as_str).unwrap_or("demo-tag"),
            "color": body.get("color").and_then(Value::as_str).unwrap_or("#7dd3fc"),
            "is_inbox_tag": body.get("is_inbox_tag").and_then(Value::as_bool).unwrap_or(false),
            "demo": true,
        }),
    )
}

fn patch_named_item(
    id: &str,
    items: Vec<Value>,
    path: &str,
    json_body: Option<Value>,
) -> Result<ResponseData, AppError> {
    let id = parse_id(id)?;
    let mut item = items
        .into_iter()
        .find(|item| item.get("id").and_then(Value::as_u64) == Some(id))
        .ok_or_else(|| AppError::Http {
            status: 404,
            url: demo_url(path),
            message: "Resource not found.".to_string(),
        })?;
    if let Some(Value::Object(patch)) = json_body {
        for (key, value) in patch {
            item[key] = value;
        }
    }
    ok_json(path, item)
}

fn create_named_matcher(path: &str, json_body: Option<Value>) -> Result<ResponseData, AppError> {
    let body = json_body.unwrap_or_else(|| json!({}));
    ok_json(
        path,
        json!({
            "id": 999,
            "name": body.get("name").and_then(Value::as_str).unwrap_or("demo-item"),
            "match": body.get("match").and_then(Value::as_str).unwrap_or(""),
            "matching_algorithm": body.get("matching_algorithm").and_then(Value::as_i64).unwrap_or(0),
            "demo": true,
        }),
    )
}

fn search_documents(query: &[(String, String)]) -> Result<ResponseData, AppError> {
    let search = query_value(query, "query")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let results = demo_documents()
        .into_iter()
        .filter(|document| matches_query(document, &search))
        .collect::<Vec<_>>();
    ok_json(
        "search/",
        json!({
            "count": results.len(),
            "results": results,
        }),
    )
}

fn autocomplete(query: &[(String, String)]) -> Result<ResponseData, AppError> {
    let term = query_value(query, "term")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let limit = query_value(query, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10);
    let suggestions = demo_documents()
        .into_iter()
        .filter_map(|document| {
            let title = document.get("title").and_then(Value::as_str)?;
            title
                .to_ascii_lowercase()
                .contains(&term)
                .then(|| json!(title))
        })
        .take(limit)
        .collect::<Vec<_>>();
    ok_json("search/autocomplete/", Value::Array(suggestions))
}

fn ok_json(path: &str, body: Value) -> Result<ResponseData, AppError> {
    Ok(ResponseData {
        status: 200,
        url: demo_url(path),
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body: serde_json::to_vec(&body)?,
    })
}

fn ok_binary(
    path: &str,
    body: Vec<u8>,
    mime: &str,
    content_disposition: Option<String>,
) -> Result<ResponseData, AppError> {
    let mut headers = BTreeMap::from([("content-type".to_string(), mime.to_string())]);
    if let Some(value) = content_disposition {
        headers.insert("content-disposition".to_string(), value);
    }
    Ok(ResponseData {
        status: 200,
        url: demo_url(path),
        headers,
        body,
    })
}

fn empty_response(path: &str) -> Result<ResponseData, AppError> {
    Ok(ResponseData {
        status: 204,
        url: demo_url(path),
        headers: BTreeMap::new(),
        body: Vec::new(),
    })
}

fn demo_url(path: &str) -> String {
    format!("{}/api/{}", DEMO_BASE_URL, path.trim_start_matches('/'))
}

fn query_value(query: &[(String, String)], key: &str) -> Option<String> {
    query
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.clone())
}

fn parse_id(raw: &str) -> Result<u64, AppError> {
    raw.parse::<u64>()
        .map_err(|_| AppError::Message(format!("Invalid demo identifier: {raw}")))
}

fn filter_documents(query: &[(String, String)]) -> Vec<Value> {
    let search = query_value(query, "query")
        .unwrap_or_default()
        .to_ascii_lowercase();
    demo_documents()
        .into_iter()
        .filter(|document| matches_query(document, &search))
        .collect()
}

fn matches_query(document: &Value, search: &str) -> bool {
    if search.is_empty() {
        return true;
    }

    [
        document
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        document
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        document
            .get("original_file_name")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ]
    .iter()
    .any(|field| field.to_ascii_lowercase().contains(search))
}

fn slugify(value: &str) -> String {
    value
        .chars()
        .map(|char| match char {
            'a'..='z' | 'A'..='Z' | '0'..='9' => char.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn demo_documents() -> Vec<Value> {
    vec![
        json!({
            "id": 301,
            "title": "Quarterly Tax Summary Q1 2026",
            "created": "2026-04-02",
            "content": "Quarterly tax summary for the period ending March 2026.\nRevenue: £84,210.13\nTax due: £6,418.22",
            "original_file_name": "quarterly-tax-summary-q1-2026.pdf",
            "page_count": 3,
            "mime_type": "application/pdf",
            "tags": [22, 59],
            "correspondent": 11,
            "document_type": 7,
        }),
        json!({
            "id": 302,
            "title": "Consulting Invoice March 2026",
            "created": "2026-03-26",
            "content": "Invoice for March consulting services.\nAmount due: £12,500.00\nDue date: 2026-04-10",
            "original_file_name": "consulting-invoice-2026-03.pdf",
            "page_count": 1,
            "mime_type": "application/pdf",
            "tags": [22],
            "correspondent": 12,
            "document_type": 8,
        }),
        json!({
            "id": 303,
            "title": "Project Delivery Invoice 25-26-010",
            "created": "2026-04-10",
            "content": "Invoice REF-25-26-010\nProject delivery and advisory services.\nTotal: £7,840.00",
            "original_file_name": "project-delivery-invoice-25-26-010.pdf",
            "page_count": 2,
            "mime_type": "application/pdf",
            "tags": [60, 22],
            "correspondent": 13,
            "document_type": 8,
        }),
        json!({
            "id": 304,
            "title": "Business Account Statement April 2026",
            "created": "2026-04-14",
            "content": "Opening balance £18,902.14\nClosing balance £24,411.91\n22 transactions recorded.",
            "original_file_name": "business-account-statement-april-2026.pdf",
            "page_count": 5,
            "mime_type": "application/pdf",
            "tags": [61],
            "correspondent": 14,
            "document_type": 9,
        }),
        json!({
            "id": 305,
            "title": "Equipment Purchase Receipt",
            "created": "2026-02-18",
            "content": "Receipt for office equipment purchase.\nVendor: Hardware Supplier Ltd\nTotal paid: £2,149.00",
            "original_file_name": "equipment-purchase-receipt.pdf",
            "page_count": 1,
            "mime_type": "application/pdf",
            "tags": [59],
            "correspondent": 15,
            "document_type": 10,
        }),
        json!({
            "id": 306,
            "title": "Office Rent Invoice April 2026",
            "created": "2026-04-01",
            "content": "Monthly office rent invoice.\nPeriod: April 2026\nAmount: £1,250.00",
            "original_file_name": "office-rent-invoice-april-2026.pdf",
            "page_count": 1,
            "mime_type": "application/pdf",
            "tags": [22, 61],
            "correspondent": 16,
            "document_type": 8,
        }),
    ]
}

fn demo_tags() -> Vec<Value> {
    vec![
        json!({"id": 22, "name": "finance", "color": "#38bdf8", "is_inbox_tag": false}),
        json!({"id": 59, "name": "tax", "color": "#f59e0b", "is_inbox_tag": false}),
        json!({"id": 60, "name": "TODO", "color": "#f97316", "is_inbox_tag": true}),
        json!({"id": 61, "name": "banking", "color": "#10b981", "is_inbox_tag": false}),
    ]
}

fn demo_correspondents() -> Vec<Value> {
    vec![
        json!({"id": 11, "name": "Revenue Authority", "match": "tax summary", "matching_algorithm": 0}),
        json!({"id": 12, "name": "Consulting Client Ltd", "match": "consulting invoice", "matching_algorithm": 0}),
        json!({"id": 13, "name": "Project Customer Ltd", "match": "project delivery", "matching_algorithm": 0}),
        json!({"id": 14, "name": "Business Bank", "match": "account statement", "matching_algorithm": 0}),
        json!({"id": 15, "name": "Hardware Supplier Ltd", "match": "equipment purchase", "matching_algorithm": 0}),
        json!({"id": 16, "name": "Office Landlord Ltd", "match": "office rent", "matching_algorithm": 0}),
    ]
}

fn demo_document_types() -> Vec<Value> {
    vec![
        json!({"id": 7, "name": "tax", "match": "vat return", "matching_algorithm": 0}),
        json!({"id": 8, "name": "invoice", "match": "invoice", "matching_algorithm": 0}),
        json!({"id": 9, "name": "statement", "match": "statement", "matching_algorithm": 0}),
        json!({"id": 10, "name": "receipt", "match": "receipt", "matching_algorithm": 0}),
    ]
}

fn demo_tasks() -> Vec<Value> {
    vec![
        json!({
            "id": 901,
            "status": "SUCCESS",
            "task_file_name": "project-delivery-invoice-25-26-010.pdf",
            "type": "consume_file",
            "result": "Imported into demo library"
        }),
        json!({
            "id": 902,
            "status": "PENDING",
            "task_file_name": "workspace-rent-april-2026.pdf",
            "type": "consume_file",
            "result": ""
        }),
    ]
}
