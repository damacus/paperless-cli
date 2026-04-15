use std::path::PathBuf;

use paperless_cli::pdf::{read_local_pdf_info, read_local_pdf_text, PdfError};
use tempfile::tempdir;

#[test]
fn read_local_pdf_text_returns_page_text() {
    let pdf_path = write_fixture_pdf();

    let text = read_local_pdf_text(&pdf_path).unwrap();
    assert_eq!(text, "Hello from PDF");
}

#[test]
fn read_local_pdf_info_returns_metadata_and_file_details() {
    let pdf_path = write_fixture_pdf();

    let info = read_local_pdf_info(&pdf_path).unwrap();
    assert_eq!(info.file_name, "sample.pdf");
    assert_eq!(info.page_count, 1);
    assert!(info.size_bytes > 0);
    assert_eq!(info.metadata.title.as_deref(), Some("Invoice 2026"));
    assert_eq!(info.metadata.author.as_deref(), Some("Paperless"));
    assert_eq!(info.metadata.subject.as_deref(), Some("Metadata test"));
    assert_eq!(info.metadata.keywords.as_deref(), Some("invoice, pdf"));
    assert_eq!(info.metadata.creator.as_deref(), Some("Codex"));
    assert_eq!(info.metadata.producer.as_deref(), Some("Unit Test"));
    assert_eq!(
        info.metadata.creation_date.as_deref(),
        Some("D:20260415093000Z")
    );
    assert_eq!(
        info.metadata.modified_date.as_deref(),
        Some("D:20260415094500Z")
    );
}

#[test]
fn missing_pdf_returns_useful_error() {
    let error = read_local_pdf_text(PathBuf::from("missing.pdf")).unwrap_err();
    match error {
        PdfError::MissingFile(path) => assert!(path.ends_with("missing.pdf")),
        other => panic!("unexpected error: {other:?}"),
    }
}

fn write_fixture_pdf() -> PathBuf {
    let tempdir = tempdir().unwrap();
    let pdf_path = tempdir.path().join("sample.pdf");
    let bytes = build_fixture_pdf();
    std::fs::write(&pdf_path, bytes).unwrap();
    tempdir.keep().join("sample.pdf")
}

fn build_fixture_pdf() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();

    push_line(&mut bytes, "%PDF-1.4");
    push_line(&mut bytes, "%\u{00E2}\u{00E3}\u{00CF}\u{00D3}");

    push_object(
        &mut bytes,
        &mut offsets,
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    );
    push_object(
        &mut bytes,
        &mut offsets,
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
    );
    push_object(
        &mut bytes,
        &mut offsets,
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n",
    );
    push_object(
        &mut bytes,
        &mut offsets,
        "4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );

    let stream = b"BT\n/F1 24 Tf\n72 720 Td\n(Hello from PDF) Tj\nET\n";
    let content_object = format!(
        "5 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
        stream.len(),
        String::from_utf8_lossy(stream)
    );
    push_object(&mut bytes, &mut offsets, &content_object);

    push_object(
        &mut bytes,
        &mut offsets,
        "6 0 obj\n<< /Title (Invoice 2026) /Author (Paperless) /Subject (Metadata test) /Creator (Codex) /Producer (Unit Test) /Keywords (invoice, pdf) /CreationDate (D:20260415093000Z) /ModDate (D:20260415094500Z) >>\nendobj\n",
    );

    let xref_offset = bytes.len();
    push_line(&mut bytes, "xref");
    push_line(&mut bytes, "0 7");
    push_line(&mut bytes, "0000000000 65535 f ");
    for offset in offsets {
        push_line(&mut bytes, &format!("{offset:010} 00000 n "));
    }
    push_line(&mut bytes, "trailer << /Root 1 0 R /Info 6 0 R /Size 7 >>");
    push_line(&mut bytes, &format!("startxref\n{xref_offset}"));
    push_line(&mut bytes, "%%EOF");

    bytes
}

fn push_object(bytes: &mut Vec<u8>, offsets: &mut Vec<usize>, object: &str) {
    offsets.push(bytes.len());
    bytes.extend_from_slice(object.as_bytes());
}

fn push_line(bytes: &mut Vec<u8>, line: &str) {
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
}
