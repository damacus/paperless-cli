"""HTTP client for the Paperless-ngx REST API.

Handles authentication, pagination, and error reporting for all API calls.
Config is loaded from ~/.config/paperless-cli/config.json.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any, cast

import requests
from requests import Response, Session

CONFIG_PATH = os.path.expanduser("~/.config/paperless-cli/config.json")
SESSION_PATH = "/tmp/paperless-cli-session.json"
SessionState = dict[str, Any]


class PaperlessConfig:
    """Holds connection configuration for Paperless-ngx."""

    def __init__(self, url: str, token: str):
        self.url = url.rstrip("/")
        self.token = token

    def api_url(self, path: str) -> str:
        """Build a full API URL from a relative path."""
        path = path.lstrip("/")
        return f"{self.url}/api/{path}"

    def to_dict(self) -> dict:
        return {"url": self.url, "token": self.token}

    @classmethod
    def from_dict(cls, data: dict[str, str]) -> PaperlessConfig:
        return cls(url=data["url"], token=data["token"])


def find_paperless_server() -> PaperlessConfig:
    """Load Paperless-ngx connection config from disk.

    Raises:
        RuntimeError: If config file is missing or malformed.
    """
    config_path = Path(CONFIG_PATH)
    if not config_path.exists():
        raise RuntimeError(
            "Paperless-ngx not configured. Run:\n"
            "  paperless project init\n"
            "to configure the connection."
        )
    try:
        data = json.loads(config_path.read_text())
        if "url" not in data or "token" not in data:
            raise KeyError("Missing required fields")
        return PaperlessConfig.from_dict(data)
    except (json.JSONDecodeError, KeyError) as exc:
        raise RuntimeError(
            f"Config file at {CONFIG_PATH} is malformed: {exc}\n"
            "Run: paperless project init"
        ) from exc


def save_config(url: str, token: str) -> None:
    """Save Paperless-ngx connection config to disk."""
    config_path = Path(CONFIG_PATH)
    config_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.write_text(json.dumps({"url": url, "token": token}, indent=2))


def _make_session(config: PaperlessConfig) -> Session:
    """Create an authenticated requests Session."""
    session = requests.Session()
    session.headers.update(
        {
            "Authorization": f"Token {config.token}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        }
    )
    return session


def _check_response(response: Response) -> Response:
    """Raise a descriptive RuntimeError for HTTP errors."""
    if response.status_code == 401:
        raise RuntimeError(
            "Authentication failed. Check your API token.\nRun: paperless project init"
        )
    if response.status_code == 403:
        raise RuntimeError("Permission denied for this operation.")
    if response.status_code == 404:
        raise RuntimeError(f"Resource not found: {response.url}")
    if response.status_code >= 400:
        try:
            detail = response.json()
        except Exception:
            detail = response.text
        raise RuntimeError(f"API error {response.status_code}: {detail}")
    return response


class PaperlessBackend:
    """HTTP client wrapping the Paperless-ngx REST API."""

    def __init__(self, config: PaperlessConfig | None = None):
        if config is None:
            config = find_paperless_server()
        self.config = config
        self._session = _make_session(config)

    def get(self, path: str, params: dict | None = None) -> Any:
        """GET a single resource. Returns parsed JSON."""
        url = self.config.api_url(path)
        resp = self._session.get(url, params=params)
        _check_response(resp)
        return resp.json()

    def get_raw(self, path: str, params: dict | None = None) -> Response:
        """GET and return the raw Response (for binary downloads)."""
        url = self.config.api_url(path)
        # Remove Content-Type header for binary requests
        headers = {
            k: v for k, v in self._session.headers.items() if k != "Content-Type"
        }
        resp = self._session.get(url, params=params, headers=headers, stream=True)
        _check_response(resp)
        return resp

    def post(
        self, path: str, data: dict | None = None, files: dict | None = None
    ) -> Any:
        """POST to a resource. Returns parsed JSON."""
        url = self.config.api_url(path)
        if files:
            # Multipart form upload — strip Content-Type to let requests set boundary
            headers = {
                k: v for k, v in self._session.headers.items() if k != "Content-Type"
            }
            resp = self._session.post(url, data=data, files=files, headers=headers)
        else:
            resp = self._session.post(url, json=data)
        _check_response(resp)
        if resp.status_code == 204 or not resp.content:
            return {}
        return resp.json()

    def patch(self, path: str, data: dict) -> Any:
        """PATCH a resource. Returns parsed JSON."""
        url = self.config.api_url(path)
        resp = self._session.patch(url, json=data)
        _check_response(resp)
        return resp.json()

    def put(self, path: str, data: dict) -> Any:
        """PUT (replace) a resource. Returns parsed JSON."""
        url = self.config.api_url(path)
        resp = self._session.put(url, json=data)
        _check_response(resp)
        return resp.json()

    def delete(self, path: str) -> None:
        """DELETE a resource."""
        url = self.config.api_url(path)
        resp = self._session.delete(url)
        _check_response(resp)

    def paginate(self, path: str, params: dict | None = None) -> list[Any]:
        """Fetch all pages from a DRF paginated endpoint.

        Paperless uses page-based pagination with 'next' cursor links.
        Returns the flat list of all results.
        """
        params = dict(params or {})
        params.setdefault("page_size", 100)
        results: list[Any] = []

        url = self.config.api_url(path)
        while url:
            resp = self._session.get(url, params=params)
            _check_response(resp)
            data = resp.json()
            # Support both DRF list response and plain list
            if isinstance(data, list):
                results.extend(data)
                break
            results.extend(data.get("results", []))
            url = data.get("next")
            # After first page, params are embedded in 'next' URL
            params = {}
        return results

    def ping(self) -> dict:
        """Check server connectivity. Returns status info."""
        try:
            resp = self._session.get(
                self.config.api_url("status/"),
                timeout=10,
            )
            _check_response(resp)
            return {
                "status": "ok",
                "url": self.config.url,
                "response_code": resp.status_code,
            }
        except requests.ConnectionError as exc:
            raise RuntimeError(
                f"Cannot connect to Paperless-ngx at {self.config.url}: {exc}"
            ) from exc
        except requests.Timeout as exc:
            raise RuntimeError(f"Connection to {self.config.url} timed out.") from exc

    def get_token(self, username: str, password: str) -> str:
        """Obtain an API token using username/password credentials.

        Makes a POST to /api/token/ and returns the token string.
        """
        url = self.config.url + "/api/token/"
        resp = requests.post(
            url,
            data={"username": username, "password": password},
            timeout=10,
        )
        if resp.status_code == 200:
            body = cast(dict[str, str], resp.json())
            return body["token"]
        raise RuntimeError(
            f"Failed to obtain token (HTTP {resp.status_code}). "
            "Check your username and password."
        )


def load_session() -> SessionState:
    """Load ephemeral session state from /tmp."""
    path = Path(SESSION_PATH)
    if path.exists():
        try:
            return cast(SessionState, json.loads(path.read_text()))
        except Exception:
            pass
    return {
        "last_query": "",
        "selected_docs": [],
        "history": [],
    }


def save_session(state: SessionState) -> None:
    """Persist ephemeral session state to /tmp."""
    Path(SESSION_PATH).write_text(json.dumps(state, indent=2))
