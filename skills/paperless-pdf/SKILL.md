---
name: paperless-pdf
description: Read local PDF text and inspect local PDF metadata with the `paperless` CLI before upload or troubleshooting.
---

# Paperless PDF

Use this skill for local PDF inspection without talking to a Paperless server.

## Commands

```bash
paperless pdf read ./document.pdf
paperless pdf info ./document.pdf
paperless --output json pdf info ./document.pdf
```

## When to use

- Checking whether a PDF has extractable text
- Reading a PDF locally before upload
- Inspecting page count, filename, size, and embedded metadata
- Verifying a generated PDF in tests or scripts

## Guidance

- `pdf read` is text-first and intended for quick human or agent inspection.
- `pdf info` is metadata-first and best with `--output json` in automation.
- These commands do not mutate the PDF.
