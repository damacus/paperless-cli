"""Document CRUD operations for Paperless-ngx CLI."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any, cast

from paperless_ngx.utils.paperless_backend import PaperlessBackend


def list_documents(
    backend: PaperlessBackend,
    query: str | None = None,
    tag: str | None = None,
    correspondent: str | None = None,
    doc_type: str | None = None,
    page_size: int = 25,
    page: int = 1,
) -> dict[str, Any]:
    """List documents with optional filters.

    Args:
        backend: Authenticated PaperlessBackend instance.
        query: Full-text search query string.
        tag: Filter by tag name (partial match supported via API).
        correspondent: Filter by correspondent name.
        doc_type: Filter by document type name.
        page_size: Number of results per page.
        page: Page number (1-based).

    Returns:
        Dict with 'count', 'next', 'previous', 'results' keys.
    """
    params: dict[str, Any] = {
        "page_size": page_size,
        "page": page,
        "ordering": "-created",
    }
    if query:
        params["query"] = query
    if tag:
        params["tags__name__icontains"] = tag
    if correspondent:
        params["correspondent__name__icontains"] = correspondent
    if doc_type:
        params["document_type__name__icontains"] = doc_type

    return cast(dict[str, Any], backend.get("documents/", params=params))


def get_document(backend: PaperlessBackend, doc_id: int) -> dict[str, Any]:
    """Get a single document by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_id: Document primary key.

    Returns:
        Document metadata dict.
    """
    return cast(dict[str, Any], backend.get(f"documents/{doc_id}/"))


def upload_document(
    backend: PaperlessBackend,
    file_path: str,
    title: str | None = None,
    correspondent_id: int | None = None,
    document_type_id: int | None = None,
    tag_ids: list[int] | None = None,
) -> dict[str, Any]:
    """Upload a document file to Paperless-ngx.

    Args:
        backend: Authenticated PaperlessBackend instance.
        file_path: Local path to the file to upload.
        title: Optional title override.
        correspondent_id: Optional correspondent ID to assign.
        document_type_id: Optional document type ID to assign.
        tag_ids: Optional list of tag IDs to assign.

    Returns:
        API response (task ID or document info).
    """
    path = Path(file_path)
    if not path.exists():
        raise FileNotFoundError(f"File not found: {file_path}")

    data: dict[str, Any] = {}
    if title:
        data["title"] = title
    if correspondent_id is not None:
        data["correspondent"] = correspondent_id
    if document_type_id is not None:
        data["document_type"] = document_type_id
    if tag_ids:
        for tag_id in tag_ids:
            data.setdefault("tags", []).append(tag_id)

    with open(file_path, "rb") as f:
        files = {"document": (path.name, f, _guess_mime(path))}
        return cast(
            dict[str, Any],
            backend.post("documents/post_document/", data=data, files=files),
        )


def download_document(
    backend: PaperlessBackend,
    doc_id: int,
    output_dir: str = ".",
    original: bool = False,
) -> str:
    """Download a document file to a local directory.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_id: Document primary key.
        output_dir: Local directory to save the file.
        original: If True, download the original file; otherwise download the
                  archived (processed) version.

    Returns:
        The local path where the file was saved.
    """
    endpoint = f"documents/{doc_id}/download/"
    params = {}
    if original:
        params["original"] = "true"

    resp = backend.get_raw(endpoint, params=params or None)

    # Extract filename from Content-Disposition or fallback
    filename = f"document_{doc_id}"
    cd = resp.headers.get("Content-Disposition", "")
    if "filename=" in cd:
        filename = cd.split("filename=")[-1].strip().strip('"')

    output_path = os.path.join(output_dir, filename)
    os.makedirs(output_dir, exist_ok=True)
    with open(output_path, "wb") as f:
        for chunk in resp.iter_content(chunk_size=8192):
            f.write(chunk)
    return output_path


def update_document(
    backend: PaperlessBackend,
    doc_id: int,
    title: str | None = None,
    correspondent_id: int | None = None,
    document_type_id: int | None = None,
    tag_ids: list[int] | None = None,
    created: str | None = None,
    custom_fields: list[dict] | None = None,
) -> dict[str, Any]:
    """Update document metadata.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_id: Document primary key.
        title: New title.
        correspondent_id: New correspondent ID (None = clear).
        document_type_id: New document type ID (None = clear).
        tag_ids: New tag IDs (replaces existing tags).
        created: New created date string (YYYY-MM-DD).
        custom_fields: List of custom field dicts.

    Returns:
        Updated document dict.
    """
    patch: dict[str, Any] = {}
    if title is not None:
        patch["title"] = title
    if correspondent_id is not None:
        patch["correspondent"] = correspondent_id
    if document_type_id is not None:
        patch["document_type"] = document_type_id
    if tag_ids is not None:
        patch["tags"] = tag_ids
    if created is not None:
        patch["created"] = created
    if custom_fields is not None:
        patch["custom_fields"] = custom_fields

    if not patch:
        raise ValueError("No fields to update provided.")

    return cast(dict[str, Any], backend.patch(f"documents/{doc_id}/", data=patch))


def delete_document(backend: PaperlessBackend, doc_id: int) -> None:
    """Delete a document by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_id: Document primary key.
    """
    backend.delete(f"documents/{doc_id}/")


def search_documents(
    backend: PaperlessBackend,
    query: str,
    page_size: int = 25,
) -> dict[str, Any]:
    """Full-text search documents.

    Args:
        backend: Authenticated PaperlessBackend instance.
        query: Search query string.
        page_size: Maximum results to return.

    Returns:
        Dict with 'count' and 'results'.
    """
    return cast(
        dict[str, Any],
        backend.get("documents/", params={"query": query, "page_size": page_size}),
    )


def _guess_mime(path: Path) -> str:
    """Guess MIME type from file extension."""
    ext = path.suffix.lower()
    mime_map = {
        ".pdf": "application/pdf",
        ".png": "image/png",
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
        ".tiff": "image/tiff",
        ".tif": "image/tiff",
        ".gif": "image/gif",
        ".webp": "image/webp",
        ".txt": "text/plain",
        ".csv": "text/csv",
        ".odt": "application/vnd.oasis.opendocument.text",
        ".docx": (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ),
        ".xlsx": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    }
    return mime_map.get(ext, "application/octet-stream")
