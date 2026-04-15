use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Response, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::Config;
use crate::models::Paginated;

#[derive(Clone, Debug)]
pub struct PaperlessClient {
    http: reqwest::Client,
    base_url: Url,
    config: Config,
}

impl PaperlessClient {
    pub fn new(config: Config) -> Result<Self> {
        let base_url = Url::parse(&format!("{}/", config.url))
            .with_context(|| format!("Invalid Paperless URL: {}", config.url))?;

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Token {}", config.token))
                .context("Unable to build authorization header")?,
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static("paperless-cli-rs/0.1.0"));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http,
            base_url,
            config,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub async fn get<T>(&self, path: &str, params: &[(String, String)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .get(self.api_url(path)?)
            .query(params)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn get_raw(&self, path: &str, params: &[(String, String)]) -> Result<Response> {
        let response = self
            .http
            .get(self.api_url(path)?)
            .query(params)
            .send()
            .await?;
        check_response(response).await
    }

    pub async fn post_json<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let response = self
            .http
            .post(self.api_url(path)?)
            .json(body)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn post_optional_json<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: DeserializeOwned + Default,
        B: Serialize + ?Sized,
    {
        let response = self
            .http
            .post(self.api_url(path)?)
            .json(body)
            .send()
            .await?;
        decode_json_allow_empty(response).await
    }

    pub async fn post_form_multipart<T>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T>
    where
        T: DeserializeOwned + Default,
    {
        let response = self
            .http
            .post(self.api_url(path)?)
            .multipart(form)
            .send()
            .await?;
        decode_json_allow_empty(response).await
    }

    pub async fn patch_json<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let response = self
            .http
            .patch(self.api_url(path)?)
            .json(body)
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        let response = self.http.delete(self.api_url(path)?).send().await?;
        let _ = check_response(response).await?;
        Ok(())
    }

    pub async fn paginate<T>(&self, path: &str, params: &[(String, String)]) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut results = Vec::new();
        let mut url = self.api_url(path)?;
        let mut first_page = true;

        loop {
            let request = if first_page {
                first_page = false;
                self.http.get(url.clone()).query(params)
            } else {
                self.http.get(url.clone())
            };

            let response = request.send().await?;
            let value = check_response(response).await?.json::<Value>().await?;
            if let Ok(items) = serde_json::from_value::<Vec<T>>(value.clone()) {
                results.extend(items);
                break;
            }

            let page = serde_json::from_value::<Paginated<T>>(value)?;
            results.extend(page.results);
            let Some(next) = page.next else {
                break;
            };
            let parsed = Url::parse(&next)?;
            if parsed.domain() != self.base_url.domain() || parsed.scheme() != self.base_url.scheme()
            {
                bail!("Refusing to follow cross-origin pagination link: {}", parsed);
            }
            url = parsed;
        }

        Ok(results)
    }

    pub async fn ping(&self) -> Result<Value> {
        self.get("status/", &[]).await
    }

    pub async fn exchange_token(base_url: &str, username: &str, password: &str) -> Result<String> {
        let url = Url::parse(&format!("{}/api/token/", base_url.trim_end_matches('/')))
            .with_context(|| format!("Invalid Paperless URL: {}", base_url))?;
        let response = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?
            .post(url)
            .form(&[("username", username), ("password", password)])
            .send()
            .await?;

        let value = check_response(response).await?.json::<Value>().await?;
        value
            .get("token")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("Token exchange response did not contain a token"))
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(&format!("api/{}", path.trim_start_matches('/')))
            .with_context(|| format!("Invalid API path: {}", path))
    }
}

async fn decode_json<T>(response: Response) -> Result<T>
where
    T: DeserializeOwned,
{
    Ok(check_response(response).await?.json::<T>().await?)
}

async fn decode_json_allow_empty<T>(response: Response) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    let checked = check_response(response).await?;
    let bytes = checked.bytes().await?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    Ok(serde_json::from_slice::<T>(&bytes)?)
}

async fn check_response(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let url = response.url().to_string();
    let text = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&text)
        .ok()
        .map(|value| value.to_string())
        .unwrap_or(text);

    match status.as_u16() {
        401 => bail!("Authentication failed. Check your API token and run `paperless login`."),
        403 => bail!("Permission denied for this operation."),
        404 => bail!("Resource not found: {}", url),
        code => bail!("API error {}: {}", code, detail),
    }
}
