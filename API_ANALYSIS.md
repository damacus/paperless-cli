# Paperless-ngx API Analysis

This document is a Paperless API reference and roadmap input for the CLI. It is
not a statement that every endpoint listed below is implemented in
`paperless-ngx-cli`.

## Source Location

Cloned from: https://github.com/paperless-ngx/paperless-ngx (depth=1)
Analysis date: 2026-03-12

## Architecture Overview

Paperless-ngx is a Django application with a Django REST Framework (DRF) API.
The CLI harness communicates exclusively via HTTP to the REST API.

## Current CLI Coverage

The current CLI intentionally implements a narrow subset of the API centered on:

- connection setup and status
- document listing, retrieval, upload, download, preview, thumbnail, update, delete
- tags, correspondents, and document types
- bulk export/download

Notable endpoints currently analyzed but not exposed by the CLI include:

- `documents/bulk_edit`
- `documents/reprocess`
- `/api/search/` and `/api/search/autocomplete/`
- saved views, storage paths, tasks
- users, groups
- mail accounts, mail rules
- custom fields, config

## API Base URL

All API endpoints are under `/api/`. The API version is negotiated via the
`Accept: application/json; version=7` header.

## Authentication

**Token authentication** (preferred):
```
Authorization: Token <token>
```

Tokens are obtained via:
- `POST /api/token/` with `username` and `password` form fields
- Or via the admin UI at `/api/profile/generate_auth_token/`

## Key Endpoints

### Router-registered ViewSets

| Path | ViewSet | Operations |
|------|---------|------------|
| `/api/correspondents/` | CorrespondentViewSet | list, create, retrieve, update, partial_update, destroy |
| `/api/document_types/` | DocumentTypeViewSet | list, create, retrieve, update, partial_update, destroy |
| `/api/documents/` | UnifiedSearchViewSet | list, create, retrieve, update, partial_update, destroy |
| `/api/tags/` | TagViewSet | list, create, retrieve, update, partial_update, destroy |
| `/api/saved_views/` | SavedViewViewSet | list, create, retrieve, update, partial_update, destroy |
| `/api/storage_paths/` | StoragePathViewSet | list, create, retrieve, update, partial_update, destroy |
| `/api/tasks/` | TasksViewSet | list, retrieve |
| `/api/users/` | UserViewSet | list, create, retrieve, update, destroy |
| `/api/groups/` | GroupViewSet | list, create, retrieve, update, destroy |
| `/api/mail_accounts/` | MailAccountViewSet | CRUD |
| `/api/mail_rules/` | MailRuleViewSet | CRUD |
| `/api/custom_fields/` | CustomFieldViewSet | CRUD |
| `/api/config/` | ApplicationConfigurationViewSet | retrieve, update |

### Special Document Endpoints

| Path | Method | Description |
|------|--------|-------------|
| `/api/documents/post_document/` | POST | Upload a new document (multipart) |
| `/api/documents/bulk_edit/` | POST | Bulk edit (set tags, correspondent, etc.) |
| `/api/documents/delete/` | POST | Bulk delete documents |
| `/api/documents/bulk_download/` | POST | Download multiple docs as ZIP |
| `/api/documents/reprocess/` | POST | Re-run OCR on documents |
| `/api/documents/<id>/download/` | GET | Download processed document |
| `/api/documents/<id>/preview/` | GET | Download preview |
| `/api/documents/<id>/thumb/` | GET | Download thumbnail |

### Search

| Path | Method | Description |
|------|--------|-------------|
| `/api/search/` | GET | Global search (documents + tags + correspondents) |
| `/api/search/autocomplete/` | GET | Search autocomplete suggestions |

Full-text search on `/api/documents/` uses `?query=` parameter.

### System

| Path | Method | Description |
|------|--------|-------------|
| `/api/status/` | GET | System status (health, versions) |
| `/api/statistics/` | GET | Document counts by type/tag |
| `/api/remote_version/` | GET | Available remote version info |

## Data Models

### Document (key fields)

```json
{
  "id": 1,
  "title": "Invoice Q1 2024",
  "content": "OCR text content...",
  "tags": [1, 2, 3],
  "document_type": 1,
  "correspondent": 1,
  "created": "2024-01-15",
  "modified": "2024-01-16T10:30:00Z",
  "added": "2024-01-16T10:30:00Z",
  "archive_serial_number": null,
  "original_file_name": "invoice.pdf",
  "archived_file_name": "2024-01-15 Invoice Q1 2024.pdf",
  "page_count": 2,
  "mime_type": "application/pdf",
  "owner": 1,
  "custom_fields": []
}
```

### Tag (key fields)

```json
{
  "id": 1,
  "name": "invoice",
  "color": "#a6cee3",
  "is_inbox_tag": false,
  "document_count": 42,
  "slug": "invoice",
  "owner": 1
}
```

### Correspondent / DocumentType (key fields)

```json
{
  "id": 1,
  "name": "ACME Corp",
  "match": "acme",
  "matching_algorithm": 1,
  "is_insensitive": true,
  "document_count": 15,
  "slug": "acme-corp",
  "owner": 1
}
```

## Pagination

All list endpoints use DRF pagination:

```json
{
  "count": 150,
  "next": "http://localhost:8000/api/documents/?page=2&page_size=25",
  "previous": null,
  "results": [...]
}
```

Default page size: 25. Maximum: configurable (default 100 for our client).

## Filtering (Documents)

Query parameters for `/api/documents/`:

| Parameter | Description |
|-----------|-------------|
| `query` | Full-text search |
| `tags__name__icontains` | Filter by tag name (partial) |
| `tags__id__in` | Filter by tag IDs |
| `correspondent__name__icontains` | Filter by correspondent name |
| `correspondent__id` | Filter by correspondent ID |
| `document_type__name__icontains` | Filter by document type name |
| `document_type__id` | Filter by document type ID |
| `created__date__gt` | Created after date |
| `created__date__lt` | Created before date |
| `ordering` | Sort field (e.g. `-created`, `title`) |

## Upload Format

Documents are uploaded via multipart form POST to `/api/documents/post_document/`:

- Field name: `document` (the file)
- Optional: `title`, `correspondent`, `document_type`, `tags` (multiple)
- The response is a task UUID for async processing

## Error Handling

- 401: Token invalid or missing
- 403: Permission denied
- 404: Resource not found
- 400: Validation error (response body contains field errors)
- 500: Server error

## Notes

- Paperless uses soft-delete for documents (trash system)
- The `UnifiedSearchViewSet` for documents combines DRF + Whoosh FTS
- Tags support hierarchical nesting (TreeNode) with max depth of 5
- Custom fields are supported (various types: string, integer, date, URL, etc.)
