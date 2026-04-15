pub mod api;
pub mod config;
pub mod demo;
pub mod error;
pub mod pdf;
pub mod render;
pub mod security;
pub mod services;
pub mod tui;

pub use config::{AppConfig, AppPaths, OutputMode, SessionState};
pub use error::AppError;
pub use pdf::{read_local_pdf_info, read_local_pdf_text, PdfError, PdfInfo, PdfMetadata};
