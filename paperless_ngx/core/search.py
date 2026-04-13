"""Global search operations for Paperless-ngx CLI."""

from __future__ import annotations

from typing import Any, cast

from paperless_ngx.utils.paperless_backend import PaperlessBackend


def query_search(
    backend: PaperlessBackend,
    query: str,
    page_size: int = 25,
    page: int = 1,
) -> dict[str, Any]:
    """Run a global Paperless search query across indexed resources."""
    return cast(
        dict[str, Any],
        backend.get(
            "search/",
            params={"query": query, "page_size": page_size, "page": page},
        ),
    )


def autocomplete_search(
    backend: PaperlessBackend,
    term: str,
    limit: int = 10,
) -> list[str]:
    """Fetch autocomplete suggestions for a partial Paperless search term."""
    return cast(
        list[str],
        backend.get("search/autocomplete/", params={"term": term, "limit": limit}),
    )
