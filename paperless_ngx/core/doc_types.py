"""Document type management operations for Paperless-ngx CLI."""

from __future__ import annotations

from paperless_ngx.utils.paperless_backend import PaperlessBackend


def list_doc_types(backend: PaperlessBackend) -> list[dict]:
    """List all document types.

    Args:
        backend: Authenticated PaperlessBackend instance.

    Returns:
        List of document type dicts.
    """
    return backend.paginate("document_types/")


def create_doc_type(
    backend: PaperlessBackend,
    name: str,
    match: str = "",
    matching_algorithm: int = 0,
) -> dict:
    """Create a new document type.

    Args:
        backend: Authenticated PaperlessBackend instance.
        name: Document type name (must be unique).
        match: Match pattern string.
        matching_algorithm: 0=None, 1=Any, 2=All, 3=Literal, 4=Regex, 6=Auto.

    Returns:
        Created document type dict.
    """
    return backend.post("document_types/", data={
        "name": name,
        "match": match,
        "matching_algorithm": matching_algorithm,
    })


def delete_doc_type(backend: PaperlessBackend, doc_type_id: int) -> None:
    """Delete a document type by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_type_id: Document type primary key.
    """
    backend.delete(f"document_types/{doc_type_id}/")


def get_doc_type(backend: PaperlessBackend, doc_type_id: int) -> dict:
    """Get a single document type by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        doc_type_id: Document type primary key.

    Returns:
        Document type dict.
    """
    return backend.get(f"document_types/{doc_type_id}/")
