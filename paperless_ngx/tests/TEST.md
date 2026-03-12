# TEST PLAN — cli-anything-paperless-ngx

## Overview

Tests are divided into three tiers:

1. **Unit tests** (`test_core.py`) — all HTTP mocked via `responses`, no real server needed
2. **E2E tests** (`test_full_e2e.py::TestE2EWorkflow`) — hit a real Paperless-ngx server
3. **CLI subprocess tests** (`test_full_e2e.py::TestCLISubprocess`) — invoke the installed entry point as a subprocess

Config and session files on the developer's machine are never read or written
during tests — `conftest.py` redirects both paths to `tmp_path` via autouse fixture.

---

## Unit Tests (`test_core.py`)

### TestPaperlessConfig
- [x] `test_api_url_no_trailing_slash_on_base` — trailing slash on base URL stripped before path concat
- [x] `test_api_url_leading_slash_stripped_from_path` — leading slash on path stripped before concat
- [x] `test_to_dict_roundtrip` — from_dict(to_dict()) round-trips losslessly
- [x] `test_url_trailing_slash_stripped` — multiple trailing slashes all stripped

### TestFindPaperlessServer
- [x] `test_missing_config_raises` — no config file → RuntimeError mentioning `project init`
- [x] `test_malformed_config_raises` — invalid JSON → RuntimeError mentioning "malformed"
- [x] `test_missing_url_field_raises` — JSON missing url field → RuntimeError
- [x] `test_missing_token_field_raises` — JSON missing token field → RuntimeError
- [x] `test_valid_config_loads` — valid config → PaperlessConfig with correct fields

### TestSaveConfig
- [x] `test_creates_directory_and_file` — creates parent dirs and writes JSON
- [x] `test_overwrites_existing` — subsequent save overwrites previous value

### TestSessionPersistence
- [x] `test_save_and_load_session` — round-trips session state to disk
- [x] `test_load_session_defaults_when_missing` — missing file → default empty state
- [x] `test_load_session_defaults_on_corrupt_file` — corrupt file → default empty state

### TestPaperlessBackend
- [x] `test_get_success` — GET 200 returns parsed JSON
- [x] `test_get_sends_auth_header` — Authorization: Token header is set
- [x] `test_get_401_raises` — 401 → RuntimeError about authentication
- [x] `test_get_403_raises` — 403 → RuntimeError about permissions
- [x] `test_get_404_raises` — 404 → RuntimeError about not found
- [x] `test_get_500_raises` — 500 → RuntimeError with status code in message
- [x] `test_post_json` — POST with JSON body returns parsed JSON
- [x] `test_post_multipart` — POST with files= does multipart upload
- [x] `test_post_204_returns_empty_dict` — 204 No Content → empty dict
- [x] `test_patch` — PATCH returns updated resource
- [x] `test_put` — PUT returns replaced resource
- [x] `test_delete` — DELETE 204 does not raise
- [x] `test_paginate_single_page` — single DRF page returns all results
- [x] `test_paginate_multiple_pages` — follows `next` URL across pages
- [x] `test_paginate_plain_list_response` — plain list (non-paginated) handled
- [x] `test_ping_success` — ping returns {status: ok, url, response_code}
- [x] `test_ping_connection_error` — ConnectionError → RuntimeError about "Cannot connect"
- [x] `test_ping_timeout` — Timeout → RuntimeError about "timed out"
- [x] `test_get_token_success` — POST /api/token/ → returns token string
- [x] `test_get_token_failure` — bad credentials → RuntimeError

### TestDocuments
- [x] `test_list_no_filter` — list without params returns count/results
- [x] `test_list_with_query` — query param included in request URL
- [x] `test_list_with_tag_filter` — tags__name__icontains param set
- [x] `test_list_with_correspondent_filter` — correspondent__name__icontains param set
- [x] `test_list_with_doc_type_filter` — document_type__name__icontains param set
- [x] `test_list_page_size_respected` — page_size and page params sent
- [x] `test_get_document` — GET /documents/{id}/ returns document dict
- [x] `test_upload_document` — multipart POST to post_document/
- [x] `test_upload_with_tags_and_metadata` — title/correspondent/type/tags all sent
- [x] `test_upload_missing_file_raises` — FileNotFoundError for non-existent path
- [x] `test_download_document_with_content_disposition` — filename from Content-Disposition
- [x] `test_download_document_fallback_filename` — fallback filename uses doc id
- [x] `test_download_original_passes_param` — original=true passed to API
- [x] `test_update_document_title` — PATCH with title field
- [x] `test_update_document_tags` — PATCH with tags field
- [x] `test_update_no_fields_raises` — ValueError when no kwargs given
- [x] `test_delete_document` — DELETE 204 does not raise
- [x] `test_search_documents` — query param sent, returns count/results

### TestTags
- [x] `test_list_tags` — paginated list returns all items
- [x] `test_create_tag_defaults` — POST with default color
- [x] `test_create_tag_custom_color` — custom color in POST body
- [x] `test_create_inbox_tag` — is_inbox_tag: true in POST body
- [x] `test_delete_tag` — DELETE 204 does not raise
- [x] `test_get_tag` — GET /tags/{id}/

### TestCorrespondents
- [x] `test_list_correspondents` — paginated list
- [x] `test_create_correspondent` — POST body has name
- [x] `test_create_correspondent_with_match` — match pattern in POST body
- [x] `test_delete_correspondent` — DELETE 204 does not raise
- [x] `test_get_correspondent` — GET /correspondents/{id}/

### TestDocTypes
- [x] `test_list_doc_types` — paginated list
- [x] `test_create_doc_type` — POST with name
- [x] `test_delete_doc_type` — DELETE 204 does not raise
- [x] `test_get_doc_type` — GET /document_types/{id}/

### TestSession
- [x] `test_last_query_set_and_get` — setter/getter round-trip
- [x] `test_selected_docs_set_and_get` — list setter/getter
- [x] `test_add_history_appends` — history grows in order
- [x] `test_history_capped_at_500` — oldest entries dropped after 500
- [x] `test_clear_selection` — resets selected_docs to []
- [x] `test_to_dict` — returns dict with all state keys
- [x] `test_persist_is_called_on_mutation` — save_session called on write

### TestProjectCore
- [x] `test_init_connection_with_token` — pings, saves config, returns config
- [x] `test_init_connection_with_credentials` — acquires token then pings
- [x] `test_init_connection_no_auth_raises` — ValueError if neither token nor creds
- [x] `test_get_connection_info` — returns url, masked token, statistics
- [x] `test_ping_server` — returns {status: ok, elapsed_ms}
- [x] `test_ping_server_no_config_raises` — RuntimeError when not configured

### TestExportCore
- [x] `test_bulk_download_all_ok` — all docs downloaded, all status=ok
- [x] `test_bulk_download_partial_failure` — failed doc gets status=error with error message
- [x] `test_bulk_download_progress_callback` — callback called during download

### TestGuessMime
- [x] `test_pdf` — .pdf → application/pdf
- [x] `test_jpg` / `test_jpeg` — image/jpeg
- [x] `test_png` — image/png
- [x] `test_tiff` / `test_tif` — image/tiff
- [x] `test_txt` — text/plain
- [x] `test_odt` — application/vnd.oasis.opendocument.text
- [x] `test_unknown` — application/octet-stream
- [x] `test_uppercase_extension` — .PDF lowercased before lookup

### TestCLILayer (Click CliRunner — no subprocess)
- [x] `test_version` — --version outputs 1.0.0
- [x] `test_help_lists_groups` — all command groups in --help
- [x] `test_document_help` — all document subcommands listed
- [x] `test_tag_help` — all tag subcommands listed
- [x] `test_status_no_config_shows_not_connected` — graceful output when unconfigured
- [x] `test_status_json_no_config` — JSON with connected=false
- [x] `test_document_list_no_config_fails` — non-zero exit when no config
- [x] `test_document_list_no_config_json_flag` — non-zero exit with --json too
- [x] `test_document_list_json_output` — {count, results} JSON structure
- [x] `test_document_list_with_query` — query param in HTTP request
- [x] `test_document_get_json` — JSON with id field
- [x] `test_document_search_json` — count/results structure
- [x] `test_document_delete_with_yes_flag` — {status: deleted, doc_id}
- [x] `test_document_update_json` — patched title in response
- [x] `test_document_upload_json` — task_id in response
- [x] `test_document_download_json` — {status: ok, doc_id, path}
- [x] `test_tag_list_json` — JSON list of tags
- [x] `test_tag_create_json` — {id, name} response
- [x] `test_tag_delete_with_yes_flag` — {status: deleted}
- [x] `test_correspondent_list_json` — JSON list
- [x] `test_correspondent_create_json` — {id, name}
- [x] `test_correspondent_delete_json` — {status: deleted}
- [x] `test_doctype_list_json` — JSON list
- [x] `test_doctype_create_json` — {id, name}
- [x] `test_doctype_delete_json` — {status: deleted}
- [x] `test_project_ping_json` — {status: ok}
- [x] `test_project_info_json` — {url, statistics}
- [x] `test_project_init_with_token` — {status: ok}
- [x] `test_export_bulk_json` — {downloaded, errors, results}
- [x] `test_global_json_flag_with_document_list` — --json at root propagates

---

## E2E Tests (`test_full_e2e.py`)

**Requirements:**
- `PAPERLESS_URL` must be set to a running Paperless-ngx server URL
- `PAPERLESS_TOKEN` must be set to a valid API token
- Tests are **SKIPPED** (not failed) when these are absent

### TestE2EWorkflow (requires real server — Python module level)
- [ ] `test_ping` — server responds within 10s
- [ ] `test_list_documents_returns_paginated_structure` — count/results shape
- [ ] `test_list_documents_ordering` — reverse-created order when ≥2 docs
- [ ] `test_upload_and_delete_full_cycle` — upload PDF → wait → delete → 404
- [ ] `test_search_returns_valid_structure` — count/results shape
- [ ] `test_list_tags_returns_list` — returns Python list
- [ ] `test_tag_lifecycle` — create → get → delete → 404
- [ ] `test_correspondent_lifecycle` — create → get → delete → 404
- [ ] `test_doctype_lifecycle` — create → get → delete → 404

### TestCLISubprocess (subprocess tests requiring server)
- [x] `test_cli_version` — ALWAYS RUNS — exits 0, outputs 1.0.0
- [x] `test_cli_help` — ALWAYS RUNS — all groups in output
- [x] `test_document_help` — ALWAYS RUNS — all subcommands listed
- [x] `test_tag_help` — ALWAYS RUNS — all subcommands listed
- [x] `test_project_help` — ALWAYS RUNS — init/info/ping listed
- [x] `test_no_config_document_list_exits_nonzero` — ALWAYS RUNS — non-zero exit
- [x] `test_no_config_shows_project_init_instruction` — ALWAYS RUNS — message contains "project init"
- [x] `test_no_config_tag_list_exits_nonzero` — ALWAYS RUNS — non-zero exit
- [x] `test_status_no_config_exits_zero` — ALWAYS RUNS — status exits 0
- [x] `test_status_json_no_config` — ALWAYS RUNS — {connected: false}
- [ ] `test_status_json_connected` — requires server — {connected: true}
- [ ] `test_document_list_json_returns_paginated` — requires server
- [ ] `test_document_search_json` — requires server
- [ ] `test_tag_list_json_returns_list` — requires server
- [ ] `test_correspondent_list_json_returns_list` — requires server
- [ ] `test_doctype_list_json_returns_list` — requires server
- [ ] `test_global_json_flag` — requires server
- [ ] `test_project_ping_json` — requires server

---

## Test Results

### Run 1 — 2026-03-12 (initial build, 49 tests)

**Environment:** Python 3.14.3, pytest 9.0.2, darwin, no server

```
49 passed in 1.06s
```

---

### Run 2 — 2026-03-12 (complete harness, 150 tests)

**Environment:** Python 3.14.3, pytest 9.0.2, darwin, no server

```
133 passed, 17 skipped in 1.24s
```

**Passed (133):**

- TestPaperlessConfig: 4/4
- TestFindPaperlessServer: 5/5
- TestSaveConfig: 2/2
- TestSessionPersistence: 3/3
- TestPaperlessBackend: 20/20
- TestDocuments: 19/19
- TestTags: 6/6
- TestCorrespondents: 5/5
- TestDocTypes: 4/4
- TestSession: 7/7
- TestProjectCore: 6/6
- TestExportCore: 3/3
- TestGuessMime: 10/10
- TestCLILayer: 34/34 (Click CliRunner tests)
- TestCLISubprocess (no-server subset): 10/10

**Skipped (17):** All tests gated by `SKIP_E2E` (require live Paperless-ngx server):
- TestE2EWorkflow: 9 tests
- TestCLISubprocess server tests: 8 tests

To run the full suite including E2E:
```bash
export PAPERLESS_URL=http://localhost:8000
export PAPERLESS_TOKEN=your-api-token
cd /Users/damacus/repos/damacus/paperless-cli/agent-harness
pytest cli_anything/paperless_ngx/tests/ -v
```
