# paperless-ngx-cli

A command-line client and REPL for
[Paperless-ngx](https://github.com/paperless-ngx/paperless-ngx).

This project intentionally supports a curated subset of the Paperless REST API.
It focuses on common document workflows, lightweight taxonomy management, and
script-friendly JSON output. It does not aim for full Paperless API parity.

## Installation

```bash
pip install paperless-ngx-cli
```

## Quick Start

Configure the server connection with an existing token:

```bash
paperless project init --url http://localhost:8000 --token YOUR_TOKEN
```

Or exchange username/password for a token and save it locally:

```bash
paperless project init --url http://localhost:8000 --username admin --password secret
```

Verify connectivity:

```bash
paperless project ping
paperless project info
paperless status
```

Common commands:

```bash
paperless document list --query "invoice 2024"
paperless document list --tag-id 7 --created-after 2024-01-01 --order-by title
paperless document get 42
paperless document upload ~/Downloads/invoice.pdf --title "Invoice Q1"
paperless document download 42 --output-dir ~/Downloads
paperless document preview 42 --output-dir ~/Downloads
paperless search query "invoice acme"
paperless search autocomplete inv
paperless task list
paperless tag get 7
paperless --json document list
paperless repl
```

## Supported Commands

| Group | Commands |
| --- | --- |
| `project` | `init`, `info`, `ping` |
| `document` | `list`, `get`, `upload`, `download`, `preview`, `thumb`, `update`, `delete`, `search` |
| `search` | `query`, `autocomplete` |
| `tag` | `list`, `get`, `create`, `delete` |
| `correspondent` | `list`, `get`, `create`, `delete` |
| `doctype` | `list`, `get`, `create`, `delete` |
| `task` | `list`, `get` |
| `export` | `bulk` |
| top-level | `status`, `repl` |

## Capabilities And Limits

Implemented Paperless endpoint coverage:

| CLI command | Paperless endpoint |
| --- | --- |
| `project ping` | `GET /api/status/` |
| `project info` | `GET /api/statistics/` |
| `document list`, `document search` | `GET /api/documents/` |
| `search query` | `GET /api/search/` |
| `search autocomplete` | `GET /api/search/autocomplete/` |
| `document get` | `GET /api/documents/<id>/` |
| `document upload` | `POST /api/documents/post_document/` |
| `document download` | `GET /api/documents/<id>/download/` |
| `document preview` | `GET /api/documents/<id>/preview/` |
| `document thumb` | `GET /api/documents/<id>/thumb/` |
| `document update` | `PATCH /api/documents/<id>/` |
| `document delete` | `DELETE /api/documents/<id>/` |
| `task *` | `/api/tasks/` |
| `tag *` | `/api/tags/` |
| `correspondent *` | `/api/correspondents/` |
| `doctype *` | `/api/document_types/` |
| `export bulk --zip` | `POST /api/documents/bulk_download/` |

Not implemented yet:

- `documents/bulk_edit`
- `documents/reprocess`
- saved views, storage paths, users, groups
- mail accounts, mail rules, custom fields, config

## Search Modes

There are now two search entry points:

- `paperless document search`
  Searches only documents via `/api/documents/` and supports document filters
  like tag IDs, correspondent IDs, date ranges, and ordering.
- `paperless search query`
  Uses Paperless global search via `/api/search/` and can return matches across
  indexed resource types.
- `paperless search autocomplete`
  Returns term suggestions from `/api/search/autocomplete/` for partial input.

Useful document filter examples:

```bash
paperless document list --tag urgent --order-by -created
paperless document list --tag-id 4 --correspondent-id 2 --created-before 2024-12-31
paperless document search "contract" --type-id 3 --order-by title
```

## REPL And Session Behavior

Running `paperless` without a subcommand enters the REPL.

The REPL stores lightweight session state in a temporary file:

- `last_query`: most recent document search/list query
- `selected_docs`: reserved for future selection-aware workflows
- `history`: recent commands entered during the session

Configuration is stored at `~/.config/paperless-cli/config.json`.
Session state is stored at `/tmp/paperless-cli-session.json`.

Every command also supports `--json` for scripting. JSON mode prints raw
response structures instead of the human-oriented table/status output used in
the REPL and standard CLI mode.

## Development

Run the test suite from the project virtualenv:

```bash
./.venv/bin/pytest
```

Useful local checks:

```bash
./.venv/bin/ruff check .
./.venv/bin/mypy paperless_ngx
./.venv/bin/pylint paperless_ngx
```

The GitHub Actions pipeline runs:

- `pytest`
- `ruff check`
- `ruff format --check`
- `mypy`
- `pylint`
- `bandit`
- `python -m compileall`
- `python -m build`
- `twine check`

## Releases

Releases are managed with `googleapis/release-please-action` from
`.github/workflows/release-please.yml`. When a release PR is merged, the
workflow builds the package and publishes it to PyPI using trusted publishing.

### PyPI Pending Publisher

Create a pending publisher for project `paperless-ngx-cli` at
[PyPI publishing settings](https://pypi.org/manage/account/publishing/) with:

- Owner: `damacus`
- Repository name: `paperless-cli`
- Workflow filename: `release-please.yml`
- Environment name: `pypi`

After the pending publisher is saved, releases from the `main` branch can
publish without a PyPI API token.
