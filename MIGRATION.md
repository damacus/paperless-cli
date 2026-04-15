# Rust Migration Notes

## Summary

This repository has been migrated from a Python Click CLI and REPL into a Rust application centered on:

- a blocking HTTP client abstraction
- service-layer functions that preserve the Paperless API contract
- Ratatui for interactive workflows
- Markdown and JSON output for automation and LLM usage

## Design decisions

### 1. Tests first

The migration was driven by Rust tests that preserved the important behavior of the original tool:

- config/session persistence
- descriptive API error handling
- document query filter encoding
- multipart upload and binary download flows
- structured output
- non-fatal `status` behavior without config

### 2. Preserve the transport contract

The old Python surface was command-heavy, but the core contract lived in:

- authenticated HTTP requests
- endpoint-specific payload shapes
- safe local file writes
- consistent human and machine output

The Rust version keeps that split with:

- `src/api.rs`
- `src/services.rs`
- `src/render.rs`
- `src/security.rs`
- `src/tui.rs`

### 3. LLM-friendly by default

Non-interactive commands default to Markdown output instead of plain terminal formatting.

Why:

- easier to paste into LLM sessions
- stable tables for document lists
- clean machine fallback through `--output json`

### 4. Security folded into the runtime

The interactive app includes a polling security reviewer profile and exposes findings in the TUI. The implementation also hardens local behavior through:

- path sanitization for downloads
- permission-restricted config/session files
- token redaction

## Important differences from the old Python CLI

- the primary interactive experience is now the Rust TUI, not a Python REPL
- output modes are explicitly `markdown`, `json`, or `tui`
- the codebase is organized around Rust services and rendering, not Click command handlers

## Compatibility layer

The Rust CLI now includes the main compatibility-oriented commands from the
older workflow so familiar usage patterns still work:

- `document content`
- richer document edit flows for adding and removing tags
- `tag edit`
- `pdf read` and `pdf info`
- `config set-url` and `config set-token`
- `PAPERLESS_URL` and `PAPERLESS_TOKEN` compatibility

For the current command notes and testing coverage, see
[docs/cli-parity.md](docs/cli-parity.md).

## Verification

Current verification command:

```bash
cargo test
```
