use std::collections::BTreeMap;
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

pub trait Transport {
    fn send(&self, request: RequestData) -> Result<ResponseData, AppError>;
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

    pub fn config(&self) -> &AppConfig {
        &self.config
    }
}

impl Transport for ReqwestTransport {
    fn send(&self, request: RequestData) -> Result<ResponseData, AppError> {
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
        } else if let Some(file) = request.multipart_file {
            let mut form = Form::new();
            for (key, value) in request.multipart_fields {
                form = form.text(key, value);
            }

            let part = Part::bytes(file.bytes)
                .file_name(file.file_name)
                .mime_str(&file.mime)
                .map_err(|error| AppError::Message(error.to_string()))?;
            form = form.part(file.field_name, part);
            builder = builder.multipart(form);
        }

        let response = builder.send()?;
        let status = response.status().as_u16();
        let url = response.url().to_string();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string().to_lowercase(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let body = response.bytes()?.to_vec();

        Ok(ResponseData {
            status,
            url,
            headers,
            body,
        })
    }
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
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn get_json(&self, path: &str, query: Vec<(String, String)>) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Get, path);
        request.query = query;
        let response = self.transport.send(request)?;
        self.decode_json_response(response)
    }

    pub fn get_json_absolute(&self, url: String) -> Result<Value, AppError> {
        let request = RequestData::absolute(HttpMethod::Get, url);
        let response = self.transport.send(request)?;
        self.decode_json_response(response)
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
        self.decode_json_response(response)
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
        self.decode_json_response(response)
    }

    pub fn patch_json(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Patch, path);
        request.json_body = Some(body);
        let response = self.transport.send(request)?;
        self.decode_json_response(response)
    }

    pub fn put_json(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let mut request = RequestData::new(HttpMethod::Put, path);
        request.json_body = Some(body);
        let response = self.transport.send(request)?;
        self.decode_json_response(response)
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
        mut query: Vec<(String, String)>,
    ) -> Result<Vec<Value>, AppError> {
        if !query.iter().any(|(key, _)| key == "page_size") {
            query.push(("page_size".to_string(), "100".to_string()));
        }

        let mut next = Some(Endpoint::Relative(path.to_string()));
        let mut params = query;
        let mut results = Vec::new();

        while let Some(endpoint) = next.take() {
            let mut request = match endpoint {
                Endpoint::Relative(path) => RequestData::new(HttpMethod::Get, path),
                Endpoint::Absolute(url) => RequestData::absolute(HttpMethod::Get, url),
            };
            request.query = params.clone();

            let response = self.transport.send(request)?;
            check_response(&response)?;
            let current_url = response.url.clone();
            let value = parse_json_body(&response)?;
            if let Some(items) = value.as_array() {
                results.extend(items.iter().cloned());
                break;
            }

            let response_object = value.as_object().ok_or_else(|| {
                AppError::Message("Paginated response must be an object or list".to_string())
            })?;

            if let Some(items) = response_object.get("results").and_then(Value::as_array) {
                results.extend(items.iter().cloned());
            }

            next = response_object
                .get("next")
                .and_then(Value::as_str)
                .map(|url| validate_next_page_url(&current_url, url))
                .transpose()?
                .map(Endpoint::Absolute);
            params.clear();
        }

        Ok(results)
    }

    pub fn ping(&self, base_url: &str) -> Result<Value, AppError> {
        let response = self.get_json("status/", Vec::new())?;
        Ok(json!({
            "status": "ok",
            "url": base_url,
            "response": response,
        }))
    }

    fn decode_json_response(&self, response: ResponseData) -> Result<Value, AppError> {
        check_response(&response)?;
        if response.body.is_empty() || response.status == 204 {
            return Ok(json!({}));
        }
        parse_json_body(&response)
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
    serde_json::from_slice::<Value>(body)
        .ok()
        .map(|json| {
            if let Some(detail) = json.get("detail").and_then(Value::as_str) {
                return detail.to_string();
            }
            json.to_string()
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).to_string())
}

pub fn multipart_file_from_path(path: &Path) -> Result<MultipartFile, AppError> {
    if !path.exists() {
        return Err(AppError::FileMissing(path.to_path_buf()));
    }

    let bytes = std::fs::read(path)?;
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    Ok(MultipartFile {
        field_name: "document".to_string(),
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("document.bin")
            .to_string(),
        mime,
        bytes,
    })
}
