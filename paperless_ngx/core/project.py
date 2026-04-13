"""Connection and project configuration management for Paperless-ngx CLI."""

from __future__ import annotations

from paperless_ngx.utils.paperless_backend import (
    CONFIG_PATH,
    PaperlessBackend,
    PaperlessConfig,
    find_paperless_server,
    save_config,
)


def init_connection(
    url: str,
    token: str | None = None,
    username: str | None = None,
    password: str | None = None,
) -> PaperlessConfig:
    """Initialize the connection to a Paperless-ngx server.

    Accepts either a token or username+password (to obtain a token).
    Saves config to ~/.config/paperless-cli/config.json.

    Args:
        url: Base URL of the Paperless-ngx server (e.g. http://localhost:8000)
        token: API authentication token (preferred)
        username: Username for token acquisition (alternative to token)
        password: Password for token acquisition

    Returns:
        The validated PaperlessConfig.

    Raises:
        ValueError: If neither token nor username/password provided.
        RuntimeError: If connection test fails.
    """
    url = url.rstrip("/")

    if not token:
        if not username or not password:
            raise ValueError(
                "Provide either --token or both --username and --password."
            )
        # Use a temporary config to acquire the token
        tmp_config = PaperlessConfig(url=url, token="placeholder")
        backend = PaperlessBackend(config=tmp_config)
        token = backend.get_token(username, password)

    config = PaperlessConfig(url=url, token=token)
    # Verify the connection works before saving
    backend = PaperlessBackend(config=config)
    backend.ping()
    save_config(url, token)
    return config


def get_connection_info() -> dict:
    """Get current connection info and server statistics.

    Returns:
        Dict with url, token (masked), and server statistics.
    """
    config = find_paperless_server()
    backend = PaperlessBackend(config=config)

    # Fetch statistics from the API
    try:
        stats = backend.get("statistics/")
    except Exception as exc:
        stats = {"error": str(exc)}

    return {
        "url": config.url,
        "token": config.token[:8] + "..." if len(config.token) > 8 else "***",
        "config_path": CONFIG_PATH,
        "statistics": stats,
    }


def ping_server() -> dict:
    """Test connectivity to the configured Paperless-ngx server.

    Returns:
        Dict with status and response time.
    """
    import time

    config = find_paperless_server()
    backend = PaperlessBackend(config=config)
    start = time.monotonic()
    result = backend.ping()
    elapsed = time.monotonic() - start
    result["elapsed_ms"] = round(elapsed * 1000, 1)
    return result
