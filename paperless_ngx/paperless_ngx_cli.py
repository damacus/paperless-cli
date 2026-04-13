"""Paperless-ngx CLI.

Stateful CLI + REPL for the Paperless-ngx document management system.
Every command supports --json output for scripting.

Entry point: paperless
"""

from __future__ import annotations

import json
import shlex
from typing import Any

import click

from paperless_ngx import __version__
from paperless_ngx.core.session import get_session
from paperless_ngx.utils.paperless_backend import (
    PaperlessBackend,
    find_paperless_server,
)
from paperless_ngx.utils.repl_skin import ReplSkin

# ── Helpers ──────────────────────────────────────────────────────────────────


def _output(data: Any, as_json: bool, skin: ReplSkin | None = None):
    """Print data either as pretty JSON or formatted table."""
    if as_json:
        click.echo(json.dumps(data, indent=2, default=str))
    else:
        if skin:
            _pretty_print(data, skin)
        else:
            click.echo(json.dumps(data, indent=2, default=str))


def _pretty_print(data: Any, skin: ReplSkin):
    """Human-friendly output using ReplSkin."""
    if isinstance(data, list):
        if not data:
            skin.info("No results.")
            return
        # Use first item keys as headers
        headers = list(data[0].keys())[:6]  # cap at 6 columns
        rows = []
        for item in data:
            row = [str(item.get(h, "")) for h in headers]
            # Truncate long values
            row = [v[:60] + "…" if len(v) > 60 else v for v in row]
            rows.append(row)
        skin.table(headers, rows)
    elif isinstance(data, dict):
        if "results" in data and "count" in data:
            # Paginated list response
            skin.info(f"Total: {data['count']} results")
            _pretty_print(data["results"], skin)
        else:
            for k, v in data.items():
                if isinstance(v, (dict, list)):
                    skin.status(k, json.dumps(v, default=str))
                else:
                    skin.status(k, str(v))
    else:
        skin.info(str(data))


def _get_backend() -> PaperlessBackend:
    """Get an authenticated backend, raising a clear error if not configured."""
    try:
        return PaperlessBackend()
    except RuntimeError as exc:
        raise click.ClickException(str(exc)) from exc


# ── Root group ───────────────────────────────────────────────────────────────


@click.group(invoke_without_command=True)
@click.version_option(__version__, prog_name="paperless")
@click.option(
    "--json", "as_json", is_flag=True, default=False, help="Output results as JSON."
)
@click.pass_context
def main(ctx: click.Context, as_json: bool):
    """paperless — Paperless-ngx document management CLI.

    Run without a subcommand to enter the interactive REPL.
    """
    ctx.ensure_object(dict)
    ctx.obj["as_json"] = as_json
    ctx.obj["skin"] = ReplSkin("paperless_ngx", version=__version__)

    if ctx.invoked_subcommand is None:
        # Enter REPL
        _run_repl(ctx.obj["skin"])


# ── project group ─────────────────────────────────────────────────────────────


@main.group()
def project():
    """Manage the connection to your Paperless-ngx server."""


@project.command("init")
@click.option(
    "--url",
    required=True,
    prompt="Paperless-ngx URL",
    help="Base URL of the server (e.g. http://localhost:8000)",
)
@click.option("--token", default=None, help="API authentication token.")
@click.option("--username", default=None, help="Username (alternative to --token).")
@click.option(
    "--password",
    default=None,
    hide_input=True,
    help="Password (alternative to --token).",
)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def project_init(ctx, url, token, username, password, as_json):
    """Initialize the connection to a Paperless-ngx server."""
    from paperless_ngx.core.project import init_connection

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    try:
        config = init_connection(url, token=token, username=username, password=password)
        result = {
            "status": "ok",
            "url": config.url,
            "message": "Connection configured and verified.",
        }
        _output(result, as_json, skin)
    except (ValueError, RuntimeError) as exc:
        raise click.ClickException(str(exc)) from exc


@project.command("info")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def project_info(ctx, as_json):
    """Show current connection info and server statistics."""
    from paperless_ngx.core.project import get_connection_info

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    try:
        info = get_connection_info()
        _output(info, as_json, skin)
    except RuntimeError as exc:
        raise click.ClickException(str(exc)) from exc


@project.command("ping")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def project_ping(ctx, as_json):
    """Test the connection to the configured Paperless-ngx server."""
    from paperless_ngx.core.project import ping_server

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    try:
        result = ping_server()
        _output(result, as_json, skin)
        if not as_json:
            skin.success(f"Connected to {result['url']} ({result['elapsed_ms']}ms)")
    except RuntimeError as exc:
        raise click.ClickException(str(exc)) from exc


# ── document group ────────────────────────────────────────────────────────────


@main.group()
def document():
    """Document CRUD operations."""


@document.command("list")
@click.option("--query", "-q", default=None, help="Full-text search query.")
@click.option("--tag", "-t", default=None, help="Filter by tag name.")
@click.option(
    "--correspondent", "-c", default=None, help="Filter by correspondent name."
)
@click.option("--type", "doc_type", default=None, help="Filter by document type name.")
@click.option(
    "--page-size", default=25, show_default=True, help="Number of results per page."
)
@click.option("--page", default=1, show_default=True, help="Page number.")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def document_list(ctx, query, tag, correspondent, doc_type, page_size, page, as_json):
    """List documents with optional filters."""
    from paperless_ngx.core.documents import list_documents

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = list_documents(
        backend,
        query=query,
        tag=tag,
        correspondent=correspondent,
        doc_type=doc_type,
        page_size=page_size,
        page=page,
    )
    # Save last query for REPL context
    if query:
        sess = get_session()
        sess.last_query = query
    _output(result, as_json, skin)


@document.command("get")
@click.argument("doc_id", type=int)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def document_get(ctx, doc_id, as_json):
    """Get details for a specific document by ID."""
    from paperless_ngx.core.documents import get_document

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = get_document(backend, doc_id)
    _output(result, as_json, skin)


@document.command("upload")
@click.argument("file_path", type=click.Path(exists=True))
@click.option("--title", default=None, help="Document title.")
@click.option("--correspondent-id", type=int, default=None)
@click.option("--type-id", "document_type_id", type=int, default=None)
@click.option(
    "--tag-id",
    "tag_ids",
    type=int,
    multiple=True,
    help="Tag ID(s) to assign (repeatable).",
)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def document_upload(
    ctx, file_path, title, correspondent_id, document_type_id, tag_ids, as_json
):
    """Upload a document file to Paperless-ngx."""
    from paperless_ngx.core.documents import upload_document

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = upload_document(
        backend,
        file_path,
        title=title,
        correspondent_id=correspondent_id,
        document_type_id=document_type_id,
        tag_ids=list(tag_ids) if tag_ids else None,
    )
    if not as_json:
        skin.success(f"Uploaded: {file_path}")
    _output(result, as_json, skin)


@document.command("download")
@click.argument("doc_id", type=int)
@click.option(
    "--output-dir",
    "-o",
    default=".",
    show_default=True,
    help="Directory to save the file.",
)
@click.option(
    "--original",
    is_flag=True,
    default=False,
    help="Download the original file (not the archived version).",
)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def document_download(ctx, doc_id, output_dir, original, as_json):
    """Download a document file to a local directory."""
    from paperless_ngx.core.documents import download_document

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    path = download_document(backend, doc_id, output_dir=output_dir, original=original)
    result = {"doc_id": doc_id, "path": path, "status": "ok"}
    if not as_json:
        skin.success(f"Downloaded to: {path}")
    _output(result, as_json, skin)


@document.command("update")
@click.argument("doc_id", type=int)
@click.option("--title", default=None)
@click.option("--correspondent-id", type=int, default=None)
@click.option("--type-id", "document_type_id", type=int, default=None)
@click.option(
    "--tag-id",
    "tag_ids",
    type=int,
    multiple=True,
    help="Tag IDs (replaces all existing tags).",
)
@click.option("--created", default=None, help="Created date (YYYY-MM-DD).")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def document_update(
    ctx, doc_id, title, correspondent_id, document_type_id, tag_ids, created, as_json
):
    """Update document metadata."""
    from paperless_ngx.core.documents import update_document

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = update_document(
        backend,
        doc_id,
        title=title,
        correspondent_id=correspondent_id,
        document_type_id=document_type_id,
        tag_ids=list(tag_ids) if tag_ids else None,
        created=created,
    )
    if not as_json:
        skin.success(f"Updated document {doc_id}")
    _output(result, as_json, skin)


@document.command("delete")
@click.argument("doc_id", type=int)
@click.option(
    "--yes", "-y", is_flag=True, default=False, help="Skip confirmation prompt."
)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def document_delete(ctx, doc_id, yes, as_json):
    """Delete a document by ID."""
    from paperless_ngx.core.documents import delete_document

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    if not yes:
        click.confirm(f"Delete document {doc_id}?", abort=True)
    backend = _get_backend()
    delete_document(backend, doc_id)
    result = {"doc_id": doc_id, "status": "deleted"}
    if not as_json:
        skin.success(f"Deleted document {doc_id}")
    _output(result, as_json, skin)


@document.command("search")
@click.argument("query")
@click.option("--page-size", default=25, show_default=True)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def document_search(ctx, query, page_size, as_json):
    """Full-text search documents."""
    from paperless_ngx.core.documents import search_documents

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    sess = get_session()
    sess.last_query = query
    result = search_documents(backend, query, page_size=page_size)
    _output(result, as_json, skin)


# ── tag group ─────────────────────────────────────────────────────────────────


@main.group()
def tag():
    """Tag management."""


@tag.command("list")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def tag_list(ctx, as_json):
    """List all tags."""
    from paperless_ngx.core.tags import list_tags

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = list_tags(backend)
    _output(result, as_json, skin)


@tag.command("create")
@click.argument("name")
@click.option("--color", default="#a6cee3", show_default=True)
@click.option("--inbox", is_flag=True, default=False, help="Mark as inbox tag.")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def tag_create(ctx, name, color, inbox, as_json):
    """Create a new tag."""
    from paperless_ngx.core.tags import create_tag

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = create_tag(backend, name, color=color, is_inbox_tag=inbox)
    if not as_json:
        skin.success(f"Created tag: {name} (id={result.get('id')})")
    _output(result, as_json, skin)


@tag.command("delete")
@click.argument("tag_id", type=int)
@click.option("--yes", "-y", is_flag=True, default=False)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def tag_delete(ctx, tag_id, yes, as_json):
    """Delete a tag by ID."""
    from paperless_ngx.core.tags import delete_tag

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    if not yes:
        click.confirm(f"Delete tag {tag_id}?", abort=True)
    backend = _get_backend()
    delete_tag(backend, tag_id)
    result = {"tag_id": tag_id, "status": "deleted"}
    if not as_json:
        skin.success(f"Deleted tag {tag_id}")
    _output(result, as_json, skin)


# ── correspondent group ───────────────────────────────────────────────────────


@main.group()
def correspondent():
    """Correspondent management."""


@correspondent.command("list")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def correspondent_list(ctx, as_json):
    """List all correspondents."""
    from paperless_ngx.core.correspondents import list_correspondents

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = list_correspondents(backend)
    _output(result, as_json, skin)


@correspondent.command("create")
@click.argument("name")
@click.option("--match", default="", help="Matching pattern.")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def correspondent_create(ctx, name, match, as_json):
    """Create a new correspondent."""
    from paperless_ngx.core.correspondents import create_correspondent

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = create_correspondent(backend, name, match=match)
    if not as_json:
        skin.success(f"Created correspondent: {name} (id={result.get('id')})")
    _output(result, as_json, skin)


@correspondent.command("delete")
@click.argument("correspondent_id", type=int)
@click.option("--yes", "-y", is_flag=True, default=False)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def correspondent_delete(ctx, correspondent_id, yes, as_json):
    """Delete a correspondent by ID."""
    from paperless_ngx.core.correspondents import delete_correspondent

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    if not yes:
        click.confirm(f"Delete correspondent {correspondent_id}?", abort=True)
    backend = _get_backend()
    delete_correspondent(backend, correspondent_id)
    result = {"correspondent_id": correspondent_id, "status": "deleted"}
    if not as_json:
        skin.success(f"Deleted correspondent {correspondent_id}")
    _output(result, as_json, skin)


# ── doctype group ─────────────────────────────────────────────────────────────


@main.group()
def doctype():
    """Document type management."""


@doctype.command("list")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def doctype_list(ctx, as_json):
    """List all document types."""
    from paperless_ngx.core.doc_types import list_doc_types

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = list_doc_types(backend)
    _output(result, as_json, skin)


@doctype.command("create")
@click.argument("name")
@click.option("--match", default="", help="Matching pattern.")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def doctype_create(ctx, name, match, as_json):
    """Create a new document type."""
    from paperless_ngx.core.doc_types import create_doc_type

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()
    result = create_doc_type(backend, name, match=match)
    if not as_json:
        skin.success(f"Created document type: {name} (id={result.get('id')})")
    _output(result, as_json, skin)


@doctype.command("delete")
@click.argument("doc_type_id", type=int)
@click.option("--yes", "-y", is_flag=True, default=False)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def doctype_delete(ctx, doc_type_id, yes, as_json):
    """Delete a document type by ID."""
    from paperless_ngx.core.doc_types import delete_doc_type

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    if not yes:
        click.confirm(f"Delete document type {doc_type_id}?", abort=True)
    backend = _get_backend()
    delete_doc_type(backend, doc_type_id)
    result = {"doc_type_id": doc_type_id, "status": "deleted"}
    if not as_json:
        skin.success(f"Deleted document type {doc_type_id}")
    _output(result, as_json, skin)


# ── export group ──────────────────────────────────────────────────────────────


@main.group()
def export():
    """Export and bulk download operations."""


@export.command("bulk")
@click.argument("ids", nargs=-1, type=int, required=True)
@click.option(
    "--output-dir",
    "-o",
    default="./paperless-export",
    show_default=True,
    help="Directory to save files.",
)
@click.option("--original", is_flag=True, default=False)
@click.option(
    "--zip",
    "as_zip",
    is_flag=True,
    default=False,
    help="Download as a single ZIP file.",
)
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def export_bulk(ctx, ids, output_dir, original, as_zip, as_json):
    """Bulk download documents to a directory."""
    from paperless_ngx.core.export import bulk_download, bulk_download_zip

    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    backend = _get_backend()

    if as_zip:
        import os

        zip_path = os.path.join(output_dir, "paperless-export.zip")
        content = "originals" if original else "both"
        path = bulk_download_zip(backend, list(ids), zip_path, content=content)
        result = {"path": path, "count": len(ids), "status": "ok"}
        if not as_json:
            skin.success(f"Downloaded {len(ids)} documents to: {path}")
    else:

        def progress(current, total, doc_id):
            if not as_json and total > 0:
                skin.progress(current, total, f"doc {doc_id}" if doc_id else "done")

        results = bulk_download(
            backend,
            list(ids),
            output_dir=output_dir,
            original=original,
            progress_callback=progress,
        )
        ok = [r for r in results if r["status"] == "ok"]
        errors = [r for r in results if r["status"] == "error"]
        result = {"downloaded": len(ok), "errors": len(errors), "results": results}
        if not as_json:
            skin.success(
                f"Downloaded {len(ok)} of {len(ids)} documents to: {output_dir}"
            )
            if errors:
                skin.warning(f"{len(errors)} document(s) failed.")
    _output(result, as_json, skin)


# ── status command ────────────────────────────────────────────────────────────


@main.command("status")
@click.option("--json", "as_json", is_flag=True, default=False)
@click.pass_context
def status_cmd(ctx, as_json):
    """Show current session status and connection info."""
    as_json = as_json or ctx.obj.get("as_json", False)
    skin = ctx.obj["skin"]
    sess = get_session()

    try:
        config = find_paperless_server()
        connected = True
        url = config.url
        token_preview = config.token[:8] + "..." if len(config.token) > 8 else "***"
    except RuntimeError:
        connected = False
        url = "not configured"
        token_preview = "n/a"

    result = {
        "connected": connected,
        "url": url,
        "token": token_preview,
        "last_query": sess.last_query,
        "selected_docs": sess.selected_docs,
    }
    _output(result, as_json, skin)


# ── repl command ──────────────────────────────────────────────────────────────


@main.command("repl")
@click.pass_context
def repl_cmd(ctx):
    """Enter the interactive REPL."""
    skin = ctx.obj["skin"]
    _run_repl(skin)


# ── REPL engine ───────────────────────────────────────────────────────────────

_REPL_HELP = {
    "document list [opts]": "List documents",
    "document get <id>": "Get document details",
    "document upload <file>": "Upload a file",
    "document download <id>": "Download a document",
    "document search <q>": "Search documents",
    "document update <id>": "Update document metadata",
    "document delete <id>": "Delete a document",
    "tag list": "List tags",
    "tag create <name>": "Create a tag",
    "tag delete <id>": "Delete a tag",
    "correspondent list": "List correspondents",
    "correspondent create": "Create a correspondent",
    "doctype list": "List document types",
    "export bulk <ids>": "Bulk download documents",
    "project info": "Show connection info",
    "project ping": "Test connection",
    "status": "Show session status",
    "help": "Show this help",
    "quit / exit": "Exit the REPL",
}


def _run_repl(skin: ReplSkin):
    """Run the interactive REPL loop."""
    skin.print_banner()

    sess = get_session()
    pt_session = skin.create_prompt_session()

    while True:
        try:
            # Build context string for prompt
            ctx_str = ""
            try:
                config = find_paperless_server()
                from urllib.parse import urlparse

                host = urlparse(config.url).netloc
                ctx_str = host
            except RuntimeError:
                ctx_str = "not connected"

            raw = skin.get_input(pt_session, context=ctx_str)
        except (EOFError, KeyboardInterrupt):
            skin.print_goodbye()
            break

        if not raw:
            continue

        cmd = raw.strip()
        if cmd in ("quit", "exit", "q"):
            skin.print_goodbye()
            break
        if cmd in ("help", "?", "h"):
            skin.help(_REPL_HELP)
            continue

        # Add to session history
        sess.add_history(cmd)

        # Dispatch to Click CLI
        try:
            args = shlex.split(cmd)
            # Re-invoke the Click main group with the parsed args
            # standalone_mode=False means Click won't call sys.exit
            main.main(
                args=args, standalone_mode=False, obj={"as_json": False, "skin": skin}
            )
        except SystemExit:
            pass
        except click.exceptions.Abort:
            skin.warning("Aborted.")
        except click.ClickException as exc:
            skin.error(exc.format_message())
        except Exception as exc:
            skin.error(f"Error: {exc}")
