# paperless-ngx-cli

A command-line client and REPL for [Paperless-ngx](https://github.com/paperless-ngx/paperless-ngx).

## Installation

```bash
pip install paperless-ngx-cli
```

## Quick Start

Configure the server connection:

```bash
paperless project init --url http://localhost:8000 --token YOUR_TOKEN
```

Or exchange username/password for an API token:

```bash
paperless project init --url http://localhost:8000 --username admin --password secret
```

Verify connectivity:

```bash
paperless project ping
```

Common commands:

```bash
paperless document list
paperless document search "invoice 2024"
paperless document upload ~/Downloads/invoice.pdf --title "Invoice Q1"
paperless document download 42 --output-dir ~/Downloads
paperless --json document list
paperless repl
```

## Commands

| Group | Commands |
| --- | --- |
| `project` | `init`, `info`, `ping` |
| `document` | `list`, `get`, `upload`, `download`, `update`, `delete`, `search` |
| `tag` | `list`, `create`, `delete` |
| `correspondent` | `list`, `create`, `delete` |
| `doctype` | `list`, `create`, `delete` |
| `export` | `bulk` |
| top-level | `status`, `repl` |

## Development

Run the test suite:

```bash
python3 -m pytest
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

Releases are managed with `googleapis/release-please-action` from `.github/workflows/release-please.yml`.
When a release PR is merged, the workflow builds the package and publishes it to PyPI using trusted publishing.

### PyPI Pending Publisher

Create a pending publisher for project `paperless-ngx-cli` at [PyPI publishing settings](https://pypi.org/manage/account/publishing/) with:

- Owner: `damacus`
- Repository name: `paperless-cli`
- Workflow filename: `release-please.yml`
- Environment name: `pypi`

After the pending publisher is saved, releases from the `main` branch can publish without a PyPI API token.
