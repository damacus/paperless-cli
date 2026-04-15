use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use crate::client::PaperlessClient;
use crate::config::{Config, load_config, save_config};
use crate::models::{DashboardData, Document, DownloadResult, Paginated, SimpleItem, Task};

#[derive(Clone, Debug, Default)]
pub struct DocumentQuery {
    pub query: Option<String>,
    pub tag: Option<String>,
    pub tag_id: Option<i64>,
    pub correspondent: Option<String>,
    pub correspondent_id: Option<i64>,
    pub doc_type: Option<String>,
    pub doc_type_id: Option<i64>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub order_by: String,
    pub page_size: usize,
    pub page: usize,
}

impl DocumentQuery {
    pub fn to_params(&self) -> Vec<(String, String)> {
        let mut params = vec![
            ("page_size".to_string(), self.page_size.to_string()),
            ("page".to_string(), self.page.to_string()),
            ("ordering".to_string(), self.order_by.clone()),
        ];
        push_opt(&mut params, "query", self.query.clone());
        push_opt(&mut params, "tags__name__icontains", self.tag.clone());
        push_opt_num(&mut params, "tags__id__in", self.tag_id);
        push_opt(
            &mut params,
            "correspondent__name__icontains",
            self.correspondent.clone(),
        );
        push_opt_num(&mut params, "correspondent__id", self.correspondent_id);
        push_opt(
            &mut params,
            "document_type__name__icontains",
            self.doc_type.clone(),
        );
        push_opt_num(&mut params, "document_type__id", self.doc_type_id);
        push_opt(&mut params, "created__date__gt", self.created_after.clone());
        push_opt(&mut params, "created__date__lt", self.created_before.clone());
        params
    }
}

impl Default for DocumentQuery {
    fn default() -> Self {
        Self {
            query: None,
            tag: None,
            tag_id: None,
            correspondent: None,
            correspondent_id: None,
            doc_type: None,
            doc_type_id: None,
            created_after: None,
            created_before: None,
            order_by: "-created".to_string(),
            page_size: 25,
            page: 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PaperlessService {
    client: PaperlessClient,
}

impl PaperlessService {
    pub fn new(client: PaperlessClient) -> Self {
        Self { client }
    }

    pub fn from_saved_config() -> Result<Self> {
        let config = load_config()?;
        let client = PaperlessClient::new(config)?;
        Ok(Self::new(client))
    }

    pub async fn init_connection(
        url: &str,
        token: Option<String>,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<Value> {
        let token = match token {
            Some(token) => token,
            None => {
                let username = username.ok_or_else(|| {
                    anyhow!("Provide either --token or both --username and --password.")
                })?;
                let password = password.ok_or_else(|| {
                    anyhow!("Provide either --token or both --username and --password.")
                })?;
                PaperlessClient::exchange_token(url, &username, &password).await?
            }
        };

        let config = Config::new(url, token);
        let client = PaperlessClient::new(config.clone())?;
        let _: Value = client.ping().await?;
        let path = save_config(&config)?;

        Ok(json!({
            "status": "ok",
            "url": config.url,
            "config_path": path,
            "message": "Connection configured and verified."
        }))
    }

    pub async fn status() -> Value {
        match load_config() {
            Ok(config) => match PaperlessClient::new(config.clone()) {
                Ok(client) => match client.ping().await {
                    Ok(_) => json!({
                        "connected": true,
                        "url": config.url,
                        "token": config.masked_token(),
                        "message": "Connected"
                    }),
                    Err(error) => json!({
                        "connected": false,
                        "url": config.url,
                        "token": config.masked_token(),
                        "message": error.to_string()
                    }),
                },
                Err(error) => json!({
                    "connected": false,
                    "message": error.to_string()
                }),
            },
            Err(_) => json!({
                "connected": false,
                "message": "Paperless is not configured. Run `paperless login`."
            }),
        }
    }

    pub async fn connection_info() -> Result<Value> {
        let config = load_config()?;
        let client = PaperlessClient::new(config.clone())?;
        let statistics = client
            .get::<Value>("statistics/", &[])
            .await
            .unwrap_or_else(|error| json!({ "error": error.to_string() }));

        Ok(json!({
            "url": config.url,
            "token": config.masked_token(),
            "statistics": statistics
        }))
    }

    pub async fn ping(&self) -> Result<Value> {
        let _ = self.client.ping().await?;
        Ok(json!({
            "status": "ok",
            "url": self.client.config().url,
        }))
    }

    pub async fn dashboard(&self) -> DashboardData {
        let (ping_result, documents, tasks, tags) = tokio::join!(
            self.client.ping(),
            self.list_documents(DocumentQuery::default()),
            self.list_tasks(),
            self.list_tags(),
        );

        match ping_result {
            Ok(_) => DashboardData {
                connected: true,
                url: Some(self.client.config().url.clone()),
                documents: documents.map(|page| page.results).unwrap_or_default(),
                tasks: tasks.unwrap_or_default(),
                tags: tags.unwrap_or_default(),
                message: "Connected".to_string(),
            },
            Err(error) => DashboardData {
                connected: false,
                url: Some(self.client.config().url.clone()),
                documents: Vec::new(),
                tasks: Vec::new(),
                tags: Vec::new(),
                message: error.to_string(),
            },
        }
    }

    pub async fn list_documents(&self, query: DocumentQuery) -> Result<Paginated<Document>> {
        self.client.get("documents/", &query.to_params()).await
    }

    pub async fn get_document(&self, document_id: i64) -> Result<Document> {
        self.client.get(&format!("documents/{document_id}/"), &[]).await
    }

    pub async fn search_documents(&self, query: DocumentQuery) -> Result<Paginated<Document>> {
        self.list_documents(query).await
    }

    pub async fn query_search(
        &self,
        query: &str,
        page_size: usize,
        page: usize,
    ) -> Result<Value> {
        self.client
            .get(
                "search/",
                &[
                    ("query".to_string(), query.to_string()),
                    ("page_size".to_string(), page_size.to_string()),
                    ("page".to_string(), page.to_string()),
                ],
            )
            .await
    }

    pub async fn autocomplete_search(&self, term: &str, limit: usize) -> Result<Vec<String>> {
        self.client
            .get(
                "search/autocomplete/",
                &[
                    ("term".to_string(), term.to_string()),
                    ("limit".to_string(), limit.to_string()),
                ],
            )
            .await
    }

    pub async fn list_tasks(&self) -> Result<Vec<Task>> {
        self.client.paginate("tasks/", &[]).await
    }

    pub async fn get_task(&self, task_id: i64) -> Result<Task> {
        self.client.get(&format!("tasks/{task_id}/"), &[]).await
    }

    pub async fn list_tags(&self) -> Result<Vec<SimpleItem>> {
        self.client.paginate("tags/", &[]).await
    }

    pub async fn get_tag(&self, tag_id: i64) -> Result<SimpleItem> {
        self.client.get(&format!("tags/{tag_id}/"), &[]).await
    }

    pub async fn create_tag(
        &self,
        name: &str,
        color: &str,
        is_inbox_tag: bool,
    ) -> Result<SimpleItem> {
        self.client
            .post_json(
                "tags/",
                &json!({
                    "name": name,
                    "color": color,
                    "is_inbox_tag": is_inbox_tag,
                }),
            )
            .await
    }

    pub async fn delete_tag(&self, tag_id: i64) -> Result<()> {
        self.client.delete(&format!("tags/{tag_id}/")).await
    }

    pub async fn list_correspondents(&self) -> Result<Vec<SimpleItem>> {
        self.client.paginate("correspondents/", &[]).await
    }

    pub async fn get_correspondent(&self, correspondent_id: i64) -> Result<SimpleItem> {
        self.client
            .get(&format!("correspondents/{correspondent_id}/"), &[])
            .await
    }

    pub async fn create_correspondent(
        &self,
        name: &str,
        r#match: &str,
        matching_algorithm: i64,
    ) -> Result<SimpleItem> {
        self.client
            .post_json(
                "correspondents/",
                &json!({
                    "name": name,
                    "match": r#match,
                    "matching_algorithm": matching_algorithm,
                }),
            )
            .await
    }

    pub async fn delete_correspondent(&self, correspondent_id: i64) -> Result<()> {
        self.client
            .delete(&format!("correspondents/{correspondent_id}/"))
            .await
    }

    pub async fn list_doc_types(&self) -> Result<Vec<SimpleItem>> {
        self.client.paginate("document_types/", &[]).await
    }

    pub async fn get_doc_type(&self, doc_type_id: i64) -> Result<SimpleItem> {
        self.client
            .get(&format!("document_types/{doc_type_id}/"), &[])
            .await
    }

    pub async fn create_doc_type(
        &self,
        name: &str,
        r#match: &str,
        matching_algorithm: i64,
    ) -> Result<SimpleItem> {
        self.client
            .post_json(
                "document_types/",
                &json!({
                    "name": name,
                    "match": r#match,
                    "matching_algorithm": matching_algorithm,
                }),
            )
            .await
    }

    pub async fn delete_doc_type(&self, doc_type_id: i64) -> Result<()> {
        self.client
            .delete(&format!("document_types/{doc_type_id}/"))
            .await
    }

    pub async fn update_document(
        &self,
        document_id: i64,
        patch: BTreeMap<String, Value>,
    ) -> Result<Document> {
        if patch.is_empty() {
            return Err(anyhow!("No fields to update provided."));
        }

        self.client
            .patch_json(&format!("documents/{document_id}/"), &patch)
            .await
    }

    pub async fn delete_document(&self, document_id: i64) -> Result<()> {
        self.client.delete(&format!("documents/{document_id}/")).await
    }

    pub async fn upload_document(
        &self,
        file_path: &Path,
        title: Option<String>,
        correspondent_id: Option<i64>,
        document_type_id: Option<i64>,
        tag_ids: Vec<i64>,
    ) -> Result<Value> {
        if !file_path.exists() {
            return Err(anyhow!("File not found: {}", file_path.display()));
        }

        let bytes = tokio::fs::read(file_path).await?;
        let filename = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("Upload path does not have a valid filename"))?;
        let mime = mime_guess::from_path(file_path).first_or_octet_stream();

        let mut form = Form::new().part(
            "document",
            Part::bytes(bytes)
                .file_name(filename.to_string())
                .mime_str(mime.as_ref())?,
        );

        if let Some(title) = title {
            form = form.text("title", title);
        }
        if let Some(correspondent_id) = correspondent_id {
            form = form.text("correspondent", correspondent_id.to_string());
        }
        if let Some(document_type_id) = document_type_id {
            form = form.text("document_type", document_type_id.to_string());
        }
        for tag_id in tag_ids {
            form = form.text("tags", tag_id.to_string());
        }

        self.client
            .post_form_multipart("documents/post_document/", form)
            .await
    }

    pub async fn download_document(
        &self,
        document_id: i64,
        output_dir: &Path,
        original: bool,
    ) -> Result<PathBuf> {
        let params = if original {
            vec![("original".to_string(), "true".to_string())]
        } else {
            Vec::new()
        };
        self.download_asset(document_id, "download", output_dir, &params)
            .await
    }

    pub async fn download_preview(&self, document_id: i64, output_dir: &Path) -> Result<PathBuf> {
        self.download_asset(document_id, "preview", output_dir, &[]).await
    }

    pub async fn download_thumbnail(
        &self,
        document_id: i64,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        self.download_asset(document_id, "thumb", output_dir, &[]).await
    }

    pub async fn download_asset(
        &self,
        document_id: i64,
        asset: &str,
        output_dir: &Path,
        params: &[(String, String)],
    ) -> Result<PathBuf> {
        let response = self
            .client
            .get_raw(&format!("documents/{document_id}/{asset}/"), params)
            .await?;
        let headers = response.headers().clone();
        let bytes = response.bytes().await?;
        tokio::fs::create_dir_all(output_dir).await?;
        let filename = safe_download_name(
            headers
                .get(reqwest::header::CONTENT_DISPOSITION)
                .and_then(|value| value.to_str().ok()),
            &format!("document_{document_id}_{asset}"),
        );
        let path = output_dir.join(filename);
        tokio::fs::write(&path, bytes).await?;
        Ok(path)
    }

    pub async fn bulk_download(
        &self,
        document_ids: &[i64],
        output_dir: &Path,
    ) -> Vec<DownloadResult> {
        let mut results = Vec::new();
        for document_id in document_ids {
            match self.download_document(*document_id, output_dir, false).await {
                Ok(path) => results.push(DownloadResult {
                    doc_id: *document_id,
                    path: Some(path.display().to_string()),
                    status: "ok".to_string(),
                    error: None,
                }),
                Err(error) => results.push(DownloadResult {
                    doc_id: *document_id,
                    path: None,
                    status: "error".to_string(),
                    error: Some(error.to_string()),
                }),
            }
        }
        results
    }

    pub async fn bulk_download_zip(
        &self,
        document_ids: &[i64],
        output_path: &Path,
        content: &str,
    ) -> Result<PathBuf> {
        let body = json!({
            "documents": document_ids,
            "content": content,
        });
        let response = self
            .client
            .get_raw("documents/bulk_download/", &[])
            .await
            .err()
            .map(|_| ())
            .is_none();
        let _ = response;
        let raw = self
            .client
            .post_optional_json::<Value, _>("documents/bulk_download/", &body)
            .await;

        if raw.is_ok() {
            return Err(anyhow!(
                "Bulk ZIP endpoint returned JSON. Expected a binary archive."
            ));
        }

        let response = self
            .client
            .get_raw("documents/bulk_download/", &[])
            .await
            .err()
            .map(|_| ())
            .is_none();
        let _ = response;

        let response = self
            .client
            .config()
            .api_url("documents/bulk_download/");
        let url = response;

        let client = reqwest::Client::new();
        let archive = client
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Token {}", self.client.config().token),
            )
            .json(&body)
            .send()
            .await?;
        let archive = archive.bytes().await?;

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(output_path, archive).await?;
        Ok(output_path.to_path_buf())
    }
}

fn push_opt(params: &mut Vec<(String, String)>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        params.push((key.to_string(), value));
    }
}

fn push_opt_num(params: &mut Vec<(String, String)>, key: &str, value: Option<i64>) {
    if let Some(value) = value {
        params.push((key.to_string(), value.to_string()));
    }
}

fn safe_download_name(content_disposition: Option<&str>, fallback: &str) -> String {
    let candidate = content_disposition
        .and_then(extract_filename)
        .map(sanitize_filename)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_string());

    if candidate.is_empty() {
        fallback.to_string()
    } else {
        candidate
    }
}

fn extract_filename(content_disposition: &str) -> Option<&str> {
    content_disposition
        .split(';')
        .find_map(|part| part.trim().strip_prefix("filename="))
        .map(|value| value.trim_matches('"'))
}

fn sanitize_filename(input: &str) -> String {
    let name = Path::new(input)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();

    name.chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => character,
            _ => '_',
        })
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::{DocumentQuery, sanitize_filename};

    #[test]
    fn query_params_cover_extended_filters() {
        let query = DocumentQuery {
            query: Some("invoice".to_string()),
            tag: Some("urgent".to_string()),
            tag_id: Some(7),
            correspondent: Some("Acme".to_string()),
            correspondent_id: Some(8),
            doc_type: Some("Invoice".to_string()),
            doc_type_id: Some(9),
            created_after: Some("2024-01-01".to_string()),
            created_before: Some("2024-12-31".to_string()),
            order_by: "title".to_string(),
            page_size: 50,
            page: 2,
        };
        let params = query.to_params();
        assert!(params.contains(&("query".to_string(), "invoice".to_string())));
        assert!(params.contains(&("tags__name__icontains".to_string(), "urgent".to_string())));
        assert!(params.contains(&("tags__id__in".to_string(), "7".to_string())));
        assert!(params.contains(&(
            "correspondent__name__icontains".to_string(),
            "Acme".to_string()
        )));
        assert!(params.contains(&("correspondent__id".to_string(), "8".to_string())));
        assert!(params.contains(&(
            "document_type__name__icontains".to_string(),
            "Invoice".to_string()
        )));
        assert!(params.contains(&("document_type__id".to_string(), "9".to_string())));
    }

    #[test]
    fn sanitize_filename_blocks_traversal() {
        assert_eq!(sanitize_filename("../../secret.txt"), "secret.txt");
        assert_eq!(sanitize_filename("folder/report 1.pdf"), "report_1.pdf");
    }
}
