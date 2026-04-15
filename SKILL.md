---
name: paperless-cli
description: Use the local `paperless` Rust CLI to search, inspect, upload, download, edit, and manage documents in Paperless-ngx, plus read local PDFs and configure access.
---

# paperless-cli

Use this repo's `paperless` binary for Paperless-ngx workflows when the user needs direct document operations or local PDF inspection.

## Use when

- Searching or listing Paperless documents
- Reading extracted document text
- Uploading, downloading, previewing, or deleting documents
- Editing document metadata or tags
- Managing tags, correspondents, or document types
- Reading a local PDF's text or metadata
- Configuring Paperless access for this CLI

## Core commands

```bash
paperless login --url https://paperless.example.com --token YOUR_TOKEN
paperless document list --query "invoice"
paperless document content 303
paperless document edit 303 --add-tag important --remove-tag TODO
paperless tag edit 12 --name receipts
paperless pdf read ./document.pdf
paperless pdf info ./document.pdf
```

## Guidance

- Prefer `paperless document content <id>` when the user wants OCR or extracted text only.
- Prefer `paperless --output json ...` for automation or when structured output is easier to consume.
- Use exact tag names with `document edit --add-tag/--remove-tag` when the user does not know numeric IDs.
- Use `PAPERLESS_URL` and `PAPERLESS_TOKEN` for temporary overrides when changing global config is unnecessary.
- Use `paperless` with no subcommand to launch the TUI.

## Skill index

- [Paperless Documents](skills/paperless-documents/SKILL.md)
- [Paperless PDF](skills/paperless-pdf/SKILL.md)
- [Paperless Admin](skills/paperless-admin/SKILL.md)
