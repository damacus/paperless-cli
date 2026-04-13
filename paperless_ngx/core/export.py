"""Export and bulk download operations for Paperless-ngx CLI."""

from __future__ import annotations

import os

from paperless_ngx.core.documents import download_document
from paperless_ngx.utils.paperless_backend import PaperlessBackend


def bulk_download(
    backend: PaperlessBackend,
    doc_ids: list[int],
    output_dir: str = ".",
    original: bool = False,
    progress_callback=None,
) -> list[dict]:
    """Download multiple documents to a local directory.

    Uses the Paperless bulk_download API endpoint for efficiency.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_ids: List of document IDs to download.
        output_dir: Local directory to write files.
        original: If True, download original files instead of archived.
        progress_callback: Optional callable(current, total, doc_id) for progress.

    Returns:
        List of dicts with 'doc_id', 'path', and 'status' for each document.
    """
    os.makedirs(output_dir, exist_ok=True)
    results = []
    total = len(doc_ids)

    for i, doc_id in enumerate(doc_ids):
        if progress_callback:
            progress_callback(i, total, doc_id)
        try:
            path = download_document(
                backend=backend,
                doc_id=doc_id,
                output_dir=output_dir,
                original=original,
            )
            results.append({"doc_id": doc_id, "path": path, "status": "ok"})
        except Exception as exc:
            results.append(
                {
                    "doc_id": doc_id,
                    "path": None,
                    "status": "error",
                    "error": str(exc),
                }
            )

    if progress_callback:
        progress_callback(total, total, None)

    return results


def bulk_download_zip(
    backend: PaperlessBackend,
    doc_ids: list[int],
    output_path: str,
    content: str = "both",
) -> str:
    """Download multiple documents as a single ZIP archive via the API.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_ids: List of document IDs to include.
        output_path: Local path to write the ZIP file.
        content: 'originals', 'archive', or 'both'.

    Returns:
        The output_path where the ZIP was saved.
    """
    payload = {
        "documents": doc_ids,
        "content": content,
    }
    # The bulk_download endpoint returns a ZIP file
    resp = backend._session.post(
        backend.config.api_url("documents/bulk_download/"),
        json=payload,
        stream=True,
    )
    from paperless_ngx.utils.paperless_backend import _check_response

    _check_response(resp)

    os.makedirs(os.path.dirname(os.path.abspath(output_path)), exist_ok=True)
    with open(output_path, "wb") as f:
        for chunk in resp.iter_content(chunk_size=8192):
            f.write(chunk)
    return output_path
