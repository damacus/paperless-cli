# Architecture Overview

Author: Charles-Dickens

## System shape

The Rust application follows the orchestrated-state pattern described by the
TUI workflow guidance:

1. `config`: durable configuration, session persistence, output mode selection
2. `api`: transport abstraction, request construction, error mapping
3. `services`: Paperless workflows expressed as domain operations
4. `security`: continuously polling reviewer that feeds findings to the main app
5. `tui`: Ratatui layout that renders state rather than owning business logic
6. `render`: markdown and JSON emitters for non-interactive use

## Why this split

### Testability

`api::Transport` lets the service layer run entirely against mocked request and
response data. That means contract tests can focus on behavior without a live
Paperless server.

### Security

Sensitive values are concentrated in `AppConfig`, which redacts tokens in
`Debug` output. Config writes enforce restricted permissions on Unix, and
filename sanitization lives in the service layer where downloads are actually
materialized.

### TUI discipline

The TUI only renders a `DashboardSnapshot`. Networking, persistence, and
security review stay out of the drawing code. That keeps the interactive layer
small and makes snapshot-style testing viable.

## Output modes

### TUI

The default mode. It shows:

- document list
- selected document inspector
- latest task summary
- active security findings
- persistent hotkey footer

### Markdown

The default non-interactive mode. It is optimized for direct consumption:

- document text commands return plain extracted text
- list and object responses render as compact tables or bullet summaries
- security output appears only when there are active findings

This keeps the output stable enough for humans, GitHub comments, and LLM prompts
without forcing extra wrapper noise around common commands.

### Structured JSON

JSON mode emits an `OutputEnvelope` with:

- `mode`
- `command`
- `data`
- `security`

This keeps machine consumers from having to infer whether security findings were
present or silently omitted.

### Compatibility extensions

The architecture keeps a few compatibility-focused commands close to the
service/config layers:

- `document content` should remain a thin service call over the existing
  document retrieval path
- document tag add/remove flows should be expressed as metadata updates in the
  service layer, not ad hoc TUI logic
- `tag edit`, `config set-url`, `config set-token`, and `pdf read/info` stay as
  narrow adapters over shared transport and file-handling helpers
- environment-variable compatibility should be resolved in `config` before any
  network or render work begins

Keeping these concerns in the service/config layers avoids a second CLI
architecture growing around them later.

## Security reviewer agent

The background reviewer is intentionally modeled as a first-class subsystem:

- profile name: `security-reviewer`
- model: `gpt-5.4`
- cadence: periodic polling over shared audit state
- transport to main app: message passing through a channel

Current rules focus on:

- remote plain-HTTP usage
- overly broad config permissions
- suspicious download paths
- accidental model downgrade away from `gpt-5.4`

The point is not to pretend that static heuristics are an LLM. The point is to
reserve a real place in the architecture for continuous security review and to
make its outputs visible to operators.
