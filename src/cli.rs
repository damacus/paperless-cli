use std::path::PathBuf;
use std::time::Instant;

use anyhow::{anyhow, Result};
use clap::{ArgAction, Args, Parser, Subcommand};
use serde_json::{json, Value};

use crate::api::{DocumentQuery, PaperlessApi};
use crate::config::load_config;
use crate::output::{render, OutputFormat};
use crate::tui;

#[derive(Parser, Debug)]
#[command(name = "paperless", version, about = "Rust TUI and CLI for Paperless-ngx")]
pub struct Cli {
    #[arg(long)]
    output: Option<String>,
    #[arg(long, action = ArgAction::SetTrue)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Project(ProjectCommand),
    Document(DocumentCommand),
    Search(SearchCommand),
    Tag(TagCommand),
    Correspondent(CorrespondentCommand),
    Doctype(DoctypeCommand),
    Task(TaskCommand),
    Export(ExportCommand),
    Status,
    Tui,
}

#[derive(Subcommand, Debug)]
pub enum ProjectCommand {
    Init(ProjectInit),
    Info,
    Ping,
}

#[derive(Args, Debug)]
pub struct ProjectInit {
    #[arg(long)]
    url: String,
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    username: Option<String>,
    #[arg(long)]
    password: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum DocumentCommand {
    List(DocumentList),
    Search(DocumentList),
    Get(IdArg),
    Delete(DeleteArgs),
    Update(DocumentUpdate),
    Upload(DocumentUpload),
    Download(DocumentDownload),
    Preview(DocumentDownload),
    Thumb(DocumentDownload),
}

#[derive(Args, Debug, Clone)]
pub struct DocumentList {
    #[arg(long, short)]
    query: Option<String>,
    #[arg(long, short = 't')]
    tag: Option<String>,
    #[arg(long)]
    tag_id: Option<u64>,
    #[arg(long, short = 'c')]
    correspondent: Option<String>,
    #[arg(long)]
    correspondent_id: Option<u64>,
    #[arg(long = "type")]
    doc_type: Option<String>,
    #[arg(long = "type-id")]
    doc_type_id: Option<u64>,
    #[arg(long)]
    created_after: Option<String>,
    #[arg(long)]
    created_before: Option<String>,
    #[arg(long, default_value = "-created")]
    order_by: String,
    #[arg(long, default_value_t = 25)]
    page_size: u64,
    #[arg(long, default_value_t = 1)]
    page: u64,
}

impl From<DocumentList> for DocumentQuery {
    fn from(value: DocumentList) -> Self {
        Self {
            query: value.query,
            tag: value.tag,
            tag_id: value.tag_id,
            correspondent: value.correspondent,
            correspondent_id: value.correspondent_id,
            doc_type: value.doc_type,
            doc_type_id: value.doc_type_id,
            created_after: value.created_after,
            created_before: value.created_before,
            order_by: value.order_by,
            page_size: value.page_size,
            page: value.page,
        }
    }
}

#[derive(Args, Debug)]
pub struct DocumentUpload {
    file: PathBuf,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    correspondent_id: Option<u64>,
    #[arg(long)]
    document_type_id: Option<u64>,
    #[arg(long)]
    tag_id: Vec<u64>,
}

#[derive(Args, Debug)]
pub struct DocumentDownload {
    id: u64,
    #[arg(long, default_value = ".")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = false)]
    original: bool,
}

#[derive(Args, Debug)]
pub struct DocumentUpdate {
    id: u64,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    correspondent_id: Option<u64>,
    #[arg(long)]
    document_type_id: Option<u64>,
    #[arg(long)]
    tag_id: Vec<u64>,
    #[arg(long)]
    created: Option<String>,
}

#[derive(Args, Debug)]
pub struct IdArg {
    id: u64,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    id: u64,
    #[arg(long, action = ArgAction::SetTrue)]
    yes: bool,
}

#[derive(Subcommand, Debug)]
pub enum SearchCommand {
    Query(SearchQueryArgs),
    Autocomplete(SearchAutocompleteArgs),
}

#[derive(Args, Debug)]
pub struct SearchQueryArgs {
    query: String,
    #[arg(long, default_value_t = 25)]
    page_size: u64,
    #[arg(long, default_value_t = 1)]
    page: u64,
}

#[derive(Args, Debug)]
pub struct SearchAutocompleteArgs {
    term: String,
    #[arg(long, default_value_t = 10)]
    limit: u64,
}

#[derive(Subcommand, Debug)]
pub enum TagCommand {
    List,
    Get(IdArg),
    Create(TagCreateArgs),
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct TagCreateArgs {
    name: String,
    #[arg(long, default_value = "#a6cee3")]
    color: String,
}

#[derive(Subcommand, Debug)]
pub enum CorrespondentCommand {
    List,
    Get(IdArg),
    Create(NameArg),
    Delete(DeleteArgs),
}

#[derive(Subcommand, Debug)]
pub enum DoctypeCommand {
    List,
    Get(IdArg),
    Create(NameArg),
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct NameArg {
    name: String,
}

#[derive(Subcommand, Debug)]
pub enum TaskCommand {
    List,
    Get(IdArg),
}

#[derive(Subcommand, Debug)]
pub enum ExportCommand {
    Bulk(ExportBulkArgs),
}

#[derive(Args, Debug)]
pub struct ExportBulkArgs {
    #[arg(long)]
    id: Vec<u64>,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value = "both")]
    content: String,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let output_format = OutputFormat::from_flags(cli.output.as_deref(), cli.json);

    match cli.command {
        None | Some(Command::Tui) => {
            let api = PaperlessApi::from_disk()?;
            tui::run(api)
        }
        Some(Command::Status) => {
            let value = status_value()?;
            print_value(&value, output_format);
            Ok(())
        }
        Some(Command::Project(command)) => {
            let value = run_project(command)?;
            print_value(&value, output_format);
            Ok(())
        }
        Some(command) => {
            let api = PaperlessApi::from_disk()?;
            let value = run_with_api(api, command)?;
            print_value(&value, output_format);
            Ok(())
        }
    }
}

fn run_project(command: ProjectCommand) -> Result<Value> {
    match command {
        ProjectCommand::Init(args) => {
            let config = PaperlessApi::init_config(
                &args.url,
                args.token.as_deref(),
                args.username.as_deref(),
                args.password.as_deref(),
            )?;
            Ok(json!({
                "url": config.url,
                "token": config.masked_token(),
                "configured": true
            }))
        }
        ProjectCommand::Info => {
            let api = PaperlessApi::from_disk()?;
            api.connection_info()
        }
        ProjectCommand::Ping => {
            let api = PaperlessApi::from_disk()?;
            let start = Instant::now();
            let mut value = api.ping()?;
            value["elapsed_ms"] = json!((start.elapsed().as_secs_f64() * 1000.0).round());
            Ok(value)
        }
    }
}

fn status_value() -> Result<Value> {
    let config = match load_config() {
        Ok(config) => config,
        Err(error) => {
            return Ok(json!({
                "connected": false,
                "status": "not_configured",
                "message": error.to_string(),
            }))
        }
    };

    let api = PaperlessApi::new(config.clone())?;
    match api.ping() {
        Ok(ping) => Ok(json!({
            "connected": true,
            "status": "ok",
            "url": config.url,
            "server": ping,
        })),
        Err(error) => Ok(json!({
            "connected": false,
            "status": "error",
            "url": config.url,
            "message": error.to_string(),
        })),
    }
}

fn run_with_api(api: PaperlessApi, command: Command) -> Result<Value> {
    match command {
        Command::Document(command) => match command {
            DocumentCommand::List(args) => api.list_documents(&args.into()),
            DocumentCommand::Search(args) => api.search_documents(&args.into()),
            DocumentCommand::Get(args) => api.get_document(args.id),
            DocumentCommand::Delete(args) => {
                ensure_yes(args.yes)?;
                api.delete_document(args.id)?;
                Ok(json!({"status": "deleted", "id": args.id}))
            }
            DocumentCommand::Update(args) => {
                let mut patch = serde_json::Map::new();
                if let Some(title) = args.title {
                    patch.insert("title".into(), json!(title));
                }
                if let Some(correspondent_id) = args.correspondent_id {
                    patch.insert("correspondent".into(), json!(correspondent_id));
                }
                if let Some(document_type_id) = args.document_type_id {
                    patch.insert("document_type".into(), json!(document_type_id));
                }
                if !args.tag_id.is_empty() {
                    patch.insert("tags".into(), json!(args.tag_id));
                }
                if let Some(created) = args.created {
                    patch.insert("created".into(), json!(created));
                }
                if patch.is_empty() {
                    return Err(anyhow!("No fields to update provided."));
                }
                api.update_document(args.id, Value::Object(patch))
            }
            DocumentCommand::Upload(args) => api.upload_document(
                &args.file,
                args.title.as_deref(),
                args.correspondent_id,
                args.document_type_id,
                &args.tag_id,
            ),
            DocumentCommand::Download(args) => Ok(json!({
                "path": api.download_document_asset(
                    args.id,
                    "download",
                    &args.output_dir,
                    if args.original {
                        &[("original".into(), "true".into())]
                    } else {
                        &[]
                    },
                )?.display().to_string()
            })),
            DocumentCommand::Preview(args) => Ok(json!({
                "path": api.download_document_asset(args.id, "preview", &args.output_dir, &[])?.display().to_string()
            })),
            DocumentCommand::Thumb(args) => Ok(json!({
                "path": api.download_document_asset(args.id, "thumb", &args.output_dir, &[])?.display().to_string()
            })),
        },
        Command::Search(command) => match command {
            SearchCommand::Query(args) => api.search_query(&args.query, args.page_size, args.page),
            SearchCommand::Autocomplete(args) => api.search_autocomplete(&args.term, args.limit),
        },
        Command::Task(command) => match command {
            TaskCommand::List => api.list_tasks(),
            TaskCommand::Get(args) => api.get_task(args.id),
        },
        Command::Tag(command) => match command {
            TagCommand::List => api.list_tags(),
            TagCommand::Get(args) => api.get_tag(args.id),
            TagCommand::Create(args) => api.create_tag(&args.name, &args.color),
            TagCommand::Delete(args) => {
                ensure_yes(args.yes)?;
                api.delete_tag(args.id)?;
                Ok(json!({"status": "deleted", "id": args.id}))
            }
        },
        Command::Correspondent(command) => match command {
            CorrespondentCommand::List => api.list_correspondents(),
            CorrespondentCommand::Get(args) => api.get_correspondent(args.id),
            CorrespondentCommand::Create(args) => api.create_correspondent(&args.name),
            CorrespondentCommand::Delete(args) => {
                ensure_yes(args.yes)?;
                api.delete_correspondent(args.id)?;
                Ok(json!({"status": "deleted", "id": args.id}))
            }
        },
        Command::Doctype(command) => match command {
            DoctypeCommand::List => api.list_doctypes(),
            DoctypeCommand::Get(args) => api.get_doctype(args.id),
            DoctypeCommand::Create(args) => api.create_doctype(&args.name),
            DoctypeCommand::Delete(args) => {
                ensure_yes(args.yes)?;
                api.delete_doctype(args.id)?;
                Ok(json!({"status": "deleted", "id": args.id}))
            }
        },
        Command::Export(ExportCommand::Bulk(args)) => Ok(json!({
            "path": api.bulk_download_zip(&args.id, &args.output, &args.content)?.display().to_string()
        })),
        Command::Status | Command::Project(_) | Command::Tui => unreachable!(),
    }
}

fn ensure_yes(yes: bool) -> Result<()> {
    if yes {
        Ok(())
    } else {
        Err(anyhow!("Refusing destructive action without --yes"))
    }
}

fn print_value(value: &Value, output_format: OutputFormat) {
    println!("{}", render(value, output_format));
}
