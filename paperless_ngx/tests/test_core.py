"""Unit tests for paperless-ngx-cli core modules.

All HTTP calls are mocked with the `responses` library.
No real Paperless-ngx server is required.

Config and session files are isolated to tmp_path by conftest.py (autouse).
"""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import ANY, MagicMock, patch

import pytest
import responses as resp_lib
from click.testing import CliRunner

import paperless_ngx.utils.paperless_backend as be_mod
from paperless_ngx.core.correspondents import (
    create_correspondent,
    delete_correspondent,
    get_correspondent,
    list_correspondents,
)
from paperless_ngx.core.doc_types import (
    create_doc_type,
    delete_doc_type,
    get_doc_type,
    list_doc_types,
)
from paperless_ngx.core.documents import (
    _guess_mime,
    build_document_query_params,
    delete_document,
    download_document,
    download_document_asset,
    download_document_preview,
    download_document_thumbnail,
    get_document,
    list_documents,
    search_documents,
    update_document,
    upload_document,
)
from paperless_ngx.core.search import autocomplete_search, query_search
from paperless_ngx.core.session import Session
from paperless_ngx.core.tags import create_tag, delete_tag, get_tag, list_tags
from paperless_ngx.core.tasks import get_task, list_tasks
from paperless_ngx.paperless_ngx_cli import _REPL_HELP, _pretty_print, main
from paperless_ngx.utils.paperless_backend import (
    PaperlessBackend,
    PaperlessConfig,
    find_paperless_server,
    load_session,
    save_config,
    save_session,
)

BASE_URL = "http://paperless.test"


# ── Factories ─────────────────────────────────────────────────────────────────


def make_config() -> PaperlessConfig:
    return PaperlessConfig(url=BASE_URL, token="testtoken123")


def make_backend() -> PaperlessBackend:
    return PaperlessBackend(config=make_config())


def write_config(
    tmp_path_or_path: str | Path, url: str = BASE_URL, token: str = "testtoken123"
):
    """Write a valid config.json to the mocked CONFIG_PATH."""
    # be_mod.CONFIG_PATH is already redirected by conftest autouse fixture
    p = Path(be_mod.CONFIG_PATH)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps({"url": url, "token": token}))


# ── TestPaperlessConfig ───────────────────────────────────────────────────────


class TestPaperlessConfig:
    def test_api_url_no_trailing_slash_on_base(self):
        cfg = PaperlessConfig(url="http://example.com/", token="tok")
        assert cfg.api_url("documents/") == "http://example.com/api/documents/"

    def test_api_url_leading_slash_stripped_from_path(self):
        cfg = PaperlessConfig(url="http://example.com", token="tok")
        assert cfg.api_url("/tags/1/") == "http://example.com/api/tags/1/"

    def test_to_dict_roundtrip(self):
        cfg = PaperlessConfig(url="http://x.com", token="mytoken")
        d = cfg.to_dict()
        cfg2 = PaperlessConfig.from_dict(d)
        assert cfg2.url == "http://x.com"
        assert cfg2.token == "mytoken"

    def test_url_trailing_slash_stripped(self):
        cfg = PaperlessConfig(url="http://x.com///", token="t")
        assert not cfg.url.endswith("/")


# ── TestFindPaperlessServer ───────────────────────────────────────────────────


class TestFindPaperlessServer:
    def test_missing_config_raises(self):
        # CONFIG_PATH redirected to tmp_path by conftest; file does not exist
        with pytest.raises(RuntimeError, match="project init"):
            find_paperless_server()

    def test_malformed_config_raises(self):
        Path(be_mod.CONFIG_PATH).parent.mkdir(parents=True, exist_ok=True)
        Path(be_mod.CONFIG_PATH).write_text("not valid json {{")
        with pytest.raises(RuntimeError, match="malformed"):
            find_paperless_server()

    def test_missing_url_field_raises(self):
        Path(be_mod.CONFIG_PATH).parent.mkdir(parents=True, exist_ok=True)
        Path(be_mod.CONFIG_PATH).write_text(json.dumps({"token": "abc"}))
        with pytest.raises(RuntimeError):
            find_paperless_server()

    def test_missing_token_field_raises(self):
        Path(be_mod.CONFIG_PATH).parent.mkdir(parents=True, exist_ok=True)
        Path(be_mod.CONFIG_PATH).write_text(json.dumps({"url": "http://x.com"}))
        with pytest.raises(RuntimeError):
            find_paperless_server()

    def test_valid_config_loads(self):
        save_config("http://valid.host", "mytoken999")
        result = find_paperless_server()
        assert result.url == "http://valid.host"
        assert result.token == "mytoken999"


# ── TestSaveConfig ────────────────────────────────────────────────────────────


class TestSaveConfig:
    def test_creates_directory_and_file(self):
        save_config("http://example.com", "tok123")
        p = Path(be_mod.CONFIG_PATH)
        assert p.exists()
        data = json.loads(p.read_text())
        assert data["url"] == "http://example.com"
        assert data["token"] == "tok123"

    def test_overwrites_existing(self):
        save_config("http://old.host", "oldtok")
        save_config("http://new.host", "newtok")
        data = json.loads(Path(be_mod.CONFIG_PATH).read_text())
        assert data["url"] == "http://new.host"
        assert data["token"] == "newtok"


# ── TestSessionPersistence ────────────────────────────────────────────────────


class TestSessionPersistence:
    def test_save_and_load_session(self):
        state = {"last_query": "test", "selected_docs": [1, 2], "history": []}
        save_session(state)
        loaded = load_session()
        assert loaded["last_query"] == "test"
        assert loaded["selected_docs"] == [1, 2]

    def test_load_session_defaults_when_missing(self):
        # SESSION_PATH redirected to tmp_path by conftest; file does not exist
        loaded = load_session()
        assert loaded == {"last_query": "", "selected_docs": [], "history": []}

    def test_load_session_defaults_on_corrupt_file(self):
        Path(be_mod.SESSION_PATH).write_text("not json")
        loaded = load_session()
        assert loaded == {"last_query": "", "selected_docs": [], "history": []}


# ── TestPaperlessBackend ──────────────────────────────────────────────────────


class TestPaperlessBackend:
    @resp_lib.activate
    def test_get_success(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 1, "results": [{"id": 1}]},
            status=200,
        )
        backend = make_backend()
        data = backend.get("documents/")
        assert data["count"] == 1

    @resp_lib.activate
    def test_get_sends_auth_header(self):
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/documents/", json={}, status=200)
        backend = make_backend()
        backend.get("documents/")
        req = resp_lib.calls[0].request
        assert req.headers["Authorization"] == "Token testtoken123"

    @resp_lib.activate
    def test_get_401_raises(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"detail": "auth error"},
            status=401,
        )
        backend = make_backend()
        with pytest.raises(RuntimeError, match="Authentication failed"):
            backend.get("documents/")

    @resp_lib.activate
    def test_get_403_raises(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/1/",
            json={"detail": "forbidden"},
            status=403,
        )
        backend = make_backend()
        with pytest.raises(RuntimeError, match="Permission denied"):
            backend.get("documents/1/")

    @resp_lib.activate
    def test_get_404_raises(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/999/",
            json={"detail": "not found"},
            status=404,
        )
        backend = make_backend()
        with pytest.raises(RuntimeError, match="not found"):
            backend.get("documents/999/")

    @resp_lib.activate
    def test_get_500_raises(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"error": "server error"},
            status=500,
        )
        backend = make_backend()
        with pytest.raises(RuntimeError, match="500"):
            backend.get("documents/")

    @resp_lib.activate
    def test_post_json(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/tags/",
            json={"id": 5, "name": "test"},
            status=201,
        )
        backend = make_backend()
        result = backend.post("tags/", data={"name": "test"})
        assert result["id"] == 5

    @resp_lib.activate
    def test_post_multipart(self, tmp_path):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/documents/post_document/",
            json={"task_id": "abc123"},
            status=200,
        )
        backend = make_backend()
        pdf_file = tmp_path / "test.pdf"
        pdf_file.write_bytes(b"%PDF fake content")
        result = backend.post(
            "documents/post_document/",
            files={"document": ("test.pdf", pdf_file.open("rb"), "application/pdf")},
        )
        assert result["task_id"] == "abc123"

    @resp_lib.activate
    def test_post_204_returns_empty_dict(self):
        resp_lib.add(resp_lib.POST, f"{BASE_URL}/api/something/", status=204)
        backend = make_backend()
        result = backend.post("something/", data={})
        assert result == {}

    @resp_lib.activate
    def test_patch(self):
        resp_lib.add(
            resp_lib.PATCH,
            f"{BASE_URL}/api/documents/1/",
            json={"id": 1, "title": "Updated"},
            status=200,
        )
        backend = make_backend()
        result = backend.patch("documents/1/", data={"title": "Updated"})
        assert result["title"] == "Updated"

    @resp_lib.activate
    def test_put(self):
        resp_lib.add(
            resp_lib.PUT,
            f"{BASE_URL}/api/documents/1/",
            json={"id": 1, "title": "Replaced"},
            status=200,
        )
        backend = make_backend()
        result = backend.put("documents/1/", data={"title": "Replaced"})
        assert result["title"] == "Replaced"

    @resp_lib.activate
    def test_delete(self):
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/documents/1/", status=204)
        backend = make_backend()
        backend.delete("documents/1/")  # Must not raise

    @resp_lib.activate
    def test_paginate_single_page(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tags/",
            json={
                "count": 2,
                "next": None,
                "previous": None,
                "results": [{"id": 1}, {"id": 2}],
            },
            status=200,
        )
        backend = make_backend()
        results = backend.paginate("tags/")
        assert len(results) == 2

    @resp_lib.activate
    def test_paginate_multiple_pages(self):
        page2_url = f"{BASE_URL}/api/tags/?page=2&page_size=100"
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tags/",
            json={
                "count": 3,
                "next": page2_url,
                "previous": None,
                "results": [{"id": 1}, {"id": 2}],
            },
            status=200,
        )
        resp_lib.add(
            resp_lib.GET,
            page2_url,
            json={"count": 3, "next": None, "previous": None, "results": [{"id": 3}]},
            status=200,
        )
        backend = make_backend()
        results = backend.paginate("tags/")
        assert len(results) == 3
        assert results[2]["id"] == 3

    @resp_lib.activate
    def test_paginate_plain_list_response(self):
        """Some endpoints may return a plain JSON list (not paginated)."""
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tasks/",
            json=[{"id": 1}, {"id": 2}],
            status=200,
        )
        backend = make_backend()
        results = backend.paginate("tasks/")
        assert results == [{"id": 1}, {"id": 2}]

    @resp_lib.activate
    def test_ping_success(self):
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/status/", json={}, status=200)
        backend = make_backend()
        result = backend.ping()
        assert result["status"] == "ok"
        assert result["url"] == BASE_URL
        assert "response_code" in result

    def test_ping_connection_error(self):
        import requests

        backend = make_backend()
        with (
            patch.object(
                backend._session,
                "get",
                side_effect=requests.ConnectionError("refused"),
            ),
            pytest.raises(RuntimeError, match="Cannot connect"),
        ):
            backend.ping()

    def test_ping_timeout(self):
        import requests

        backend = make_backend()
        with (
            patch.object(
                backend._session,
                "get",
                side_effect=requests.Timeout("timed out"),
            ),
            pytest.raises(RuntimeError, match="timed out"),
        ):
            backend.ping()

    @resp_lib.activate
    def test_get_token_success(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/token/",
            json={"token": "mytoken456"},
            status=200,
        )
        backend = make_backend()
        token = backend.get_token("admin", "password")
        assert token == "mytoken456"

    @resp_lib.activate
    def test_get_token_failure(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/token/",
            json={"non_field_errors": ["bad creds"]},
            status=400,
        )
        backend = make_backend()
        with pytest.raises(RuntimeError, match="Failed to obtain token"):
            backend.get_token("admin", "wrongpass")


# ── TestDocuments ─────────────────────────────────────────────────────────────


class TestDocuments:
    def test_build_document_query_params_with_extended_filters(self):
        params = build_document_query_params(
            query="invoice",
            tag="urgent",
            tag_id=5,
            correspondent="ACME",
            correspondent_id=7,
            doc_type="Invoice",
            doc_type_id=9,
            created_after="2024-01-01",
            created_before="2024-12-31",
            order_by="title",
            page_size=10,
            page=2,
        )
        assert params["query"] == "invoice"
        assert params["tags__name__icontains"] == "urgent"
        assert params["tags__id__in"] == "5"
        assert params["correspondent__id"] == 7
        assert params["document_type__id"] == 9
        assert params["created__date__gt"] == "2024-01-01"
        assert params["created__date__lt"] == "2024-12-31"
        assert params["ordering"] == "title"
        assert params["page_size"] == 10
        assert params["page"] == 2

    @resp_lib.activate
    def test_list_no_filter(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        backend = make_backend()
        result = list_documents(backend)
        assert result["count"] == 0

    @resp_lib.activate
    def test_list_with_query(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 1, "results": [{"id": 1, "title": "inv"}]},
            status=200,
        )
        backend = make_backend()
        result = list_documents(backend, query="invoice")
        assert result["count"] == 1
        assert "query=invoice" in resp_lib.calls[0].request.url

    @resp_lib.activate
    def test_list_with_tag_filter(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        backend = make_backend()
        list_documents(backend, tag="urgent")
        assert "tags__name__icontains=urgent" in resp_lib.calls[0].request.url

    @resp_lib.activate
    def test_list_with_tag_id_filter(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        backend = make_backend()
        list_documents(backend, tag_id=12)
        assert "tags__id__in=12" in resp_lib.calls[0].request.url

    @resp_lib.activate
    def test_list_with_correspondent_filter(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        backend = make_backend()
        list_documents(backend, correspondent="ACME")
        assert "correspondent__name__icontains=ACME" in resp_lib.calls[0].request.url

    @resp_lib.activate
    def test_list_with_doc_type_filter(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        backend = make_backend()
        list_documents(backend, doc_type="Invoice")
        assert "document_type__name__icontains=Invoice" in resp_lib.calls[0].request.url

    @resp_lib.activate
    def test_list_with_date_filters_and_ordering(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        backend = make_backend()
        list_documents(
            backend,
            created_after="2024-01-01",
            created_before="2024-12-31",
            order_by="title",
        )
        url = resp_lib.calls[0].request.url
        assert "created__date__gt=2024-01-01" in url
        assert "created__date__lt=2024-12-31" in url
        assert "ordering=title" in url

    @resp_lib.activate
    def test_list_page_size_respected(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        backend = make_backend()
        list_documents(backend, page_size=10, page=2)
        url = resp_lib.calls[0].request.url
        assert "page_size=10" in url
        assert "page=2" in url

    @resp_lib.activate
    def test_get_document(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/42/",
            json={"id": 42, "title": "Test Doc"},
            status=200,
        )
        backend = make_backend()
        result = get_document(backend, 42)
        assert result["id"] == 42
        assert result["title"] == "Test Doc"

    @resp_lib.activate
    def test_upload_document(self, tmp_path):
        pdf_file = tmp_path / "test.pdf"
        pdf_file.write_bytes(b"%PDF-1.4 fake")
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/documents/post_document/",
            json={"task_id": "uuid-123"},
            status=200,
        )
        backend = make_backend()
        result = upload_document(backend, str(pdf_file), title="Test Upload")
        assert result["task_id"] == "uuid-123"

    @resp_lib.activate
    def test_upload_with_tags_and_metadata(self, tmp_path):
        pdf_file = tmp_path / "invoice.pdf"
        pdf_file.write_bytes(b"%PDF-1.4 fake")
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/documents/post_document/",
            json={"task_id": "uuid-456"},
            status=200,
        )
        backend = make_backend()
        result = upload_document(
            backend,
            str(pdf_file),
            title="Invoice",
            correspondent_id=3,
            document_type_id=5,
            tag_ids=[1, 2],
        )
        assert result["task_id"] == "uuid-456"

    def test_upload_missing_file_raises(self):
        backend = make_backend()
        with pytest.raises(FileNotFoundError):
            upload_document(backend, "/nonexistent/file.pdf")

    @resp_lib.activate
    def test_download_document_with_content_disposition(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/1/download/",
            body=b"%PDF-1.4 content",
            headers={"Content-Disposition": 'attachment; filename="doc_1.pdf"'},
            status=200,
        )
        backend = make_backend()
        path = download_document(backend, 1, output_dir=str(tmp_path))
        assert path.endswith("doc_1.pdf")
        assert Path(path).exists()
        assert Path(path).read_bytes() == b"%PDF-1.4 content"

    @resp_lib.activate
    def test_download_document_fallback_filename(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/99/download/",
            body=b"content",
            headers={},  # No Content-Disposition
            status=200,
        )
        backend = make_backend()
        path = download_document(backend, 99, output_dir=str(tmp_path))
        assert "99" in path  # fallback uses doc id

    @resp_lib.activate
    def test_download_original_passes_param(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/1/download/",
            body=b"original content",
            headers={"Content-Disposition": 'attachment; filename="orig.pdf"'},
            status=200,
        )
        backend = make_backend()
        download_document(backend, 1, output_dir=str(tmp_path), original=True)
        url = resp_lib.calls[0].request.url
        assert "original=true" in url

    @resp_lib.activate
    def test_download_preview_uses_preview_endpoint(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/7/preview/",
            body=b"preview",
            headers={"Content-Disposition": 'attachment; filename="preview-7.webp"'},
            status=200,
        )
        backend = make_backend()
        path = download_document_preview(backend, 7, output_dir=str(tmp_path))
        assert path.endswith("preview-7.webp")
        assert Path(path).read_bytes() == b"preview"

    @resp_lib.activate
    def test_download_thumb_fallback_filename(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/8/thumb/",
            body=b"thumb",
            headers={},
            status=200,
        )
        backend = make_backend()
        path = download_document_thumbnail(backend, 8, output_dir=str(tmp_path))
        assert path.endswith("document_8_thumb")
        assert Path(path).read_bytes() == b"thumb"

    @resp_lib.activate
    def test_download_document_asset_respects_custom_asset_name(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/6/preview/",
            body=b"asset",
            headers={},
            status=200,
        )
        backend = make_backend()
        path = download_document_asset(
            backend, 6, asset="preview", output_dir=str(tmp_path)
        )
        assert path.endswith("document_6_preview")

    @resp_lib.activate
    def test_update_document_title(self):
        resp_lib.add(
            resp_lib.PATCH,
            f"{BASE_URL}/api/documents/1/",
            json={"id": 1, "title": "New Title"},
            status=200,
        )
        backend = make_backend()
        result = update_document(backend, 1, title="New Title")
        assert result["title"] == "New Title"

    @resp_lib.activate
    def test_update_document_tags(self):
        resp_lib.add(
            resp_lib.PATCH,
            f"{BASE_URL}/api/documents/1/",
            json={"id": 1, "tags": [2, 3]},
            status=200,
        )
        backend = make_backend()
        result = update_document(backend, 1, tag_ids=[2, 3])
        assert result["tags"] == [2, 3]

    def test_update_no_fields_raises(self):
        backend = make_backend()
        with pytest.raises(ValueError, match="No fields"):
            update_document(backend, 1)

    @resp_lib.activate
    def test_delete_document(self):
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/documents/1/", status=204)
        backend = make_backend()
        delete_document(backend, 1)  # Must not raise

    @resp_lib.activate
    def test_search_documents(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 2, "results": [{"id": 1}, {"id": 2}]},
            status=200,
        )
        backend = make_backend()
        result = search_documents(backend, "invoice 2024")
        assert result["count"] == 2
        assert "query=invoice+2024" in resp_lib.calls[0].request.url

    @resp_lib.activate
    def test_search_documents_with_filters(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 1, "results": [{"id": 9}]},
            status=200,
        )
        backend = make_backend()
        search_documents(
            backend,
            "invoice 2024",
            tag_id=3,
            created_after="2024-01-01",
            order_by="-added",
        )
        url = resp_lib.calls[0].request.url
        assert "tags__id__in=3" in url
        assert "created__date__gt=2024-01-01" in url
        assert "ordering=-added" in url


class TestGlobalSearch:
    @resp_lib.activate
    def test_query_search(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/search/",
            json={"count": 1, "results": [{"document": {"id": 1}}]},
            status=200,
        )
        backend = make_backend()
        result = query_search(backend, "invoice", page_size=5, page=2)
        assert result["count"] == 1
        url = resp_lib.calls[0].request.url
        assert "query=invoice" in url
        assert "page_size=5" in url
        assert "page=2" in url

    @resp_lib.activate
    def test_autocomplete_search(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/search/autocomplete/",
            json=["invoice", "invoices"],
            status=200,
        )
        backend = make_backend()
        result = autocomplete_search(backend, "inv", limit=7)
        assert result == ["invoice", "invoices"]
        url = resp_lib.calls[0].request.url
        assert "term=inv" in url
        assert "limit=7" in url


class TestTasks:
    @resp_lib.activate
    def test_list_tasks(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tasks/",
            json={
                "count": 1,
                "next": None,
                "results": [{"id": 1, "status": "SUCCESS"}],
            },
            status=200,
        )
        backend = make_backend()
        result = list_tasks(backend)
        assert len(result) == 1
        assert result[0]["id"] == 1

    @resp_lib.activate
    def test_get_task(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tasks/4/",
            json={"id": 4, "status": "PENDING"},
            status=200,
        )
        backend = make_backend()
        result = get_task(backend, 4)
        assert result["status"] == "PENDING"


# ── TestTags ──────────────────────────────────────────────────────────────────


class TestTags:
    @resp_lib.activate
    def test_list_tags(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tags/",
            json={
                "count": 2,
                "next": None,
                "results": [{"id": 1, "name": "a"}, {"id": 2, "name": "b"}],
            },
            status=200,
        )
        backend = make_backend()
        result = list_tags(backend)
        assert len(result) == 2
        assert result[0]["name"] == "a"

    @resp_lib.activate
    def test_create_tag_defaults(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/tags/",
            json={"id": 10, "name": "newtag", "color": "#a6cee3"},
            status=201,
        )
        backend = make_backend()
        result = create_tag(backend, "newtag")
        assert result["id"] == 10

    @resp_lib.activate
    def test_create_tag_custom_color(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/tags/",
            json={"id": 11, "name": "urgent", "color": "#ff0000"},
            status=201,
        )
        backend = make_backend()
        create_tag(backend, "urgent", color="#ff0000")
        body = json.loads(resp_lib.calls[0].request.body)
        assert body["color"] == "#ff0000"

    @resp_lib.activate
    def test_create_inbox_tag(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/tags/",
            json={"id": 12, "name": "inbox", "is_inbox_tag": True},
            status=201,
        )
        backend = make_backend()
        create_tag(backend, "inbox", is_inbox_tag=True)
        body = json.loads(resp_lib.calls[0].request.body)
        assert body["is_inbox_tag"] is True

    @resp_lib.activate
    def test_delete_tag(self):
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/tags/10/", status=204)
        backend = make_backend()
        delete_tag(backend, 10)  # Must not raise

    @resp_lib.activate
    def test_get_tag(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tags/5/",
            json={"id": 5, "name": "mytag"},
            status=200,
        )
        backend = make_backend()
        result = get_tag(backend, 5)
        assert result["id"] == 5


# ── TestCorrespondents ────────────────────────────────────────────────────────


class TestCorrespondents:
    @resp_lib.activate
    def test_list_correspondents(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/correspondents/",
            json={"count": 1, "next": None, "results": [{"id": 1, "name": "ACME"}]},
            status=200,
        )
        backend = make_backend()
        result = list_correspondents(backend)
        assert len(result) == 1
        assert result[0]["name"] == "ACME"

    @resp_lib.activate
    def test_create_correspondent(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/correspondents/",
            json={"id": 5, "name": "New Corp"},
            status=201,
        )
        backend = make_backend()
        result = create_correspondent(backend, "New Corp")
        assert result["id"] == 5
        body = json.loads(resp_lib.calls[0].request.body)
        assert body["name"] == "New Corp"

    @resp_lib.activate
    def test_create_correspondent_with_match(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/correspondents/",
            json={"id": 6, "name": "ACME"},
            status=201,
        )
        backend = make_backend()
        create_correspondent(backend, "ACME", match="acme corp")
        body = json.loads(resp_lib.calls[0].request.body)
        assert body["match"] == "acme corp"

    @resp_lib.activate
    def test_delete_correspondent(self):
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/correspondents/5/", status=204)
        backend = make_backend()
        delete_correspondent(backend, 5)  # Must not raise

    @resp_lib.activate
    def test_get_correspondent(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/correspondents/5/",
            json={"id": 5, "name": "ACME"},
            status=200,
        )
        backend = make_backend()
        result = get_correspondent(backend, 5)
        assert result["id"] == 5


# ── TestDocTypes ──────────────────────────────────────────────────────────────


class TestDocTypes:
    @resp_lib.activate
    def test_list_doc_types(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/document_types/",
            json={"count": 1, "next": None, "results": [{"id": 1, "name": "Invoice"}]},
            status=200,
        )
        backend = make_backend()
        result = list_doc_types(backend)
        assert len(result) == 1
        assert result[0]["name"] == "Invoice"

    @resp_lib.activate
    def test_create_doc_type(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/document_types/",
            json={"id": 3, "name": "Contract"},
            status=201,
        )
        backend = make_backend()
        result = create_doc_type(backend, "Contract")
        assert result["name"] == "Contract"

    @resp_lib.activate
    def test_delete_doc_type(self):
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/document_types/3/", status=204)
        backend = make_backend()
        delete_doc_type(backend, 3)  # Must not raise

    @resp_lib.activate
    def test_get_doc_type(self):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/document_types/3/",
            json={"id": 3, "name": "Contract"},
            status=200,
        )
        backend = make_backend()
        result = get_doc_type(backend, 3)
        assert result["id"] == 3


# ── TestSession ───────────────────────────────────────────────────────────────


class TestSession:
    def test_last_query_set_and_get(self):
        with (
            patch("paperless_ngx.core.session.save_session"),
            patch("paperless_ngx.core.session.load_session", return_value={}),
        ):
            sess = Session()
            sess.last_query = "test query"
            assert sess.last_query == "test query"

    def test_selected_docs_set_and_get(self):
        with (
            patch("paperless_ngx.core.session.save_session"),
            patch("paperless_ngx.core.session.load_session", return_value={}),
        ):
            sess = Session()
            sess.selected_docs = [1, 2, 3]
            assert sess.selected_docs == [1, 2, 3]

    def test_add_history_appends(self):
        with (
            patch("paperless_ngx.core.session.save_session"),
            patch("paperless_ngx.core.session.load_session", return_value={}),
        ):
            sess = Session()
            sess.add_history("document list")
            sess.add_history("tag list")
            assert sess.history == ["document list", "tag list"]

    def test_history_capped_at_500(self):
        with (
            patch("paperless_ngx.core.session.save_session"),
            patch("paperless_ngx.core.session.load_session", return_value={}),
        ):
            sess = Session()
            for i in range(600):
                sess.add_history(f"cmd {i}")
            assert len(sess.history) == 500
            # Last 500 are kept
            assert sess.history[-1] == "cmd 599"

    def test_clear_selection(self):
        with (
            patch("paperless_ngx.core.session.save_session"),
            patch(
                "paperless_ngx.core.session.load_session",
                return_value={"selected_docs": [1, 2]},
            ),
        ):
            sess = Session()
            assert sess.selected_docs == [1, 2]
            sess.clear_selection()
            assert sess.selected_docs == []

    def test_to_dict(self):
        with (
            patch("paperless_ngx.core.session.save_session"),
            patch(
                "paperless_ngx.core.session.load_session",
                return_value={
                    "last_query": "foo",
                    "selected_docs": [],
                    "history": [],
                },
            ),
        ):
            sess = Session()
            d = sess.to_dict()
            assert d["last_query"] == "foo"

    def test_persist_is_called_on_mutation(self):
        save_mock = MagicMock()
        with (
            patch("paperless_ngx.core.session.save_session", save_mock),
            patch("paperless_ngx.core.session.load_session", return_value={}),
        ):
            sess = Session()
            sess.last_query = "x"
            assert save_mock.called


# ── TestProjectCore ───────────────────────────────────────────────────────────


class TestProjectCore:
    @resp_lib.activate
    def test_init_connection_with_token(self):
        # ping endpoint
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/status/", json={}, status=200)
        from paperless_ngx.core.project import init_connection

        config = init_connection(BASE_URL, token="mytoken")
        assert config.url == BASE_URL
        assert config.token == "mytoken"
        # Config should be persisted
        saved = json.loads(Path(be_mod.CONFIG_PATH).read_text())
        assert saved["token"] == "mytoken"

    @resp_lib.activate
    def test_init_connection_with_credentials(self):
        # Token acquisition endpoint
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/token/",
            json={"token": "acquired_token"},
            status=200,
        )
        # ping endpoint
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/status/", json={}, status=200)
        from paperless_ngx.core.project import init_connection

        config = init_connection(BASE_URL, username="admin", password="pass")
        assert config.token == "acquired_token"

    def test_init_connection_no_auth_raises(self):
        from paperless_ngx.core.project import init_connection

        with pytest.raises(ValueError, match="Provide either"):
            init_connection(BASE_URL)

    @resp_lib.activate
    def test_init_connection_credential_failure_raises(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/token/",
            json={"non_field_errors": ["bad creds"]},
            status=400,
        )
        from paperless_ngx.core.project import init_connection

        with pytest.raises(RuntimeError, match="Failed to obtain token"):
            init_connection(BASE_URL, username="admin", password="wrong")

    @resp_lib.activate
    def test_get_connection_info(self):
        # Use a token long enough to trigger masking (>8 chars)
        save_config(BASE_URL, "longtesttoken123")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/statistics/",
            json={"documents_total": 42},
            status=200,
        )
        from paperless_ngx.core.project import get_connection_info

        info = get_connection_info()
        assert info["url"] == BASE_URL
        # Token should be masked but show first 8 chars + "..."
        assert info["token"].endswith("...")
        assert info["token"].startswith("longte")
        assert info["statistics"]["documents_total"] == 42

    @resp_lib.activate
    def test_ping_server(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/status/", json={}, status=200)
        from paperless_ngx.core.project import ping_server

        result = ping_server()
        assert result["status"] == "ok"
        assert result["elapsed_ms"] >= 0

    def test_ping_server_no_config_raises(self):
        from paperless_ngx.core.project import ping_server

        with pytest.raises(RuntimeError, match="project init"):
            ping_server()


# ── TestExportCore ────────────────────────────────────────────────────────────


class TestExportCore:
    @resp_lib.activate
    def test_bulk_download_all_ok(self, tmp_path):
        for doc_id in [1, 2, 3]:
            resp_lib.add(
                resp_lib.GET,
                f"{BASE_URL}/api/documents/{doc_id}/download/",
                body=b"PDF content",
                headers={
                    "Content-Disposition": f'attachment; filename="doc_{doc_id}.pdf"'
                },
                status=200,
            )
        from paperless_ngx.core.export import bulk_download

        backend = make_backend()
        results = bulk_download(backend, [1, 2, 3], output_dir=str(tmp_path))
        assert len(results) == 3
        assert all(r["status"] == "ok" for r in results)
        assert all(Path(r["path"]).exists() for r in results)

    @resp_lib.activate
    def test_bulk_download_partial_failure(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/1/download/",
            body=b"PDF content",
            headers={"Content-Disposition": 'attachment; filename="doc_1.pdf"'},
            status=200,
        )
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/2/download/",
            json={"detail": "not found"},
            status=404,
        )
        from paperless_ngx.core.export import bulk_download

        backend = make_backend()
        results = bulk_download(backend, [1, 2], output_dir=str(tmp_path))
        assert results[0]["status"] == "ok"
        assert results[1]["status"] == "error"
        assert "error" in results[1]

    @resp_lib.activate
    def test_bulk_download_progress_callback(self, tmp_path):
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/1/download/",
            body=b"PDF",
            headers={"Content-Disposition": 'attachment; filename="d.pdf"'},
            status=200,
        )
        progress_calls = []

        def progress(current, total, doc_id):
            progress_calls.append((current, total, doc_id))

        from paperless_ngx.core.export import bulk_download

        backend = make_backend()
        bulk_download(
            backend, [1], output_dir=str(tmp_path), progress_callback=progress
        )
        assert len(progress_calls) >= 2  # at least one mid-progress + final


# ── TestGuessMime ─────────────────────────────────────────────────────────────


class TestGuessMime:
    def test_pdf(self):
        assert _guess_mime(Path("test.pdf")) == "application/pdf"

    def test_jpg(self):
        assert _guess_mime(Path("photo.jpg")) == "image/jpeg"

    def test_jpeg(self):
        assert _guess_mime(Path("photo.jpeg")) == "image/jpeg"

    def test_png(self):
        assert _guess_mime(Path("image.png")) == "image/png"

    def test_tiff(self):
        assert _guess_mime(Path("scan.tiff")) == "image/tiff"

    def test_tif(self):
        assert _guess_mime(Path("scan.tif")) == "image/tiff"

    def test_txt(self):
        assert _guess_mime(Path("readme.txt")) == "text/plain"

    def test_odt(self):
        assert _guess_mime(Path("doc.odt")) == "application/vnd.oasis.opendocument.text"

    def test_unknown(self):
        assert _guess_mime(Path("file.xyz")) == "application/octet-stream"

    def test_uppercase_extension(self):
        # Extensions are lowercased before lookup
        assert _guess_mime(Path("SCAN.PDF")) == "application/pdf"


# ── TestCLILayer ──────────────────────────────────────────────────────────────


class TestCLILayer:
    """Tests of the Click CLI layer using CliRunner (no subprocess)."""

    def _runner(self):
        return CliRunner()

    # ── help / version ──

    def test_version(self):
        runner = self._runner()
        result = runner.invoke(main, ["--version"])
        assert result.exit_code == 0
        assert "1.0.0" in result.output

    def test_help_lists_groups(self):
        runner = self._runner()
        result = runner.invoke(main, ["--help"])
        assert result.exit_code == 0
        for group in (
            "document",
            "search",
            "tag",
            "correspondent",
            "doctype",
            "project",
            "task",
            "export",
            "status",
            "repl",
        ):
            assert group in result.output

    def test_document_help(self):
        runner = self._runner()
        result = runner.invoke(main, ["document", "--help"])
        assert result.exit_code == 0
        for cmd in (
            "list",
            "get",
            "upload",
            "download",
            "preview",
            "thumb",
            "update",
            "delete",
            "search",
        ):
            assert cmd in result.output

    def test_search_help(self):
        runner = self._runner()
        result = runner.invoke(main, ["search", "--help"])
        assert result.exit_code == 0
        for cmd in ("query", "autocomplete"):
            assert cmd in result.output

    def test_task_help(self):
        runner = self._runner()
        result = runner.invoke(main, ["task", "--help"])
        assert result.exit_code == 0
        for cmd in ("list", "get"):
            assert cmd in result.output

    def test_tag_help(self):
        runner = self._runner()
        result = runner.invoke(main, ["tag", "--help"])
        assert result.exit_code == 0
        for cmd in ("list", "get", "create", "delete"):
            assert cmd in result.output

    def test_document_update_help_describes_mutating_flags(self):
        runner = self._runner()
        result = runner.invoke(main, ["document", "update", "--help"])
        assert result.exit_code == 0
        assert "Tag IDs to set on the document." in result.output
        assert "existing tag list" in result.output
        assert "YYYY-MM-DD" in result.output

    def test_export_bulk_help_describes_zip_and_original_flags(self):
        runner = self._runner()
        result = runner.invoke(main, ["export", "bulk", "--help"])
        assert result.exit_code == 0
        assert "single ZIP" in result.output
        assert "original files" in result.output

    def test_project_init_help_describes_token_exchange(self):
        runner = self._runner()
        result = runner.invoke(main, ["project", "init", "--help"])
        assert result.exit_code == 0
        assert "exchange credentials for an API token" in result.output

    # ── status without config ──

    def test_status_no_config_shows_not_connected(self):
        runner = self._runner()
        result = runner.invoke(main, ["status"])
        assert result.exit_code == 0
        assert "not configured" in result.output or "False" in result.output

    def test_status_json_no_config(self):
        runner = self._runner()
        result = runner.invoke(main, ["status", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["connected"] is False
        assert "url" in data

    # ── document commands without config → ClickException ──

    def test_document_list_no_config_fails(self):
        runner = self._runner()
        result = runner.invoke(main, ["document", "list"])
        assert result.exit_code != 0
        assert "project init" in result.output or "project init" in (
            result.stderr or ""
        )

    def test_document_list_no_config_json_flag(self):
        runner = self._runner()
        result = runner.invoke(main, ["--json", "document", "list"])
        assert result.exit_code != 0

    # ── document list with mocked backend ──

    @resp_lib.activate
    def test_document_list_json_output(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={
                "count": 1,
                "next": None,
                "results": [{"id": 1, "title": "My Doc", "created": "2024-01-01"}],
            },
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["document", "list", "--json"])
        assert result.exit_code == 0, result.output
        data = json.loads(result.output)
        assert data["count"] == 1
        assert data["results"][0]["id"] == 1

    @resp_lib.activate
    def test_document_list_with_query(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["document", "list", "--query", "invoice"])
        assert result.exit_code == 0
        assert "query=invoice" in resp_lib.calls[0].request.url

    @resp_lib.activate
    def test_document_list_with_extended_filters(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(
            main,
            [
                "document",
                "list",
                "--tag-id",
                "4",
                "--created-after",
                "2024-01-01",
                "--order-by",
                "title",
            ],
        )
        assert result.exit_code == 0
        url = resp_lib.calls[0].request.url
        assert "tags__id__in=4" in url
        assert "created__date__gt=2024-01-01" in url
        assert "ordering=title" in url

    @resp_lib.activate
    def test_document_get_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/42/",
            json={"id": 42, "title": "Test"},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["document", "get", "42", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 42

    @resp_lib.activate
    def test_document_search_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 1, "results": [{"id": 7}]},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["document", "search", "test query", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["count"] == 1

    @resp_lib.activate
    def test_document_search_with_filters_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 1, "results": [{"id": 7}]},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(
            main,
            [
                "document",
                "search",
                "test query",
                "--tag-id",
                "2",
                "--created-before",
                "2024-12-31",
                "--order-by",
                "-added",
                "--json",
            ],
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["count"] == 1
        url = resp_lib.calls[0].request.url
        assert "tags__id__in=2" in url
        assert "created__date__lt=2024-12-31" in url
        assert "ordering=-added" in url

    @resp_lib.activate
    def test_search_query_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/search/",
            json={"count": 1, "results": [{"rank": 0}]},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["search", "query", "invoice", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["count"] == 1

    @resp_lib.activate
    def test_search_autocomplete_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/search/autocomplete/",
            json=["invoice", "invoices"],
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(
            main, ["search", "autocomplete", "inv", "--limit", "5", "--json"]
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data == ["invoice", "invoices"]

    @resp_lib.activate
    def test_task_list_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tasks/",
            json={
                "count": 1,
                "next": None,
                "results": [{"id": 1, "status": "SUCCESS"}],
            },
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["task", "list", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data[0]["status"] == "SUCCESS"

    @resp_lib.activate
    def test_task_get_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tasks/1/",
            json={"id": 1, "status": "SUCCESS"},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["task", "get", "1", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 1

    @resp_lib.activate
    def test_document_delete_with_yes_flag(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/documents/5/", status=204)
        runner = self._runner()
        result = runner.invoke(main, ["document", "delete", "5", "--yes", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "deleted"
        assert data["doc_id"] == 5

    @resp_lib.activate
    def test_document_update_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.PATCH,
            f"{BASE_URL}/api/documents/3/",
            json={"id": 3, "title": "New"},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(
            main, ["document", "update", "3", "--title", "New", "--json"]
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["title"] == "New"

    @resp_lib.activate
    def test_document_upload_json(self, tmp_path):
        save_config(BASE_URL, "tok")
        pdf_file = tmp_path / "doc.pdf"
        pdf_file.write_bytes(b"%PDF fake")
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/documents/post_document/",
            json={"task_id": "t-1"},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["document", "upload", str(pdf_file), "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["task_id"] == "t-1"

    @resp_lib.activate
    def test_document_download_json(self, tmp_path):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/1/download/",
            body=b"%PDF",
            headers={"Content-Disposition": 'attachment; filename="d.pdf"'},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(
            main, ["document", "download", "1", "--output-dir", str(tmp_path), "--json"]
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "ok"
        assert data["doc_id"] == 1

    @resp_lib.activate
    def test_document_preview_json(self, tmp_path):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/2/preview/",
            body=b"preview",
            headers={"Content-Disposition": 'attachment; filename="preview.png"'},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(
            main, ["document", "preview", "2", "--output-dir", str(tmp_path), "--json"]
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["asset"] == "preview"
        assert data["doc_id"] == 2

    @resp_lib.activate
    def test_document_thumb_json(self, tmp_path):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/3/thumb/",
            body=b"thumb",
            headers={"Content-Disposition": 'attachment; filename="thumb.webp"'},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(
            main, ["document", "thumb", "3", "--output-dir", str(tmp_path), "--json"]
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["asset"] == "thumb"
        assert data["doc_id"] == 3

    # ── tag commands ──

    @resp_lib.activate
    def test_tag_list_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tags/",
            json={"count": 1, "next": None, "results": [{"id": 1, "name": "urgent"}]},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["tag", "list", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert isinstance(data, list)
        assert data[0]["name"] == "urgent"

    @resp_lib.activate
    def test_tag_create_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/tags/",
            json={"id": 7, "name": "new-tag"},
            status=201,
        )
        runner = self._runner()
        result = runner.invoke(main, ["tag", "create", "new-tag", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 7

    @resp_lib.activate
    def test_tag_delete_with_yes_flag(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/tags/7/", status=204)
        runner = self._runner()
        result = runner.invoke(main, ["tag", "delete", "7", "--yes", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "deleted"

    @resp_lib.activate
    def test_tag_get_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/tags/7/",
            json={"id": 7, "name": "new-tag"},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["tag", "get", "7", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 7

    # ── correspondent commands ──

    @resp_lib.activate
    def test_correspondent_list_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/correspondents/",
            json={"count": 1, "next": None, "results": [{"id": 1, "name": "ACME"}]},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["correspondent", "list", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data[0]["name"] == "ACME"

    @resp_lib.activate
    def test_correspondent_create_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/correspondents/",
            json={"id": 9, "name": "Corp X"},
            status=201,
        )
        runner = self._runner()
        result = runner.invoke(main, ["correspondent", "create", "Corp X", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 9

    @resp_lib.activate
    def test_correspondent_delete_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/correspondents/9/", status=204)
        runner = self._runner()
        result = runner.invoke(
            main, ["correspondent", "delete", "9", "--yes", "--json"]
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "deleted"

    @resp_lib.activate
    def test_correspondent_get_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/correspondents/9/",
            json={"id": 9, "name": "Corp X"},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["correspondent", "get", "9", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 9

    # ── doctype commands ──

    @resp_lib.activate
    def test_doctype_list_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/document_types/",
            json={"count": 1, "next": None, "results": [{"id": 1, "name": "Invoice"}]},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["doctype", "list", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data[0]["name"] == "Invoice"

    @resp_lib.activate
    def test_doctype_create_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/document_types/",
            json={"id": 4, "name": "Receipt"},
            status=201,
        )
        runner = self._runner()
        result = runner.invoke(main, ["doctype", "create", "Receipt", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 4

    @resp_lib.activate
    def test_doctype_delete_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(resp_lib.DELETE, f"{BASE_URL}/api/document_types/4/", status=204)
        runner = self._runner()
        result = runner.invoke(main, ["doctype", "delete", "4", "--yes", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "deleted"

    @resp_lib.activate
    def test_doctype_get_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/document_types/4/",
            json={"id": 4, "name": "Receipt"},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["doctype", "get", "4", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["id"] == 4

    # ── project commands ──

    @resp_lib.activate
    def test_project_ping_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/status/", json={}, status=200)
        runner = self._runner()
        result = runner.invoke(main, ["project", "ping", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "ok"

    @resp_lib.activate
    def test_project_info_json(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/statistics/",
            json={"documents_total": 10},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["project", "info", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert "url" in data
        assert "statistics" in data

    @resp_lib.activate
    def test_project_init_with_token(self):
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/status/", json={}, status=200)
        runner = self._runner()
        result = runner.invoke(
            main,
            ["project", "init", "--url", BASE_URL, "--token", "newtok", "--json"],
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "ok"

    @resp_lib.activate
    def test_project_init_with_username_password_json(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/token/",
            json={"token": "acquired"},
            status=200,
        )
        resp_lib.add(resp_lib.GET, f"{BASE_URL}/api/status/", json={}, status=200)
        runner = self._runner()
        result = runner.invoke(
            main,
            [
                "project",
                "init",
                "--url",
                BASE_URL,
                "--username",
                "admin",
                "--password",
                "secret",
                "--json",
            ],
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["status"] == "ok"

    @resp_lib.activate
    def test_project_init_with_username_password_failure(self):
        resp_lib.add(
            resp_lib.POST,
            f"{BASE_URL}/api/token/",
            json={"non_field_errors": ["bad creds"]},
            status=400,
        )
        runner = self._runner()
        result = runner.invoke(
            main,
            [
                "project",
                "init",
                "--url",
                BASE_URL,
                "--username",
                "admin",
                "--password",
                "wrong",
                "--json",
            ],
        )
        assert result.exit_code != 0
        assert "Failed to obtain token" in result.output

    def test_export_bulk_zip_honors_original_flag(self, tmp_path):
        save_config(BASE_URL, "tok")
        runner = self._runner()

        with patch("paperless_ngx.core.export.bulk_download_zip") as mock_zip:
            mock_zip.return_value = str(tmp_path / "paperless-export.zip")
            result = runner.invoke(
                main,
                [
                    "export",
                    "bulk",
                    "1",
                    "2",
                    "--output-dir",
                    str(tmp_path),
                    "--zip",
                    "--original",
                    "--json",
                ],
            )

        assert result.exit_code == 0
        mock_zip.assert_called_once_with(
            ANY,
            [1, 2],
            str(tmp_path / "paperless-export.zip"),
            content="originals",
        )

    # ── export command ──

    @resp_lib.activate
    def test_export_bulk_json(self, tmp_path):
        save_config(BASE_URL, "tok")
        for doc_id in [1, 2]:
            resp_lib.add(
                resp_lib.GET,
                f"{BASE_URL}/api/documents/{doc_id}/download/",
                body=b"PDF",
                headers={
                    "Content-Disposition": f'attachment; filename="d{doc_id}.pdf"'
                },
                status=200,
            )
        runner = self._runner()
        result = runner.invoke(
            main,
            ["export", "bulk", "1", "2", "--output-dir", str(tmp_path), "--json"],
        )
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["downloaded"] == 2
        assert data["errors"] == 0

    # ── global --json flag propagates ──

    @resp_lib.activate
    def test_global_json_flag_with_document_list(self):
        save_config(BASE_URL, "tok")
        resp_lib.add(
            resp_lib.GET,
            f"{BASE_URL}/api/documents/",
            json={"count": 0, "results": []},
            status=200,
        )
        runner = self._runner()
        result = runner.invoke(main, ["--json", "document", "list"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["count"] == 0


class TestPrettyPrint:
    def _skin(self):
        skin = MagicMock()
        skin.info = MagicMock()
        skin.table = MagicMock()
        skin.status = MagicMock()
        return skin

    def test_pretty_print_empty_list_calls_info(self):
        skin = self._skin()
        _pretty_print([], skin)
        skin.info.assert_called_once_with("No results.")

    def test_pretty_print_list_renders_table(self):
        skin = self._skin()
        _pretty_print([{"id": 1, "title": "Doc"}], skin)
        skin.table.assert_called_once()

    def test_pretty_print_paginated_dict_prints_count_and_results(self):
        skin = self._skin()
        _pretty_print({"count": 2, "results": [{"id": 1}, {"id": 2}]}, skin)
        skin.info.assert_called_once_with("Total: 2 results")
        skin.table.assert_called_once()

    def test_pretty_print_scalar_dict_uses_status_lines(self):
        skin = self._skin()
        _pretty_print({"id": 1, "title": "Doc"}, skin)
        assert skin.status.call_count == 2


class TestReplMetadata:
    def test_repl_help_covers_new_discoverability_commands(self):
        assert "document preview <id>" in _REPL_HELP
        assert "document thumb <id>" in _REPL_HELP
        assert "search query <text>" in _REPL_HELP
        assert "search autocomplete <term>" in _REPL_HELP
        assert "tag get <id>" in _REPL_HELP
        assert "correspondent get <id>" in _REPL_HELP
        assert "doctype get <id>" in _REPL_HELP
        assert "task list" in _REPL_HELP
        assert "task get <id>" in _REPL_HELP
