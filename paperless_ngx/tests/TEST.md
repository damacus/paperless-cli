# TEST PLAN — paperless-ngx-cli

## Overview

The test suite has three layers:

1. `test_core.py`
Unit tests and CLI-layer tests using mocked HTTP responses. These are the main
regression suite and cover the supported command surface.

2. `test_full_e2e.py::TestE2EWorkflow`
Optional live-server tests against a real Paperless-ngx instance. These are
skipped unless `PAPERLESS_URL` and `PAPERLESS_TOKEN` are set.

3. `test_full_e2e.py::TestCLISubprocess`
Subprocess tests for the installed `paperless` entry point. Basic help/version
and missing-config behavior always run; server-backed cases are skipped without
live credentials.

`conftest.py` redirects config and session file paths to `tmp_path`, so tests do
not read or write a developer's real `~/.config/paperless-cli/config.json` or
`/tmp/paperless-cli-session.json`.

## Current Focus Areas

The mocked suite covers:

- backend config, auth, pagination, error handling
- document list/get/upload/download/preview/thumb/update/delete/search
- tag, correspondent, and doctype list/get/create/delete
- project init/info/ping, including username/password token exchange
- export bulk behavior and ZIP-path selection
- CLI help output and root `--json` propagation
- `_pretty_print` output behavior for empty lists, tables, and paginated payloads
- REPL command metadata and session persistence primitives

The optional live-server suite covers:

- connectivity checks
- document listing/search structure
- upload/delete lifecycle
- tag, correspondent, and doctype lifecycle
- subprocess JSON output for the installed CLI

## How To Run

Run the full local suite:

```bash
./.venv/bin/pytest
```

Run only the mocked fast suite:

```bash
./.venv/bin/pytest paperless_ngx/tests/test_core.py
```

Run live-server tests when credentials are available:

```bash
PAPERLESS_URL=http://localhost:8000 \
PAPERLESS_TOKEN=... \
./.venv/bin/pytest paperless_ngx/tests/test_full_e2e.py
```

## Notes

- Live-server tests are intentionally optional to keep CI and local iteration
  fast and stable.
- `API_ANALYSIS.md` is a reference for future expansion. The tests only assert
  behavior for the command surface currently exposed by the CLI.
