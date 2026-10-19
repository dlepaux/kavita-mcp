//! Kavita as the calling person: their key on every request as `x-api-key`, never in a URL, where it
//! would land in a reverse proxy's access log.

use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::json;

#[derive(Debug)]
pub enum KavitaError {
    Unreachable,
    Http5xx,
    KeyRejected,
    /// Any other non-2xx, or a body that does not decode. The text is for the log only, never shown.
    Refused(String),
}

impl KavitaError {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Http5xx => "http_5xx",
            Self::KeyRejected => "key_rejected",
            Self::Refused(_) => "kavita_refused",
        }
    }
}

const MAX_LOGGED_BODY: usize = 300;

fn for_log(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_LOGGED_BODY)
        .collect()
}

/// Kavita's `MangaFormat`. Numbers are its JSON form; names are accepted too, in case a version
/// switches to string enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Image,
    Archive,
    Unknown,
    Epub,
    Pdf,
}

impl Format {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "images",
            Self::Archive => "CBZ",
            Self::Unknown => "unknown",
            Self::Epub => "EPUB",
            Self::Pdf => "PDF",
        }
    }

    #[must_use]
    pub fn kindle_ok(self) -> bool {
        matches!(self, Self::Epub | Self::Pdf)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NumOrName {
    Num(i64),
    Name(String),
}

impl<'de> Deserialize<'de> for Format {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match NumOrName::deserialize(d)? {
            NumOrName::Num(0) => Self::Image,
            NumOrName::Num(1) => Self::Archive,
            NumOrName::Num(3) => Self::Epub,
            NumOrName::Num(4) => Self::Pdf,
            NumOrName::Name(n) if n.eq_ignore_ascii_case("image") => Self::Image,
            NumOrName::Name(n) if n.eq_ignore_ascii_case("archive") => Self::Archive,
            NumOrName::Name(n) if n.eq_ignore_ascii_case("epub") => Self::Epub,
            NumOrName::Name(n) if n.eq_ignore_ascii_case("pdf") => Self::Pdf,
            _ => Self::Unknown,
        })
    }
}

/// `EmailDevicePlatform`: Custom 0, PocketBook 1, Kindle 2, Kobo 3.
fn platform<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    Ok(match NumOrName::deserialize(d)? {
        NumOrName::Num(n) => u8::try_from(n).unwrap_or(u8::MAX),
        NumOrName::Name(n) => match n.to_ascii_lowercase().as_str() {
            "custom" => 0,
            "pocketbook" => 1,
            "kindle" => 2,
            "kobo" => 3,
            _ => u8::MAX,
        },
    })
}

pub const KINDLE: u8 = 2;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesHit {
    pub series_id: u32,
    pub name: String,
    pub library_name: String,
}

#[derive(Deserialize)]
struct SearchGroup {
    #[serde(default)]
    series: Vec<SeriesHit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub format: Format,
    pub bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: u32,
    #[serde(default)]
    pub title_name: String,
    #[serde(default)]
    pub range: String,
    #[serde(default)]
    pub files: Vec<FileInfo>,
}

/// No email field: serde drops `emailAddress`, so it is never held, logged or returned.
#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    pub id: u32,
    pub name: String,
    #[serde(deserialize_with = "platform")]
    pub platform: u8,
}

pub struct Kavita {
    http: reqwest::Client,
    base: String,
}

impl Kavita {
    #[must_use]
    pub fn new(base: &str, timeout: Duration) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("static client config"),
            base: base.trim_end_matches('/').to_string(),
        }
    }

    async fn call(
        &self,
        key: &str,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<serde_json::Value>,
    ) -> Result<String, KavitaError> {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header("x-api-key", key)
            .query(query);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(|_| KavitaError::Unreachable)?;
        let status = resp.status();
        let text = resp.text().await.map_err(|_| KavitaError::Unreachable)?;
        match status {
            s if s.is_success() => Ok(text),
            StatusCode::UNAUTHORIZED => Err(KavitaError::KeyRejected),
            s if s.is_server_error() => Err(KavitaError::Http5xx),
            s => Err(KavitaError::Refused(format!("{s}: {}", for_log(&text)))),
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        key: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, KavitaError> {
        let text = self.call(key, Method::GET, path, query, None).await?;
        serde_json::from_str(&text)
            .map_err(|e| KavitaError::Refused(format!("{path}: undecodable body: {e}")))
    }

    /// # Errors
    /// See [`KavitaError`].
    pub async fn search(&self, key: &str, query: &str) -> Result<Vec<SeriesHit>, KavitaError> {
        let q = [
            ("queryString", query.to_string()),
            ("includeChapterAndFiles", "false".to_string()),
        ];
        Ok(self
            .get::<SearchGroup>(key, "/api/Search/search", &q)
            .await?
            .series)
    }

    /// # Errors
    /// See [`KavitaError`].
    pub async fn chapters(&self, key: &str, series_id: u32) -> Result<Vec<Chapter>, KavitaError> {
        self.get(
            key,
            "/api/Search/chapters-by-series",
            &[("seriesId", series_id.to_string())],
        )
        .await
    }

    /// # Errors
    /// See [`KavitaError`].
    pub async fn devices(&self, key: &str) -> Result<Vec<Device>, KavitaError> {
        self.get(key, "/api/Device", &[]).await
    }

    /// # Errors
    /// See [`KavitaError`].
    pub async fn email_setup(&self, key: &str) -> Result<bool, KavitaError> {
        self.get(
            key,
            "/api/Settings/is-email-setup",
            &[("forDevice", "true".to_string())],
        )
        .await
    }

    /// One chapter, never a series.
    ///
    /// # Errors
    /// See [`KavitaError`].
    pub async fn send_to(
        &self,
        key: &str,
        device_id: u32,
        chapter_id: u32,
    ) -> Result<(), KavitaError> {
        let body = json!({"deviceId": device_id, "chapterIds": [chapter_id]});
        self.call(key, Method::POST, "/api/Device/send-to", &[], Some(body))
            .await
            .map(drop)
    }
}
