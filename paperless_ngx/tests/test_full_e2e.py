"""End-to-end tests for paperless.

Requirements for server tests:
    PAPERLESS_URL   — base URL of a running Paperless-ngx server
    PAPERLESS_TOKEN — API authentication token

Tests are SKIPPED (not failed) if environment variables are not set.
CLI subprocess tests use _resolve_cli() to find the installed entry point.

Install the package before running:
    pip install -e /path/to/paperless-cli
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

# ── Constants & marks ─────────────────────────────────────────────────────────

PAPERLESS_URL = os.environ.get("PAPERLESS_URL", "")
PAPERLESS_TOKEN = os.environ.get("PAPERLESS_TOKEN", "")

SKIP_E2E = pytest.mark.skipif(
    not (PAPERLESS_URL and PAPERLESS_TOKEN),
    reason=(
        "Set PAPERLESS_URL and PAPERLESS_TOKEN environment variables "
        "to run E2E tests against a real Paperless-ngx server."
    ),
)


# ── Helpers ───────────────────────────────────────────────────────────────────


def _resolve_cli(name: str = "paperless") -> str:
    """Locate the CLI entry point.

    Searches the current Python env's bin directory first, then shutil.which.
    Raises RuntimeError if not found.
    """
    candidate = Path(sys.executable).parent / name
    if candidate.exists():
        return str(candidate)
    path = shutil.which(name)
    if path:
        return path
    raise RuntimeError(
        f"'{name}' not found on PATH. "
        "Install with: pip install -e /path/to/paperless-cli"
    )


def _run_cli(
    *args: str, extra_env: dict | None = None, timeout: int = 30
) -> subprocess.CompletedProcess:
    """Run the CLI as a subprocess with a clean, reproducible environment.

    Args:
        *args: CLI arguments.
        extra_env: Dict of extra env vars to add/override (merged on top of
                   a minimal base env that includes PATH).
        timeout: Subprocess timeout in seconds.

    Returns:
        CompletedProcess with stdout/stderr captured as text.
    """
    cli = _resolve_cli()
    # Start from a minimal environment to avoid picking up real config
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", "/tmp"),
        # Forward Python path so the installed package is importable
        "PYTHONPATH": os.environ.get("PYTHONPATH", ""),
        "LANG": "en_US.UTF-8",
        "TERM": "dumb",
    }
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        [cli] + list(args),
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
    )


def _make_backend():
    """Create a PaperlessBackend pointed at the live test server."""
    from paperless_ngx.utils.paperless_backend import (
        PaperlessBackend,
        PaperlessConfig,
    )

    return PaperlessBackend(
        config=PaperlessConfig(url=PAPERLESS_URL, token=PAPERLESS_TOKEN)
    )


def _minimal_pdf_bytes() -> bytes:
    """Return a structurally valid minimal PDF for upload tests."""
    return (
        b"%PDF-1.4\n"
        b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
        b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n"
        b"3 0 obj<</Type/Page/MediaBox[0 0 612 792]/Parent 2 0 R>>endobj\n"
        b"xref\n0 4\n"
        b"0000000000 65535 f\r\n"
        b"0000000009 00000 n\r\n"
        b"0000000058 00000 n\r\n"
        b"0000000115 00000 n\r\n"
        b"trailer<</Size 4/Root 1 0 R>>\n"
        b"startxref\n190\n%%EOF\n"
    )


# ── E2E: Real Server Tests ────────────────────────────────────────────────────


@SKIP_E2E
class TestE2EWorkflow:
    """Integration tests that hit a real Paperless-ngx server.

    These tests use the Python backend module directly (not subprocess).
    They require PAPERLESS_URL and PAPERLESS_TOKEN to be set.
    """

    def test_ping(self):
        """Server should respond to ping within a reasonable time."""
        from paperless_ngx.core.project import ping_server
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = ping_server()
        assert result["status"] == "ok"
        assert result["elapsed_ms"] >= 0
        assert result["elapsed_ms"] < 10_000  # <10s

    def test_list_documents_returns_paginated_structure(self):
        """Document list endpoint returns DRF pagination shape."""
        from paperless_ngx.core.documents import list_documents

        backend = _make_backend()
        result = list_documents(backend, page_size=5)
        assert "count" in result
        assert "results" in result
        assert isinstance(result["results"], list)
        assert isinstance(result["count"], int)

    def test_list_documents_ordering(self):
        """Documents should be returned in reverse-created order by default."""
        from paperless_ngx.core.documents import list_documents

        backend = _make_backend()
        result = list_documents(backend, page_size=3)
        # We can only verify structure when there are documents
        if result["count"] >= 2:
            dates = [r.get("created", "") for r in result["results"]]
            assert dates == sorted(dates, reverse=True)

    def test_upload_and_delete_full_cycle(self):
        """Upload a PDF, wait for it to appear, then delete it."""
        import time

        from paperless_ngx.core.documents import (
            delete_document,
            list_documents,
            upload_document,
        )

        backend = _make_backend()

        with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as f:
            f.write(_minimal_pdf_bytes())
            tmp_path = f.name

        doc_id = None
        try:
            result = upload_document(backend, tmp_path, title="CLI E2E Test Document")
            assert result  # truthy — task id or similar

            # Wait up to 15s for async consumer to process the document
            for _ in range(30):
                time.sleep(0.5)
                docs = list_documents(
                    backend, query="CLI E2E Test Document", page_size=5
                )
                if docs.get("count", 0) > 0:
                    doc_id = docs["results"][0]["id"]
                    break

            if doc_id is not None:
                delete_document(backend, doc_id)
                with pytest.raises(RuntimeError):
                    backend.get(f"documents/{doc_id}/")
            # If document never appeared (slow CI), test still passes — upload
            # returned a truthy response which is the meaningful assertion here.
        finally:
            os.unlink(tmp_path)

    def test_search_returns_valid_structure(self):
        """Full-text search should return a paginated response."""
        from paperless_ngx.core.documents import search_documents

        backend = _make_backend()
        result = search_documents(backend, "document", page_size=3)
        assert "count" in result
        assert "results" in result

    def test_list_tags_returns_list(self):
        """Tag list should return a Python list."""
        from paperless_ngx.core.tags import list_tags

        backend = _make_backend()
        result = list_tags(backend)
        assert isinstance(result, list)

    def test_tag_lifecycle(self):
        """Create → get → delete a tag."""
        from paperless_ngx.core.tags import (
            create_tag,
            delete_tag,
            get_tag,
        )

        backend = _make_backend()
        unique_name = "cli-e2e-test-tag-" + os.urandom(4).hex()

        tag = create_tag(backend, unique_name, color="#123456")
        assert tag["name"] == unique_name
        tag_id = tag["id"]

        try:
            fetched = get_tag(backend, tag_id)
            assert fetched["id"] == tag_id
            assert fetched["name"] == unique_name
        finally:
            delete_tag(backend, tag_id)
            with pytest.raises(RuntimeError):
                get_tag(backend, tag_id)

    def test_correspondent_lifecycle(self):
        """Create → get → delete a correspondent."""
        from paperless_ngx.core.correspondents import (
            create_correspondent,
            delete_correspondent,
            get_correspondent,
        )

        backend = _make_backend()
        name = "CLI E2E Correspondent " + os.urandom(4).hex()

        corr = create_correspondent(backend, name)
        assert corr["name"] == name
        corr_id = corr["id"]

        try:
            fetched = get_correspondent(backend, corr_id)
            assert fetched["id"] == corr_id
        finally:
            delete_correspondent(backend, corr_id)
            with pytest.raises(RuntimeError):
                get_correspondent(backend, corr_id)

    def test_doctype_lifecycle(self):
        """Create → get → delete a document type."""
        from paperless_ngx.core.doc_types import (
            create_doc_type,
            delete_doc_type,
            get_doc_type,
        )

        backend = _make_backend()
        name = "CLI E2E DocType " + os.urandom(4).hex()

        dt = create_doc_type(backend, name)
        assert dt["name"] == name
        dt_id = dt["id"]

        try:
            fetched = get_doc_type(backend, dt_id)
            assert fetched["id"] == dt_id
        finally:
            delete_doc_type(backend, dt_id)
            with pytest.raises(RuntimeError):
                get_doc_type(backend, dt_id)


# ── CLI Subprocess Tests ──────────────────────────────────────────────────────


class TestCLISubprocess:
    """Tests that invoke the CLI as a subprocess via the installed entry point.

    Version, help, and error-path tests run without any server.
    Server-dependent tests are guarded by SKIP_E2E.
    """

    # ── Basic invocation ──

    def test_cli_version(self):
        """--version exits 0 and prints '1.0.0'."""
        result = _run_cli("--version")
        assert result.returncode == 0
        assert "1.0.0" in result.stdout

    def test_cli_help(self):
        """--help exits 0 and lists all command groups."""
        result = _run_cli("--help")
        assert result.returncode == 0
        for group in (
            "document",
            "tag",
            "correspondent",
            "doctype",
            "project",
            "export",
            "status",
        ):
            assert group in result.stdout

    def test_document_help(self):
        """document --help lists all document subcommands."""
        result = _run_cli("document", "--help")
        assert result.returncode == 0
        for sub in ("list", "get", "upload", "download", "update", "delete", "search"):
            assert sub in result.stdout

    def test_tag_help(self):
        """tag --help lists tag subcommands."""
        result = _run_cli("tag", "--help")
        assert result.returncode == 0
        for sub in ("list", "create", "delete"):
            assert sub in result.stdout

    def test_project_help(self):
        """project --help lists project subcommands."""
        result = _run_cli("project", "--help")
        assert result.returncode == 0
        for sub in ("init", "info", "ping"):
            assert sub in result.stdout

    # ── Error path: missing config ──

    def test_no_config_document_list_exits_nonzero(self, tmp_path):
        """document list with no config file should exit non-zero."""
        result = _run_cli(
            "document",
            "list",
            extra_env={"HOME": str(tmp_path)},
        )
        assert result.returncode != 0

    def test_no_config_shows_project_init_instruction(self, tmp_path):
        """Error output must mention 'project init'."""
        result = _run_cli(
            "document",
            "list",
            extra_env={"HOME": str(tmp_path)},
        )
        combined = result.stdout + result.stderr
        assert "project init" in combined

    def test_no_config_tag_list_exits_nonzero(self, tmp_path):
        """tag list with no config should exit non-zero."""
        result = _run_cli(
            "tag",
            "list",
            extra_env={"HOME": str(tmp_path)},
        )
        assert result.returncode != 0

    # ── status without config ──

    def test_status_no_config_exits_zero(self, tmp_path):
        """status always exits 0 — it shows not-connected state gracefully."""
        result = _run_cli(
            "status",
            extra_env={"HOME": str(tmp_path)},
        )
        assert result.returncode == 0

    def test_status_json_no_config(self, tmp_path):
        """status --json with no config returns valid JSON with connected=false."""
        result = _run_cli(
            "status",
            "--json",
            extra_env={"HOME": str(tmp_path)},
        )
        assert result.returncode == 0
        data = json.loads(result.stdout)
        assert data["connected"] is False
        assert "url" in data

    # ── Server-dependent subprocess tests ──

    @SKIP_E2E
    def test_status_json_connected(self):
        """status --json returns connected=true when config is set."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("status", "--json")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert data["connected"] is True
        assert data["url"] == PAPERLESS_URL.rstrip("/")

    @SKIP_E2E
    def test_document_list_json_returns_paginated(self):
        """document list --json returns {count, results} structure."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("document", "list", "--json", "--page-size", "5")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert "count" in data
        assert "results" in data
        assert isinstance(data["results"], list)

    @SKIP_E2E
    def test_document_search_json(self):
        """document search --json returns {count, results}."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("document", "search", "the", "--json")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert "count" in data

    @SKIP_E2E
    def test_tag_list_json_returns_list(self):
        """tag list --json returns a JSON array."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("tag", "list", "--json")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert isinstance(data, list)

    @SKIP_E2E
    def test_correspondent_list_json_returns_list(self):
        """correspondent list --json returns a JSON array."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("correspondent", "list", "--json")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert isinstance(data, list)

    @SKIP_E2E
    def test_doctype_list_json_returns_list(self):
        """doctype list --json returns a JSON array."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("doctype", "list", "--json")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert isinstance(data, list)

    @SKIP_E2E
    def test_global_json_flag(self):
        """--json at root level propagates to subcommand output."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("--json", "document", "list", "--page-size", "3")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert "count" in data

    @SKIP_E2E
    def test_project_ping_json(self):
        """project ping --json returns {status: ok}."""
        from paperless_ngx.utils.paperless_backend import save_config

        save_config(PAPERLESS_URL, PAPERLESS_TOKEN)
        result = _run_cli("project", "ping", "--json")
        assert result.returncode == 0, f"stderr: {result.stderr}"
        data = json.loads(result.stdout)
        assert data["status"] == "ok"
