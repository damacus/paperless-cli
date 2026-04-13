"""Correspondent management operations for Paperless-ngx CLI."""

from __future__ import annotations

from typing import Any, cast

from paperless_ngx.utils.paperless_backend import PaperlessBackend


def list_correspondents(backend: PaperlessBackend) -> list[dict]:
    """List all correspondents.

    Args:
        backend: Authenticated PaperlessBackend instance.

    Returns:
        List of correspondent dicts.
    """
    return backend.paginate("correspondents/")


def create_correspondent(
    backend: PaperlessBackend,
    name: str,
    match: str = "",
    matching_algorithm: int = 0,
) -> dict[str, Any]:
    """Create a new correspondent.

    Args:
        backend: Authenticated PaperlessBackend instance.
        name: Correspondent name (must be unique).
        match: Match pattern string.
        matching_algorithm: 0=None, 1=Any, 2=All, 3=Literal, 4=Regex, 6=Auto.

    Returns:
        Created correspondent dict.
    """
    return cast(
        dict[str, Any],
        backend.post(
            "correspondents/",
            data={
                "name": name,
                "match": match,
                "matching_algorithm": matching_algorithm,
            },
        ),
    )


def delete_correspondent(backend: PaperlessBackend, correspondent_id: int) -> None:
    """Delete a correspondent by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        correspondent_id: Correspondent primary key.
    """
    backend.delete(f"correspondents/{correspondent_id}/")


def get_correspondent(
    backend: PaperlessBackend, correspondent_id: int
) -> dict[str, Any]:
    """Get a single correspondent by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        correspondent_id: Correspondent primary key.

    Returns:
        Correspondent dict.
    """
    return cast(dict[str, Any], backend.get(f"correspondents/{correspondent_id}/"))
