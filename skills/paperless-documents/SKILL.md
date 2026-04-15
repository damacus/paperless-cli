---
name: paperless-documents
description: Search, inspect, edit, upload, download, and organize Paperless-ngx documents with the local `paperless` CLI.
---

# Paperless Documents

Use this skill for document-centric Paperless tasks.

## Common flows

### Find documents

```bash
paperless document list --query "invoice"
paperless document search "github receipt"
paperless --output json document list --tag TODO
```

### Read document text

```bash
paperless document content 303
paperless --output json document get 303
```

### Change metadata

```bash
paperless document edit 303 --title "Updated title"
paperless document edit 303 --add-tag important --remove-tag TODO
paperless document update 303 --tag-id 59 --tag-id 60
```

### Move files in and out

```bash
paperless document upload ./invoice.pdf --title "April invoice"
paperless document download 303 --output-dir ./downloads
paperless document preview 303 --output-dir ./downloads
paperless document thumb 303 --output-dir ./downloads
paperless document delete 303
```

## Notes

- `document edit` is the ergonomic command for incremental tag changes.
- `document update` is the low-level ID-based command when you already know target IDs.
- Markdown mode is the default and is usually the best output for human review.
