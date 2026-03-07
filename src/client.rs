use std::collections::HashMap;
use std::time::Duration;

use bytes::Bytes;
use futures::stream::Stream;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{AiError, Result};

const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    default_headers: HashMap<String, String>,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .build()
                .expect("failed to build reqwest client"),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            default_headers: HashMap::new(),
        }
    }

    pub fn with_default_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.default_headers = headers;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn default_headers(&self) -> &HashMap<String, String> {
        &self.default_headers
    }

    fn build_headers(
        &self,
        extra: Option<&HashMap<String, String>>,
    ) -> Result<HeaderMap> {
        let mut map = HeaderMap::new();
        map.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", self.api_key))
            .map_err(|e| AiError::Config(format!("invalid api key header: {e}")))?);
        map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        for (k, v) in &self.default_headers {
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| AiError::Config(format!("invalid header name '{k}': {e}")))?;
            let val = HeaderValue::from_str(v)
                .map_err(|e| AiError::Config(format!("invalid header value for '{k}': {e}")))?;
            map.insert(name, val);
        }

        if let Some(extra) = extra {
            for (k, v) in extra {
                let name = HeaderName::from_bytes(k.as_bytes())
                    .map_err(|e| AiError::Config(format!("invalid header name '{k}': {e}")))?;
                let val = HeaderValue::from_str(v)
                    .map_err(|e| AiError::Config(format!("invalid header value for '{k}': {e}")))?;
                map.insert(name, val);
            }
        }

        Ok(map)
    }

    pub async fn post_json<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
        headers: Option<&HashMap<String, String>>,
    ) -> Result<R> {
        let url = format!("{}{}", self.base_url, path);
        let hdr = self.build_headers(headers)?;

        let resp = self.client.post(&url).headers(hdr).json(body).send().await?;
        let status = resp.status();

        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(AiError::Api {
                status: status.as_u16(),
                message,
            });
        }

        let result = resp.json::<R>().await?;
        Ok(result)
    }

    pub async fn post_stream(
        &self,
        path: &str,
        body: &impl Serialize,
        headers: Option<&HashMap<String, String>>,
    ) -> Result<impl Stream<Item = Result<Bytes>>> {
        let url = format!("{}{}", self.base_url, path);
        let hdr = self.build_headers(headers)?;

        let resp = self.client.post(&url).headers(hdr).json(body).send().await?;
        let status = resp.status();

        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(AiError::Api {
                status: status.as_u16(),
                message,
            });
        }

        Ok(resp.bytes_stream().map(|r| r.map_err(AiError::Http)))
    }
}

use futures::StreamExt as _;
