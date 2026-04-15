# Testing Strategy

Author: Charles-Dickens

## Approach

The migration started by encoding the new Rust contracts in tests and then
filling in the implementation around them. The goal was not a line-for-line
translation of the Python suite. The goal was to preserve behavior while
shifting the architecture to a Rust TUI and agent-friendly outputs.

## Test layers

### Config and client

`tests/config_and_client.rs` covers:

- config/session round-trips
- token redaction
- document query parameter construction
- HTTP error mapping
- pagination through `next` URLs
- multipart upload request construction
- filename sanitization on download

### Output and TUI

`tests/output_tui_security.rs` covers:

- markdown output shape
- JSON envelope shape
- core TUI rendering on a `TestBackend`
- security reviewer polling behavior and the `gpt-5.4` model requirement

### CLI behavior

`tests/cli_integration.rs` covers:

- top-level help surface
- missing-config failure for document commands
- graceful status behavior when nothing is configured
- `config set-url` and `config set-token`
- `document content`
- `pdf read` and `pdf info`

### Compatibility coverage

- `document content` returning only extracted text in the happy path
- document metadata updates that add and remove tags without disturbing other
  fields
- `tag edit` request shaping and validation
- `pdf read` and `pdf info` reading local files without mutating them
- config subcommands for `set-url` and `set-token`
- environment-variable overrides for `PAPERLESS_URL` and `PAPERLESS_TOKEN`

## Why coverage is high without full API snapshots

The transport abstraction gives broad behavioral coverage over the risky parts:

- request shaping
- error handling
- persistence
- output contracts
- security reporting

This catches most regression-prone code without needing a live Paperless test
cluster for every run.

## Reader test checklist

If another engineer picks up the repo cold, they should be able to answer the
following from the docs and tests:

1. How do I configure the app?
2. How do I get markdown or JSON instead of the TUI?
3. Where are security findings produced and surfaced?
4. How do I add a new Paperless endpoint safely?
5. Which tests should I update when I change output format or request shaping?

If any of those questions are hard to answer, the migration docs need another
pass.
