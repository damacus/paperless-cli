"""Tag management operations for Paperless-ngx CLI."""

from __future__ import annotations

from paperless_ngx.utils.paperless_backend import PaperlessBackend


def list_tags(backend: PaperlessBackend) -> list[dict]:
    """List all tags.

    Args:
        backend: Authenticated PaperlessBackend instance.

    Returns:
        List of tag dicts.
    """
    return backend.paginate("tags/")


def create_tag(
    backend: PaperlessBackend,
    name: str,
    color: str = "#a6cee3",
    is_inbox_tag: bool = False,
) -> dict:
    """Create a new tag.

    Args:
        backend: Authenticated PaperlessBackend instance.
        name: Tag name (must be unique).
        color: Hex color string (e.g. "#a6cee3").
        is_inbox_tag: Whether this is an inbox tag.

    Returns:
        Created tag dict.
    """
    return backend.post("tags/", data={
        "name": name,
        "color": color,
        "is_inbox_tag": is_inbox_tag,
    })


def delete_tag(backend: PaperlessBackend, tag_id: int) -> None:
    """Delete a tag by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        tag_id: Tag primary key.
    """
    backend.delete(f"tags/{tag_id}/")


def get_tag(backend: PaperlessBackend, tag_id: int) -> dict:
    """Get a single tag by ID.

    Args:
        backend: Authenticated PaperlessBackend instance.
        tag_id: Tag primary key.

    Returns:
        Tag dict.
    """
    return backend.get(f"tags/{tag_id}/")
