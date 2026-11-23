use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use reqwest::blocking::multipart::{Form, Part};
use serde_json::{json, Value};

use crate::config::AppConfig;
use crate::error::AppError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Endpoint {
    Relative(String),
    Absolute(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Patch,
    Put,
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultipartFile {
    pub field_name: String,
    pub file_name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestData {
    pub method: HttpMethod,
    pub endpoint: Endpoint,
    pub query: Vec<(String, String)>,
    pub json_body: Option<Value>,
    pub multipart_fields: Vec<(String, String)>,
    pub multipart_file: Option<MultipartFile>,
}

impl RequestData {
    pub fn new(method: HttpMethod, endpoint: impl Into<String>) -> Self {
        Self {
            method,
            endpoint: Endpoint::Relative(endpoint.into()),
            query: Vec::new(),
            json_body: None,
            multipart_fields: Vec::new(),
            multipart_file: None,
        }
    }

    pub fn absolute(method: HttpMethod, endpoint: impl Into<String>) -> Self {
        Self {
            method,
            endpoint: Endpoint::Absolute(endpoint.into()),
            query: Vec::new(),
            json_body: None,
            multipart_fields: Vec::new(),
            multipart_file: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseData {
    pub status: u16,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// Headers and readable body for a successful binary response.
pub struct DownloadResponse {
    pub headers: BTreeMap<String, String>,
    pub body: Box<dyn Read>,
}

impl DownloadResponse {
    fn from_buffered(response: ResponseData) -> Result<Self, AppError> {
        check_response(&response)?;
        Ok(Self {
            headers: response.headers,
            body: Box::new(Cursor::new(response.body)),
        })
    }
}

pub trait Transport {
    fn send(&self, request: RequestData) -> Result<ResponseData, AppError>;

    fn download(&self, request: RequestData) -> Result<DownloadResponse, AppError> {
        DownloadResponse::from_buffered(self.send(request)?)
    }

    fn upload_file(&self, mut request: RequestData, path: &Path) -> Result<ResponseData, AppError> {
        request.multipart_file = Some(multipart_file_from_path(path)?);
        self.send(request)
    }
}

#[derive(Clone)]
pub struct ReqwestTransport {
    config: AppConfig,
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    pub fn new(config: AppConfig) -> Result<Self, AppError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        Ok(Self { config, client })
    }

    #[must_use]
    pub const fn config(&self) -> &AppConfig {
        &self.config
    }
    fn send_http(
        &self,
        request: RequestData,
        streamed_file: Option<Part>,
    ) -> Result<reqwest::blocking::Response, AppError> {
        let url = match request.endpoint {
            Endpoint::Relative(path) => self.config.api_url(&path),
            Endpoint::Absolute(url) => url,
        };

        let mut builder = match request.method {
            HttpMethod::Get => self.client.get(&url),
            HttpMethod::Post => self.client.post(&url),
            HttpMethod::Patch => self.client.patch(&url),
            HttpMethod::Put => self.client.put(&url),
            HttpMethod::Delete => self.client.delete(&url),
        }
        .header("Authorization", format!("Token {}", self.config.token))
        .header("Accept", "application/json");

        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }

        if let Some(json_body) = request.json_body {
            builder = builder.json(&json_body);
        } else if streamed_file.is_some() || request.multipart_file.is_some() {
            let mut form = Form::new();
            for (key, value) in request.multipart_fields {
                form = form.text(key, value);
            }

            if let Some(part) = streamed_file {
                form = form.part("document", part);
            } else if let Some(file) = request.multipart_file {
                let part = Part::bytes(file.bytes)
                    .file_name(file.file_name)
                    .mime_str(&file.mime)
                    .map_err(|error| AppError::Message(error.to_string()))?;
                form = form.part(file.field_name, part);
            }
            builder = builder.multipart(form);
        }

        Ok(builder.send()?)
    }

    fn buffer_response(response: reqwest::blocking::Response) -> Result<ResponseData, AppError> {
        let status = response.status().as_u16();
        let url = response.url().to_string();
        let headers = response_headers(&response);
        let body = response.bytes()?.to_vec();

        Ok(ResponseData {
            status,
            url,
            headers,
            body,
        })
    }
}

impl Transport for ReqwestTransport {
    fn send(&self, request: RequestData) -> Result<ResponseData, AppError> {
        Self::buffer_response(self.send_http(request, None)?)
    }

    fn download(&self, request: RequestData) -> Result<DownloadResponse, AppError> {
        let response = self.send_http(request, None)?;
        if !response.status().is_success() {
            return DownloadResponse::from_buffered(Self::buffer_response(response)?);
        }
        Ok(DownloadResponse {
            headers: response_headers(&response),
            body: Box::new(response),
        })
    }

    fn upload_file(&self, mut request: RequestData, path: &Path) -> Result<ResponseData, AppError> {
        if !path.exists() {
            return Err(AppError::FileMissing(path.to_path_buf()));
        }
        if !fs::metadata(path)?.is_file() {
            request.multipart_file = Some(multipart_file_from_path(path)?);
            return self.send(request);
        }

        let file = File::open(path)?;
        let size = file.metadata()?.len();
        // Some readable pseudo-files report zero length even when they contain data.
        if size == 0 {
            request.multipart_file = Some(multipart_file_from_path(path)?);
            return self.send(request);
        }
        let reader = BufReader::with_capacity(128 * 1024, file.take(size));
        let part = Part::reader_with_length(reader, size)
            .file_name(multipart_filename(path).to_string())
            .mime_str(&multipart_mime(path))
            .map_err(|error| AppError::Message(error.to_string()))?;
        Self::buffer_response(self.send_http(request, Some(part))?)
    }
}

fn response_headers(response: &reqwest::blocking::Response) -> BTreeMap<String, String> {
    response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string().to_lowercase(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

pub fn fetch_token(base_url: &str, username: &str, password: &str) -> Result<String, AppError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let url = format!("{}/api/token/", base_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .form(&[("username", username), ("password", password)])
        .send()?;
    let status = response.status().as_u16();
    let body = response.bytes()?.to_vec();
    let response = ResponseData {
        status,
        url,
        headers: BTreeMap::new(),
        body,
    };

    if response.status != 200 {
        return Err(AppError::Http {
            status: response.status,
            url: response.url,
            message: "Failed to obtain token. Check your username and password.".to_string(),
        });
    }

    let json = parse_json_body(&response)?;
    let token = json
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Message("Token response missing `token`".to_string()))?;

    Ok(token.to_string())
}

#[derive(Clone)]
pub struct ApiClient<T: Transport> {
    transport: T,
}

impl<T: Transport> ApiClient<T> {
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }

    pub const fn transport(&self) -> &T {
        &self.transport
    }

    pub fn get_json(&self, path: &str, query: Vec<(String, String)>) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Get, path);
        request.query = query;
        let response = self.transport.send(request)?;
        Self::decode_json_response(&response)
    }

    pub fn get_json_absolute(&self, url: String) -> Result<Value, AppError> {
        let request = RequestData::absolute(HttpMethod::Get, url);
        let response = self.transport.send(request)?;
        Self::decode_json_response(&response)
    }

    pub fn get_raw(
        &self,
        path: &str,
        query: Vec<(String, String)>,
    ) -> Result<ResponseData, AppError> {
        let mut request = RequestData::new(HttpMethod::Get, path);
        request.query = query;
        let response = self.transport.send(request)?;
        check_response(&response)?;
        Ok(response)
    }

    pub fn post_json(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Post, path);
        request.json_body = Some(body);
        let response = self.transport.send(request)?;
        Self::decode_json_response(&response)
    }

    pub fn post_json_raw(&self, path: &str, body: Value) -> Result<ResponseData, AppError> {
        let mut request = RequestData::new(HttpMethod::Post, path);
        request.json_body = Some(body);
        let response = self.transport.send(request)?;
        check_response(&response)?;
        Ok(response)
    }

    pub fn post_multipart(
        &self,
        path: &str,
        fields: Vec<(String, String)>,
        file: MultipartFile,
    ) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Post, path);
        request.multipart_fields = fields;
        request.multipart_file = Some(file);
        let response = self.transport.send(request)?;
        Self::decode_json_response(&response)
    }

    pub(crate) fn post_multipart_path(
        &self,
        path: &str,
        fields: Vec<(String, String)>,
        upload: &Path,
    ) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Post, path);
        request.multipart_fields = fields;
        let response = self.transport.upload_file(request, upload)?;
        Self::decode_json_response(&response)
    }

    pub(crate) fn get_download(
        &self,
        path: &str,
        query: Vec<(String, String)>,
    ) -> Result<DownloadResponse, AppError> {
        let mut request = RequestData::new(HttpMethod::Get, path);
        request.query = query;
        self.transport.download(request)
    }

    pub(crate) fn post_download(
        &self,
        path: &str,
        body: Value,
    ) -> Result<DownloadResponse, AppError> {
        let mut request = RequestData::new(HttpMethod::Post, path);
        request.json_body = Some(body);
        self.transport.download(request)
    }

    pub fn patch_json(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Patch, path);
        request.json_body = Some(body);
        let response = self.transport.send(request)?;
        Self::decode_json_response(&response)
    }

    pub fn put_json(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Put, path);
        request.json_body = Some(body);
        let response = self.transport.send(request)?;
        Self::decode_json_response(&response)
    }

    pub fn delete(&self, path: &str) -> Result<(), AppError> {
        let request = RequestData::new(HttpMethod::Delete, path);
        let response = self.transport.send(request)?;
        check_response(&response)?;
        Ok(())
    }

    pub fn paginate(
        &self,
        path: &str,
        query: Vec<(String, String)>,
    ) -> Result<Vec<Value>, AppError> {
        self.paginate_up_to(path, query, usize::MAX)
    }

    pub(crate) fn paginate_up_to(
        &self,
        path: &str,
        mut query: Vec<(String, String)>,
        limit: usize,
    ) -> Result<Vec<Value>, AppError> {
        if !query.iter().any(|(key, _)| key == "page_size") {
            query.push(("page_size".to_string(), "100".to_string()));
        }

        let mut next = Some(Endpoint::Relative(path.to_string()));
        let mut params = query;
        let mut results = Vec::new();

        while let Some(endpoint) = next.take().filter(|_| results.len() < limit) {
            let mut request = match endpoint {
                Endpoint::Relative(path) => RequestData::new(HttpMethod::Get, path),
                Endpoint::Absolute(url) => RequestData::absolute(HttpMethod::Get, url),
            };
            request.query = std::mem::take(&mut params);

            let response = self.transport.send(request)?;
            check_response(&response)?;
            let remaining = limit - results.len();
            let mut page = match parse_json_body(&response)? {
                Value::Array(items) => {
                    results.extend(items.into_iter().take(remaining));
                    break;
                }
                Value::Object(page) => page,
                _ => {
                    return Err(AppError::Message(
                        "Paginated response must be an object or list".to_string(),
                    ))
                }
            };
            if let Some(Value::Array(items)) = page.remove("results") {
                results.extend(items.into_iter().take(remaining));
            }
            if results.len() == limit {
                break;
            }

            next = page
                .get("next")
                .and_then(Value::as_str)
                .map(|url| validate_next_page_url(&response.url, url))
                .transpose()?
                .map(Endpoint::Absolute);
        }

        Ok(results)
    }

    pub fn ping(&self, base_url: &str) -> Result<Value, AppError> {
        // Try /api/status/ first — it returns rich version/component data.
        // Paperless-ngx 3.x restricts this endpoint to users with the
        // view_status permission, so fall back to a lightweight documents
        // probe when we get a 403.
        match self.get_json("status/", Vec::new()) {
            Ok(response) => Ok(json!({
                "status": "ok",
                "url": base_url,
                "response": response,
            })),
            Err(AppError::Http { status: 403, .. }) => {
                let response = self.get_json(
                    "documents/",
                    vec![("page_size".to_string(), "1".to_string())],
                )?;
                Ok(json!({
                    "status": "ok",
                    "url": base_url,
                    "response": response,
                }))
            }
            Err(error) => Err(error),
        }
    }

    fn decode_json_response(response: &ResponseData) -> Result<Value, AppError> {
        check_response(response)?;
        if response.body.is_empty() || response.status == 204 {
            return Ok(json!({}));
        }
        parse_json_body(response)
    }
}

fn validate_next_page_url(current_url: &str, next_url: &str) -> Result<String, AppError> {
    let current = reqwest::Url::parse(current_url)
        .map_err(|_| AppError::Message(format!("Invalid current pagination URL: {current_url}")))?;
    let next = reqwest::Url::parse(next_url)
        .map_err(|_| AppError::Message(format!("Invalid pagination URL: {next_url}")))?;

    if current.host_str() != next.host_str() {
        return Err(AppError::Message(format!(
            "Refusing cross-origin pagination link: {next_url}"
        )));
    }

    if current.scheme() == next.scheme() {
        if current.port_or_known_default() != next.port_or_known_default() {
            return Err(AppError::Message(format!(
                "Refusing cross-origin pagination link: {next_url}"
            )));
        }
        return Ok(next_url.to_string());
    }

    if next.port().is_some() && current.port_or_known_default() != next.port_or_known_default() {
        return Err(AppError::Message(format!(
            "Refusing cross-origin pagination link: {next_url}"
        )));
    }

    let mut normalized = current;
    normalized.set_path(next.path());
    normalized.set_query(next.query());
    normalized.set_fragment(next.fragment());
    Ok(normalized.to_string())
}

pub fn parse_json_body(response: &ResponseData) -> Result<Value, AppError> {
    if response.body.is_empty() {
        return Ok(json!({}));
    }
    Ok(serde_json::from_slice(&response.body)?)
}

pub fn check_response(response: &ResponseData) -> Result<(), AppError> {
    match response.status {
        200..=299 => Ok(()),
        401 => Err(AppError::Http {
            status: response.status,
            url: response.url.clone(),
            message: "Authentication failed. Run `paperless login`.".to_string(),
        }),
        403 => Err(AppError::Http {
            status: response.status,
            url: response.url.clone(),
            message: "Permission denied.".to_string(),
        }),
        404 => Err(AppError::Http {
            status: response.status,
            url: response.url.clone(),
            message: "Resource not found.".to_string(),
        }),
        _ => {
            let message = parse_error_message(&response.body);
            Err(AppError::Http {
                status: response.status,
                url: response.url.clone(),
                message,
            })
        }
    }
}

fn parse_error_message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body).ok().map_or_else(
        || String::from_utf8_lossy(body).to_string(),
        |json| {
            if let Some(detail) = json.get("detail").and_then(Value::as_str) {
                return detail.to_string();
            }
            json.to_string()
        },
    )
}

pub fn multipart_file_from_path(path: &Path) -> Result<MultipartFile, AppError> {
    if !path.exists() {
        return Err(AppError::FileMissing(path.to_path_buf()));
    }

    Ok(MultipartFile {
        field_name: "document".to_string(),
        file_name: multipart_filename(path).to_string(),
        mime: multipart_mime(path),
        bytes: fs::read(path)?,
    })
}

fn multipart_filename(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.bin")
}

fn multipart_mime(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string()
}
