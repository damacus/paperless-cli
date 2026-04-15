---
name: paperless-admin
description: Configure the Paperless CLI and manage tags, correspondents, and document types with the local `paperless` binary.
---

# Paperless Admin

Use this skill for setup and catalog-management tasks.

## Setup

```bash
paperless login --url https://paperless.example.com --token YOUR_TOKEN
paperless config set-url https://paperless.example.com
paperless config set-token YOUR_TOKEN
```

## Temporary overrides

```bash
PAPERLESS_URL=https://paperless.example.com PAPERLESS_TOKEN=secret paperless status
paperless -u https://paperless.example.com status
```

## Tags

```bash
paperless tag list
paperless tag create TODO --color '#ffcc00'
paperless tag edit 12 --name receipts --color '#336699'
paperless tag delete 12
```

## Other catalog objects

```bash
paperless correspondent list
paperless correspondent create "GitHub" ""
paperless doctype list
paperless doctype create "Invoice" ""
```

## Notes

- Use `PAPERLESS_URL` and `PAPERLESS_TOKEN` when you need compatibility with external tooling or ephemeral shells.
- Use `tag edit` rather than delete-and-recreate when preserving existing references matters.
