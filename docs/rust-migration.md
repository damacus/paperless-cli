# Rust Migration Notes

Author: Charles-Dickens

## Goal

This repository has been migrated from a Python CLI and REPL to a Rust-first
application with a Ratatui interface and explicit non-interactive output modes.
The migration is intentionally one-shot at the runtime layer: the Rust binary
is the default entrypoint, the CI pipeline is Rust-first, and the release
configuration now targets Rust artifacts instead of Python packages.

## What changed

### Runtime

- `paperless` with no subcommand now launches the TUI.
- Non-interactive commands emit either markdown or structured JSON through
  `--output markdown` or `--output json`.
- The transport layer is now a blocking `reqwest` client behind a testable
  transport abstraction instead of direct `requests` calls.

### Persistence

- Config moved from JSON to TOML for the Rust runtime.
- Legacy JSON config is still accepted during read so existing users can move
  without a hard cutover.
- Session state is persisted in TOML and kept intentionally small:
  `last_query`, `selected_docs`, and `history`.

### UX

- The Python REPL has been replaced by a TUI shell with a document list,
  inspector, project status, and security summary.
- Markdown output is designed to be pasted directly into another LLM or issue
  without extra cleanup.

### Security

- Config permissions are restricted on Unix.
- Download filenames are sanitized before writing to disk.
- A polling security reviewer continuously evaluates runtime state and reports
  findings back to the app.

## Old to new command mapping

| Python-era command | Rust-era command |
| --- | --- |
| `paperless repl` | `paperless` |
| `paperless --json document list` | `paperless --output json document list` |
| `paperless document search ...` | `paperless document search ...` |
| `paperless search query ...` | `paperless search query ...` |
| `paperless status` | `paperless status` |

## Intentional compatibility choices

- Singular command groups like `document`, `tag`, `task`, `doctype`, and
  `correspondent` remain available to avoid breaking existing shell history.
- Legacy JSON config files are loaded if present.
- Status degrades gracefully when no config exists so agent workflows can still
  inspect installation state.

## Known residuals

- The legacy Python source tree still exists in the repository as migration
  reference material and should be treated as non-runtime code.
- PyPI packaging files have not been removed yet; they are now historical and
  not part of the active build or release path.
