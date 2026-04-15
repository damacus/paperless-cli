# CLI Parity Notes

This document tracks the compatibility-oriented commands and flags that are
implemented to feel familiar to users coming from `julianfbeck/paperless-cli`.

## Implemented commands

- `document content`
- document metadata updates that can add and remove tags cleanly
- `tag edit`
- `pdf read`
- `pdf info`
- `config set-url`
- `config set-token`
- `PAPERLESS_URL` and `PAPERLESS_TOKEN` compatibility
- global `--json`, `-q/--quiet`, `--no-color`, and `-u/--url`

## Current behavior

The Rust implementation keeps these commands small and direct:

- `document content` should return the extracted text representation of a
  document without extra wrapper noise
- document metadata commands should preserve unrelated fields while changing
  only the requested tags
- `tag edit` should update existing tag metadata instead of forcing delete and
  recreate
- `pdf read` and `pdf info` should operate on local files and avoid mutating
  them
- `config set-url` and `config set-token` should map to durable config writes
- environment variables should be accepted as compatibility inputs before
  falling back to persisted config

## Testing notes

These commands now have narrow, behavior-focused tests that verify:

- request construction
- text or JSON output shape
- non-destructive metadata updates
- environment-variable precedence
