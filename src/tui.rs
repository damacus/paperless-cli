use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::{Frame, Terminal};

use crate::api::{ApiClient, Transport};
use crate::error::AppError;
use crate::security::{SecurityFinding, Severity};
use crate::services::{
    document_summaries_from_response, get_document_inspector, latest_task_summary, list_documents,
    list_tasks, next_page_url_from_response, DashboardSnapshot, DocumentInspector, DocumentQuery,
    DocumentSummary, TaskSummary,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneFocus {
    Documents,
    Inspector,
}

#[derive(Clone, Debug)]
pub struct TuiApp {
    pub title: String,
    pub project_summary: String,
    pub documents: Vec<DocumentSummary>,
    pub selected_document: usize,
    pub documents_state: ListState,
    pub inspector_cache: HashMap<u64, DocumentInspector>,
    pub loading_documents: bool,
    pub loading_inspector: bool,
    pub latest_task: Option<TaskSummary>,
    pub security: Vec<SecurityFinding>,
    pub status_line: String,
    pub loaded_document_count: usize,
    pub document_total: Option<usize>,
    pub documents_viewport_height: usize,
    pub inspector_scroll: usize,
    pub inspector_viewport_height: usize,
    pub focus: PaneFocus,
    pending_inspector_id: Option<u64>,
}

#[derive(Debug)]
enum TuiEvent {
    DocumentsPage {
        documents: Vec<DocumentSummary>,
        total: Option<usize>,
        done: bool,
    },
    DocumentsError(String),
    Inspector {
        document_id: u64,
        result: Result<DocumentInspector, String>,
    },
    LatestTask(Result<Option<TaskSummary>, String>),
}

impl TuiApp {
    pub fn from_snapshot(snapshot: DashboardSnapshot) -> Self {
        let has_documents = !snapshot.documents.is_empty();
        let loaded_document_count = snapshot.documents.len();
        let project_summary = snapshot
            .project
            .get("url")
            .or_else(|| {
                snapshot
                    .project
                    .get("response")
                    .and_then(|value| value.get("version"))
            })
            .and_then(|value| value.as_str())
            .map(|value| format!("connected {value}"))
            .unwrap_or_else(|| "connected".to_string());

        Self {
            title: "paperless-cli".to_string(),
            project_summary,
            documents: snapshot.documents,
            selected_document: 0,
            documents_state: ListState::default().with_selected(has_documents.then_some(0)),
            inspector_cache: HashMap::new(),
            loading_documents: true,
            loading_inspector: false,
            latest_task: snapshot.latest_task,
            security: snapshot.security,
            status_line:
                "[Tab] focus  [j/k] move  [PgUp/PgDn] page  [g/G] top/end  [r] reload  [q] quit"
                    .to_string(),
            loaded_document_count,
            document_total: None,
            documents_viewport_height: 1,
            inspector_scroll: 0,
            inspector_viewport_height: 1,
            focus: PaneFocus::Documents,
            pending_inspector_id: None,
        }
    }

    pub fn on_security_update(&mut self, findings: Vec<SecurityFinding>) {
        self.security = findings;
    }

    pub fn current_document_id(&self) -> Option<u64> {
        self.documents
            .get(self.selected_document)
            .map(|document| document.id)
    }

    pub fn current_inspector(&self) -> Option<&DocumentInspector> {
        self.current_document_id()
            .and_then(|document_id| self.inspector_cache.get(&document_id))
    }

    pub fn selected_metadata(&self) -> Vec<String> {
        let mut sections = Vec::new();

        if let Some(document) = self.documents.get(self.selected_document) {
            sections.push(format!("doc {}  {}", document.id, document.created));
        }

        if let Some(inspector) = self.current_inspector() {
            if !inspector.metadata.is_empty() {
                sections.push(inspector.metadata.join("  |  "));
            }
        }

        sections.push(self.project_summary.clone());

        if let Some(task) = &self.latest_task {
            let detail = if task.note.is_empty() {
                format!("latest task {}", task.status)
            } else {
                format!("latest task {}  {}", task.status, task.note)
            };
            sections.push(detail);
        }

        if let Some(finding) = self.security.first() {
            sections.push(format!(
                "security {} {}",
                severity_label(&finding.severity),
                finding.title
            ));
        } else {
            sections.push("security clear".to_string());
        }

        sections.push(self.status_line.clone());

        sections
    }

    pub fn on_documents_page(
        &mut self,
        documents: Vec<DocumentSummary>,
        total: Option<usize>,
        done: bool,
    ) -> Option<u64> {
        let was_empty = self.documents.is_empty();
        if was_empty {
            self.pending_inspector_id = None;
        }
        self.documents.extend(documents);
        self.loaded_document_count = self.documents.len();
        if let Some(total) = total {
            self.document_total = Some(total);
        }
        self.loading_documents = !done;
        if was_empty && !self.documents.is_empty() {
            self.set_selected_document(0);
            self.current_document_id()
        } else {
            None
        }
    }

    pub fn on_inspector_loaded(&mut self, document_id: u64, inspector: DocumentInspector) {
        self.loading_inspector = false;
        self.pending_inspector_id = None;
        self.inspector_scroll = 0;
        self.inspector_cache.insert(document_id, inspector);
    }

    pub fn on_loader_error(&mut self, message: String) {
        self.loading_documents = false;
        self.loading_inspector = false;
        self.pending_inspector_id = None;
        self.project_summary = format!("error {message}");
    }

    pub fn request_documents_reload(&mut self) {
        self.loading_documents = true;
        self.documents.clear();
        self.loaded_document_count = 0;
        self.document_total = None;
        self.selected_document = 0;
        self.documents_state.select(None);
        self.inspector_scroll = 0;
        self.pending_inspector_id = None;
    }

    pub fn maybe_request_inspector(&mut self) -> Option<u64> {
        let document_id = self.current_document_id()?;
        if self.inspector_cache.contains_key(&document_id)
            || self.pending_inspector_id == Some(document_id)
        {
            return None;
        }

        self.loading_inspector = true;
        self.pending_inspector_id = Some(document_id);
        self.inspector_scroll = 0;
        Some(document_id)
    }

    pub fn select_next(&mut self) -> bool {
        if self.documents.is_empty() {
            return false;
        }
        let next = (self.selected_document + 1) % self.documents.len();
        let changed = next != self.selected_document;
        self.set_selected_document(next);
        changed
    }

    pub fn select_previous(&mut self) -> bool {
        if self.documents.is_empty() {
            return false;
        }
        let next = if self.selected_document == 0 {
            self.documents.len() - 1
        } else {
            self.selected_document - 1
        };
        let changed = next != self.selected_document;
        self.set_selected_document(next);
        changed
    }

    pub fn select_first(&mut self) -> bool {
        if self.documents.is_empty() || self.selected_document == 0 {
            return false;
        }
        self.set_selected_document(0);
        true
    }

    pub fn select_last(&mut self) -> bool {
        if self.documents.is_empty() {
            return false;
        }
        let last = self.documents.len() - 1;
        if self.selected_document == last {
            return false;
        }
        self.set_selected_document(last);
        true
    }

    pub fn select_page_down(&mut self) -> bool {
        if self.documents.is_empty() {
            return false;
        }
        let page = self.documents_viewport_height.max(1);
        let last = self.documents.len() - 1;
        let next = (self.selected_document + page).min(last);
        let changed = next != self.selected_document;
        self.set_selected_document(next);
        changed
    }

    pub fn select_page_up(&mut self) -> bool {
        if self.documents.is_empty() {
            return false;
        }
        let page = self.documents_viewport_height.max(1);
        let next = self.selected_document.saturating_sub(page);
        let changed = next != self.selected_document;
        self.set_selected_document(next);
        changed
    }

    fn set_selected_document(&mut self, index: usize) {
        self.selected_document = index;
        self.inspector_scroll = 0;
        let selected = if self.documents.is_empty() {
            None
        } else {
            Some(index.min(self.documents.len() - 1))
        };
        self.documents_state.select(selected);
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            PaneFocus::Documents => PaneFocus::Inspector,
            PaneFocus::Inspector => PaneFocus::Documents,
        };
    }

    pub fn scroll_inspector_down(&mut self) -> bool {
        let max_scroll = self.max_inspector_scroll();
        if self.inspector_scroll >= max_scroll {
            return false;
        }
        self.inspector_scroll = (self.inspector_scroll + 1).min(max_scroll);
        true
    }

    pub fn scroll_inspector_up(&mut self) -> bool {
        if self.inspector_scroll == 0 {
            return false;
        }
        self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
        true
    }

    pub fn scroll_inspector_page_down(&mut self) -> bool {
        let max_scroll = self.max_inspector_scroll();
        if self.inspector_scroll >= max_scroll {
            return false;
        }
        let page = self.inspector_viewport_height.max(1);
        self.inspector_scroll = (self.inspector_scroll + page).min(max_scroll);
        true
    }

    pub fn scroll_inspector_page_up(&mut self) -> bool {
        if self.inspector_scroll == 0 {
            return false;
        }
        let page = self.inspector_viewport_height.max(1);
        self.inspector_scroll = self.inspector_scroll.saturating_sub(page);
        true
    }

    pub fn scroll_inspector_top(&mut self) -> bool {
        if self.inspector_scroll == 0 {
            return false;
        }
        self.inspector_scroll = 0;
        true
    }

    pub fn scroll_inspector_bottom(&mut self) -> bool {
        let max_scroll = self.max_inspector_scroll();
        if self.inspector_scroll == max_scroll {
            return false;
        }
        self.inspector_scroll = max_scroll;
        true
    }

    fn max_inspector_scroll(&self) -> usize {
        let line_count = self
            .current_inspector()
            .map(|inspector| inspector.text.lines().count())
            .unwrap_or(1);
        line_count.saturating_sub(self.inspector_viewport_height.max(1))
    }
}

pub fn run_tui<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    snapshot: DashboardSnapshot,
    security_receiver: Receiver<Vec<SecurityFinding>>,
) -> Result<(), AppError> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = TuiApp::from_snapshot(snapshot);
    let (event_sender, event_receiver) = mpsc::channel();

    spawn_documents_loader(client.clone(), event_sender.clone());
    spawn_latest_task_loader(client.clone(), event_sender.clone());

    let result = run_event_loop(
        &mut terminal,
        &mut app,
        client,
        security_receiver,
        event_sender,
        event_receiver,
    );

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_event_loop<B: Backend, T: Transport + Clone + Send + 'static>(
    terminal: &mut Terminal<B>,
    app: &mut TuiApp,
    client: ApiClient<T>,
    security_receiver: Receiver<Vec<SecurityFinding>>,
    event_sender: Sender<TuiEvent>,
    event_receiver: Receiver<TuiEvent>,
) -> Result<(), AppError> {
    loop {
        while let Ok(findings) = security_receiver.try_recv() {
            app.on_security_update(findings);
        }

        while let Ok(event) = event_receiver.try_recv() {
            match event {
                TuiEvent::DocumentsPage {
                    documents,
                    total,
                    done,
                } => {
                    if let Some(document_id) = app.on_documents_page(documents, total, done) {
                        spawn_inspector_loader(client.clone(), event_sender.clone(), document_id);
                    }
                }
                TuiEvent::DocumentsError(error) => app.on_loader_error(error),
                TuiEvent::Inspector {
                    document_id,
                    result,
                } => match result {
                    Ok(inspector) => app.on_inspector_loaded(document_id, inspector),
                    Err(error) => app.on_loader_error(error),
                },
                TuiEvent::LatestTask(result) => match result {
                    Ok(task) => app.latest_task = task,
                    Err(error) => app.on_loader_error(error),
                },
            }
        }

        terminal.draw(|frame| draw(frame, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let moved = match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Tab => {
                        app.toggle_focus();
                        false
                    }
                    KeyCode::Char('j') | KeyCode::Down => match app.focus {
                        PaneFocus::Documents => app.select_next(),
                        PaneFocus::Inspector => app.scroll_inspector_down(),
                    },
                    KeyCode::Char('k') | KeyCode::Up => match app.focus {
                        PaneFocus::Documents => app.select_previous(),
                        PaneFocus::Inspector => app.scroll_inspector_up(),
                    },
                    KeyCode::PageDown => match app.focus {
                        PaneFocus::Documents => app.select_page_down(),
                        PaneFocus::Inspector => app.scroll_inspector_page_down(),
                    },
                    KeyCode::PageUp => match app.focus {
                        PaneFocus::Documents => app.select_page_up(),
                        PaneFocus::Inspector => app.scroll_inspector_page_up(),
                    },
                    KeyCode::Char('g') => match app.focus {
                        PaneFocus::Documents => app.select_first(),
                        PaneFocus::Inspector => app.scroll_inspector_top(),
                    },
                    KeyCode::Char('G') => match app.focus {
                        PaneFocus::Documents => app.select_last(),
                        PaneFocus::Inspector => app.scroll_inspector_bottom(),
                    },
                    KeyCode::Char('r') => {
                        app.request_documents_reload();
                        spawn_documents_loader(client.clone(), event_sender.clone());
                        spawn_latest_task_loader(client.clone(), event_sender.clone());
                        false
                    }
                    _ => false,
                };

                if moved {
                    if let Some(document_id) = app.maybe_request_inspector() {
                        spawn_inspector_loader(client.clone(), event_sender.clone(), document_id);
                    }
                }
            }
        }
    }
}

fn spawn_documents_loader<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    event_sender: Sender<TuiEvent>,
) {
    std::thread::spawn(move || {
        let mut query = DocumentQuery::new();
        query.page_size = 25;
        query.page = 1;

        loop {
            match list_documents(&client, &query) {
                Ok(response) => {
                    let documents = document_summaries_from_response(&response);
                    let total = response
                        .get("count")
                        .and_then(serde_json::Value::as_u64)
                        .map(|value| value as usize);
                    let done = next_page_url_from_response(&response).is_none();

                    if event_sender
                        .send(TuiEvent::DocumentsPage {
                            documents,
                            total,
                            done,
                        })
                        .is_err()
                    {
                        break;
                    }

                    if done {
                        break;
                    }

                    query.page += 1;
                }
                Err(error) => {
                    let _ = event_sender.send(TuiEvent::DocumentsError(error.to_string()));
                    break;
                }
            }
        }
    });
}

fn spawn_inspector_loader<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    event_sender: Sender<TuiEvent>,
    document_id: u64,
) {
    std::thread::spawn(move || {
        let result =
            get_document_inspector(&client, document_id).map_err(|error| error.to_string());
        let _ = event_sender.send(TuiEvent::Inspector {
            document_id,
            result,
        });
    });
}

fn spawn_latest_task_loader<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    event_sender: Sender<TuiEvent>,
) {
    std::thread::spawn(move || {
        let result = list_tasks(&client)
            .map(|tasks| latest_task_summary(&tasks))
            .map_err(|error| error.to_string());
        let _ = event_sender.send(TuiEvent::LatestTask(result));
    });
}

pub fn draw(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(6),
        ])
        .split(frame.area());
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
        .split(rows[1]);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "paperless-cli",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("Document browser", Style::default().fg(Color::Magenta)),
        Span::raw("  "),
        Span::styled(
            match app.document_total {
                Some(total) if app.loading_documents => {
                    format!("{}/{} docs", app.loaded_document_count, total)
                }
                Some(total) => format!("{} docs", total),
                None if app.loading_documents => {
                    format!("{} docs loading", app.loaded_document_count)
                }
                None => format!("{} docs", app.loaded_document_count),
            },
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(title, rows[0]);

    let document_items = if app.loading_documents && app.documents.is_empty() {
        vec![ListItem::new("Loading first page…")]
    } else if app.documents.is_empty() {
        vec![ListItem::new("No documents loaded.")]
    } else {
        let mut items = app
            .documents
            .iter()
            .map(|document| {
                ListItem::new(format!(
                    "{:>5}  {}  {}",
                    document.id, document.created, document.title
                ))
            })
            .collect::<Vec<_>>();
        if app.loading_documents {
            items.push(ListItem::new("… loading more"));
        }
        items
    };
    let documents_block = Block::default().borders(Borders::ALL).title("Documents");
    let documents_block = if app.focus == PaneFocus::Documents {
        documents_block.border_style(Style::default().fg(Color::Cyan))
    } else {
        documents_block
    };
    let documents_inner = documents_block.inner(body[0]);
    frame.render_widget(documents_block, body[0]);

    if documents_inner.width > 0 && documents_inner.height > 0 {
        app.documents_viewport_height = documents_inner.height as usize;
        let document_columns = if documents_inner.width > 2 {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(documents_inner)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(1)])
                .split(documents_inner)
        };

        let documents = List::new(document_items)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(documents, document_columns[0], &mut app.documents_state);

        if document_columns.len() > 1 {
            let content_length = app.documents.len() + usize::from(app.loading_documents);
            let viewport_height = app.documents_viewport_height;
            let mut scrollbar_state = ScrollbarState::new(content_length.max(1))
                .position(app.documents_state.offset())
                .viewport_content_length(viewport_height.max(1));
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("│"))
                    .thumb_symbol("█")
                    .track_style(Style::default().fg(Color::DarkGray))
                    .thumb_style(Style::default().fg(Color::Cyan)),
                document_columns[1].inner(Margin {
                    vertical: 0,
                    horizontal: 0,
                }),
                &mut scrollbar_state,
            );
        }
    } else {
        app.documents_viewport_height = 1;
    }

    let inspector_title = if app.loading_inspector {
        "Inspector · loading"
    } else {
        "Inspector"
    };
    let inspector_body = if let Some(inspector) = app.current_inspector() {
        inspector.text.clone()
    } else if app.loading_documents {
        "Loading document list…".to_string()
    } else if app.documents.is_empty() {
        "No document selected.".to_string()
    } else {
        "Loading document text…".to_string()
    };
    app.inspector_viewport_height = body[1].height.saturating_sub(2) as usize;
    let detail = Paragraph::new(inspector_body)
        .scroll((app.inspector_scroll as u16, 0))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if app.focus == PaneFocus::Inspector {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                })
                .title(inspector_title),
        );
    frame.render_widget(detail, body[1]);

    let metadata = Paragraph::new(
        app.selected_metadata()
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>(),
    )
    .wrap(Wrap { trim: true })
    .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(metadata, rows[2]);
}

fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Critical => "critical",
        Severity::High => "high",
        Severity::Medium => "medium",
        Severity::Low => "low",
    }
}
