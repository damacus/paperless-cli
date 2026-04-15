use std::fs;
use std::path::{Path, PathBuf};

use lopdf::{Document, Object};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PdfMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub modified_date: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PdfInfo {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub page_count: usize,
    pub metadata: PdfMetadata,
}

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("PDF file not found: {0}")]
    MissingFile(PathBuf),
    #[error("Failed to load PDF {path}: {source}")]
    Load {
        path: PathBuf,
        #[source]
        source: lopdf::Error,
    },
    #[error("Failed to extract text from PDF {path}: {source}")]
    ExtractText {
        path: PathBuf,
        #[source]
        source: lopdf::Error,
    },
    #[error("Failed to inspect PDF metadata for {path}: {reason}")]
    Metadata { path: PathBuf, reason: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn read_local_pdf_text(path: impl AsRef<Path>) -> Result<String, PdfError> {
    let path = path.as_ref();
    ensure_file_exists(path)?;

    let document = load_pdf(path)?;
    let pages = document.get_pages();
    let page_numbers = pages.keys().copied().collect::<Vec<_>>();
    if page_numbers.is_empty() {
        return Ok(String::new());
    }

    let text = document
        .extract_text(&page_numbers)
        .map_err(|source| PdfError::ExtractText {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(normalize_pdf_text(&text))
}

pub fn read_local_pdf_info(path: impl AsRef<Path>) -> Result<PdfInfo, PdfError> {
    let path = path.as_ref();
    ensure_file_exists(path)?;

    let document = load_pdf(path)?;
    let page_count = document.get_pages().len();
    let size_bytes = fs::metadata(path)?.len();
    let metadata = extract_metadata(&document, path)?;

    Ok(PdfInfo {
        path: path.to_path_buf(),
        file_name: path
            .file_name()
            .and_then(|part| part.to_str())
            .unwrap_or("document.pdf")
            .to_string(),
        size_bytes,
        page_count,
        metadata,
    })
}

fn ensure_file_exists(path: &Path) -> Result<(), PdfError> {
    if !path.is_file() {
        return Err(PdfError::MissingFile(path.to_path_buf()));
    }
    Ok(())
}

fn load_pdf(path: &Path) -> Result<Document, PdfError> {
    Document::load(path).map_err(|source| PdfError::Load {
        path: path.to_path_buf(),
        source,
    })
}

fn normalize_pdf_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

fn extract_metadata(document: &Document, path: &Path) -> Result<PdfMetadata, PdfError> {
    let info = match document.trailer.get(b"Info") {
        Ok(Object::Reference(reference)) => document
            .get_object(*reference)
            .map_err(|reason| PdfError::Metadata {
                path: path.to_path_buf(),
                reason: reason.to_string(),
            })?
            .as_dict()
            .map_err(|reason| PdfError::Metadata {
                path: path.to_path_buf(),
                reason: reason.to_string(),
            })?,
        Ok(Object::Dictionary(dictionary)) => dictionary,
        Ok(other) => {
            return Err(PdfError::Metadata {
                path: path.to_path_buf(),
                reason: format!("unsupported Info object: {other:?}"),
            });
        }
        Err(_) => {
            return Ok(PdfMetadata::default());
        }
    };

    Ok(PdfMetadata {
        title: read_info_string(info.get(b"Title").ok()),
        author: read_info_string(info.get(b"Author").ok()),
        subject: read_info_string(info.get(b"Subject").ok()),
        keywords: read_info_string(info.get(b"Keywords").ok()),
        creator: read_info_string(info.get(b"Creator").ok()),
        producer: read_info_string(info.get(b"Producer").ok()),
        creation_date: read_info_string(info.get(b"CreationDate").ok()),
        modified_date: read_info_string(info.get(b"ModDate").ok()),
    })
}

fn read_info_string(value: Option<&Object>) -> Option<String> {
    let value = value?;
    match value {
        Object::String(bytes, _) => decode_pdf_string(bytes),
        Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        Object::Integer(integer) => Some(integer.to_string()),
        Object::Real(real) => Some(real.to_string()),
        Object::Boolean(boolean) => Some(boolean.to_string()),
        _ => Some(format!("{value:?}")),
    }
}

fn decode_pdf_string(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        let mut units = Vec::with_capacity((bytes.len().saturating_sub(2)) / 2);
        for chunk in bytes[2..].chunks_exact(2) {
            units.push(u16::from_be_bytes([chunk[0], chunk[1]]));
        }
        return String::from_utf16(&units).ok();
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        let mut units = Vec::with_capacity((bytes.len().saturating_sub(2)) / 2);
        for chunk in bytes[2..].chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        return String::from_utf16(&units).ok();
    }

    Some(String::from_utf8_lossy(bytes).into_owned())
}
