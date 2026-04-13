"""Task inspection operations for Paperless-ngx CLI."""

from __future__ import annotations

from typing import Any, cast

from paperless_ngx.utils.paperless_backend import PaperlessBackend


def list_tasks(backend: PaperlessBackend) -> list[dict[str, Any]]:
    """List Paperless background tasks."""
    return cast(list[dict[str, Any]], backend.paginate("tasks/"))


def get_task(backend: PaperlessBackend, task_id: int) -> dict[str, Any]:
    """Get a single Paperless background task by ID."""
    return cast(dict[str, Any], backend.get(f"tasks/{task_id}/"))
