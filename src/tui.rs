use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
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
use serde_json::Value;

use crate::api::{ApiClient, Transport};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::security::{SecurityFinding, Severity};
use crate::services::{
    document_summaries_from_response, fetch_latest_task, get_document_inspector, list_documents,
    next_page_url_from_response, status, DashboardSnapshot, DocumentInspector, DocumentQuery,
    DocumentSummary, TaskSummary,
};

const INSPECTOR_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_INSPECTOR_REQUESTS: usize = 2;

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
    inspector_order: VecDeque<u64>,
    inspector_bytes: usize,
    uncached_inspector: Option<(u64, DocumentInspector)>,
    connecting: bool,
    loading_task: bool,
    task_error: Option<String>,
}

#[derive(Debug)]
enum TuiEvent {
    Connected(Result<Value, AppError>),
    DocumentsPage {
        generation: u64,
        documents: Vec<DocumentSummary>,
        total: Option<usize>,
        done: bool,
    },
    DocumentsError {
        generation: u64,
        error: String,
    },
    Inspector {
        generation: u64,
        document_id: u64,
        result: Result<DocumentInspector, String>,
    },
    LatestTask(Result<Option<TaskSummary>, String>),
}

impl TuiApp {
    pub fn from_snapshot(snapshot: DashboardSnapshot) -> Self {
        let has_documents = !snapshot.documents.is_empty();
        let loaded_document_count = snapshot.documents.len();
        let project_summary = project_summary(&snapshot.project);

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
            inspector_order: VecDeque::new(),
            inspector_bytes: 0,
            uncached_inspector: None,
            connecting: false,
            loading_task: false,
            task_error: None,
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
        let document_id = self.current_document_id()?;
        self.inspector_cache.get(&document_id).or_else(|| {
            self.uncached_inspector
                .as_ref()
                .filter(|(id, _)| *id == document_id)
                .map(|(_, inspector)| inspector)
        })
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

        if let Some(error) = &self.task_error {
            sections.push(format!("latest task unavailable: {error}"));
        } else if let Some(task) = &self.latest_task {
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
        let selected = self.current_document_id();
        if selected == Some(document_id) {
            self.loading_inspector = false;
            self.pending_inspector_id = None;
            self.inspector_scroll = 0;
            self.uncached_inspector = None;
        }
        let bytes = inspector_size(&inspector);
        if bytes > INSPECTOR_CACHE_BYTES {
            if selected == Some(document_id) {
                self.remove_cached_inspector(document_id);
                self.uncached_inspector = Some((document_id, inspector));
            }
            return;
        }

        if let Some(selected) = selected.filter(|id| *id != document_id) {
            if self
                .inspector_cache
                .get(&selected)
                .is_some_and(|inspector| inspector_size(inspector) > INSPECTOR_CACHE_BYTES - bytes)
            {
                return;
            }
        }
        self.remove_cached_inspector(document_id);
        while self.inspector_bytes > INSPECTOR_CACHE_BYTES - bytes {
            let oldest_is_selected = self.inspector_order.front().copied() == selected;
            let victim = self.inspector_order.remove(usize::from(oldest_is_selected));
            let Some(victim) = victim else {
                return;
            };
            self.remove_inspector_bytes(victim);
        }
        self.inspector_bytes += bytes;
        self.inspector_order.push_back(document_id);
        self.inspector_cache.insert(document_id, inspector);
    }

    fn remove_cached_inspector(&mut self, document_id: u64) {
        if self.inspector_cache.contains_key(&document_id) {
            self.inspector_order.retain(|id| *id != document_id);
            self.remove_inspector_bytes(document_id);
        }
    }

    fn remove_inspector_bytes(&mut self, document_id: u64) {
        if let Some(inspector) = self.inspector_cache.remove(&document_id) {
            self.inspector_bytes = self
                .inspector_bytes
                .saturating_sub(inspector_size(&inspector));
        }
    }

    pub fn on_loader_error(&mut self, message: String) {
        self.loading_documents = false;
        self.loading_inspector = false;
        self.pending_inspector_id = None;
        self.project_summary = format!("error {message}");
    }

    fn on_latest_task(&mut self, result: Result<Option<TaskSummary>, String>) {
        self.loading_task = false;
        match result {
            Ok(task) => {
                self.latest_task = task;
                self.task_error = None;
            }
            Err(error) => self.task_error = Some(error),
        }
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
        self.loading_inspector = false;
        self.inspector_cache.clear();
        self.inspector_order.clear();
        self.inspector_bytes = 0;
        self.uncached_inspector = None;
    }

    pub fn maybe_request_inspector(&mut self) -> Option<u64> {
        let document_id = self.current_document_id()?;
        if self.current_inspector().is_some() || self.pending_inspector_id == Some(document_id) {
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
        if index != self.selected_document {
            self.pending_inspector_id = None;
            self.uncached_inspector = None;
            self.loading_inspector = false;
        }
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
    run_tui_inner(
        client,
        TuiApp::from_snapshot(snapshot),
        None,
        security_receiver,
    )
}

pub fn run_connecting_tui<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    config: AppConfig,
    security: Vec<SecurityFinding>,
    security_receiver: Receiver<Vec<SecurityFinding>>,
) -> Result<(), AppError> {
    let mut app = TuiApp::from_snapshot(DashboardSnapshot {
        project: Value::Null,
        documents: Vec::new(),
        latest_task: None,
        security,
    });
    app.project_summary = format!("connecting {}", config.base_url);
    app.connecting = true;
    run_tui_inner(client, app, Some(config), security_receiver)
}

fn run_tui_inner<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    mut app: TuiApp,
    config: Option<AppConfig>,
    security_receiver: Receiver<Vec<SecurityFinding>>,
) -> Result<(), AppError> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let (event_sender, event_receiver) = mpsc::channel();

    let result = (|| {
        terminal.draw(|frame| draw(frame, &mut app))?;
        let documents_loader = DocumentsLoader::new(client.clone(), event_sender.clone());
        if let Some(config) = config {
            let client = client.clone();
            let events = event_sender.clone();
            std::thread::spawn(move || {
                let _ = events.send(TuiEvent::Connected(status(&client, &config)));
            });
        } else {
            documents_loader.reload();
        }

        run_event_loop(
            &mut terminal,
            &mut app,
            client,
            security_receiver,
            event_sender,
            event_receiver,
            documents_loader,
        )
    })();

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
    documents_loader: DocumentsLoader,
) -> Result<(), AppError> {
    let mut redraw = false;
    let mut active_inspectors = HashSet::new();
    let mut generation = documents_loader.generation();
    loop {
        while let Ok(findings) = security_receiver.try_recv() {
            if app.security != findings {
                app.on_security_update(findings);
                redraw = true;
            }
        }

        while let Ok(event) = event_receiver.try_recv() {
            redraw = true;
            match event {
                TuiEvent::Connected(result) => {
                    app.project_summary = project_summary(&result?);
                    app.connecting = false;
                    app.loading_task = true;
                    generation = documents_loader.reload();
                    spawn_latest_task_loader(client.clone(), event_sender.clone());
                }
                TuiEvent::DocumentsPage {
                    generation: loaded_generation,
                    documents,
                    total,
                    done,
                } if loaded_generation == generation => {
                    app.on_documents_page(documents, total, done);
                    request_selected_inspector(
                        app,
                        &client,
                        &event_sender,
                        generation,
                        &mut active_inspectors,
                    );
                }
                TuiEvent::DocumentsError {
                    generation: loaded_generation,
                    error,
                } if loaded_generation == generation => {
                    app.loading_documents = false;
                    app.project_summary = format!("error {error}");
                }
                TuiEvent::DocumentsPage { .. } | TuiEvent::DocumentsError { .. } => {}
                TuiEvent::Inspector {
                    generation: loaded_generation,
                    document_id,
                    result,
                } => {
                    active_inspectors.remove(&(loaded_generation, document_id));
                    if loaded_generation == generation {
                        match result {
                            Ok(inspector) => app.on_inspector_loaded(document_id, inspector),
                            Err(error) if app.current_document_id() == Some(document_id) => {
                                app.loading_inspector = false;
                                app.project_summary = format!("error {error}");
                            }
                            Err(_) => {}
                        }
                    }
                    request_selected_inspector(
                        app,
                        &client,
                        &event_sender,
                        generation,
                        &mut active_inspectors,
                    );
                }
                TuiEvent::LatestTask(result) => app.on_latest_task(result),
            }
        }

        if redraw {
            terminal
                .draw(|frame| draw(frame, app))
                .map_err(|error| AppError::Message(error.to_string()))?;
            redraw = false;
        }

        if event::poll(Duration::from_millis(100))? {
            let event = event::read()?;
            redraw = matches!(event, Event::Resize(..));
            if let Event::Key(key) = event {
                let previous_scroll = app.inspector_scroll;
                let moved = match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Tab => {
                        app.toggle_focus();
                        redraw = true;
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
                    KeyCode::Char('r') if !app.connecting => {
                        app.request_documents_reload();
                        redraw = true;
                        generation = documents_loader.reload();
                        if !app.loading_task {
                            app.loading_task = true;
                            spawn_latest_task_loader(client.clone(), event_sender.clone());
                        }
                        false
                    }
                    _ => false,
                };
                redraw |= moved || previous_scroll != app.inspector_scroll;

                if moved {
                    request_selected_inspector(
                        app,
                        &client,
                        &event_sender,
                        generation,
                        &mut active_inspectors,
                    );
                }
            }
        }
    }
}

fn project_summary(project: &Value) -> String {
    project
        .get("url")
        .or_else(|| {
            project
                .get("response")
                .and_then(|value| value.get("version"))
        })
        .and_then(Value::as_str)
        .map_or_else(
            || "connected".to_string(),
            |value| format!("connected {value}"),
        )
}

struct DocumentsLoader {
    generation: Arc<Mutex<u64>>,
    requests: SyncSender<()>,
}

impl DocumentsLoader {
    fn new<T: Transport + Send + 'static>(client: ApiClient<T>, events: Sender<TuiEvent>) -> Self {
        let generation = Arc::new(Mutex::new(0));
        let current = Arc::clone(&generation);
        let (requests, pending) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut last = 0;
            while pending.recv().is_ok() {
                let requested = document_generation(&current);
                if requested == 0 {
                    break;
                }
                if requested != last {
                    last = requested;
                    load_document_pages(&client, &events, requested, &current);
                }
            }
        });
        Self {
            generation,
            requests,
        }
    }

    fn generation(&self) -> u64 {
        document_generation(&self.generation)
    }

    fn reload(&self) -> u64 {
        let mut generation = self
            .generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *generation = generation.wrapping_add(1).max(1);
        // The worker reads the generation under the same lock, even if this
        // wakeup coalesces with a notification that is already queued.
        let _ = self.requests.try_send(());
        *generation
    }
}

impl Drop for DocumentsLoader {
    fn drop(&mut self) {
        *self
            .generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = 0;
    }
}

fn document_generation(current: &Mutex<u64>) -> u64 {
    *current.lock().unwrap_or_else(PoisonError::into_inner)
}

fn load_document_pages<T: Transport>(
    client: &ApiClient<T>,
    events: &Sender<TuiEvent>,
    generation: u64,
    current: &Mutex<u64>,
) {
    let mut query = DocumentQuery::new();
    while generation == document_generation(current) {
        let response = list_documents(client, &query);
        if generation != document_generation(current) {
            break;
        }

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let _ = events.send(TuiEvent::DocumentsError {
                    generation,
                    error: error.to_string(),
                });
                break;
            }
        };
        let done = next_page_url_from_response(&response).is_none();
        let total = response
            .get("count")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let documents = document_summaries_from_response(&response);
        if events
            .send(TuiEvent::DocumentsPage {
                generation,
                documents,
                total,
                done,
            })
            .is_err()
            || done
        {
            break;
        }
        query.page += 1;
    }
}

fn request_selected_inspector<T: Transport + Clone + Send + 'static>(
    app: &mut TuiApp,
    client: &ApiClient<T>,
    event_sender: &Sender<TuiEvent>,
    generation: u64,
    active: &mut HashSet<(u64, u64)>,
) {
    let Some(document_id) = app.current_document_id() else {
        return;
    };
    if app.current_inspector().is_some() {
        app.loading_inspector = false;
    } else if active.contains(&(generation, document_id)) {
        app.pending_inspector_id = Some(document_id);
        app.loading_inspector = true;
    } else if active.len() == MAX_INSPECTOR_REQUESTS {
        app.loading_inspector = true;
    } else if let Some(document_id) = app.maybe_request_inspector() {
        active.insert((generation, document_id));
        spawn_inspector_loader(
            client.clone(),
            event_sender.clone(),
            generation,
            document_id,
        );
    }
}

fn spawn_inspector_loader<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    event_sender: Sender<TuiEvent>,
    generation: u64,
    document_id: u64,
) {
    std::thread::spawn(move || {
        let result =
            get_document_inspector(&client, document_id).map_err(|error| error.to_string());
        let _ = event_sender.send(TuiEvent::Inspector {
            generation,
            document_id,
            result,
        });
    });
}

fn inspector_size(inspector: &DocumentInspector) -> usize {
    std::mem::size_of::<DocumentInspector>()
        + inspector.title.capacity()
        + inspector.text.capacity()
        + inspector.metadata.capacity() * std::mem::size_of::<String>()
        + inspector
            .metadata
            .iter()
            .map(String::capacity)
            .sum::<usize>()
}

fn spawn_latest_task_loader<T: Transport + Clone + Send + 'static>(
    client: ApiClient<T>,
    event_sender: Sender<TuiEvent>,
) {
    std::thread::spawn(move || {
        let result = fetch_latest_task(&client).map_err(|error| error.to_string());
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

        let selected = app
            .documents_state
            .selected()
            .filter(|_| !app.documents.is_empty())
            .map(|index| index.min(app.documents.len() - 1));
        let (offset, document_items) = visible_document_items(app, selected);
        let mut viewport = ListState::default().with_selected(selected.map(|index| index - offset));
        let documents = List::new(document_items)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(documents, document_columns[0], &mut viewport);
        *app.documents_state.offset_mut() = offset + viewport.offset();

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
    app.inspector_viewport_height = body[1].height.saturating_sub(2) as usize;
    let inspector_body = if let Some(inspector) = app.current_inspector() {
        inspector.text.as_str()
    } else if app.loading_documents {
        "Loading document list…"
    } else if app.documents.is_empty() {
        "No document selected."
    } else {
        "Loading document text…"
    };
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

fn visible_document_items(
    app: &TuiApp,
    selected: Option<usize>,
) -> (usize, Vec<ListItem<'static>>) {
    if app.documents.is_empty() {
        let message = if app.loading_documents {
            "Loading first page…"
        } else {
            "No documents loaded."
        };
        return (0, vec![ListItem::new(message)]);
    }

    let item_count = app.documents.len() + usize::from(app.loading_documents);
    let height = app.documents_viewport_height;
    let mut offset = app.documents_state.offset().min(item_count - 1);
    if let Some(selected) = selected {
        // Each item needs at least one row. Anything earlier cannot fit alongside
        // the selection. Ratatui accounts for multiline items inside this window.
        offset = offset.clamp(selected.saturating_sub(height - 1), selected);
    }
    let end = offset.saturating_add(height);
    let mut items = app
        .documents
        .iter()
        .skip(offset)
        .take(height)
        .map(|document| {
            ListItem::new(format!(
                "{:>5}  {}  {}",
                document.id, document.created, document.title
            ))
        })
        .collect::<Vec<_>>();
    if app.loading_documents && end > app.documents.len() {
        items.push(ListItem::new("… loading more"));
    }
    (offset, items)
}

fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Critical => "critical",
        Severity::High => "high",
        Severity::Medium => "medium",
        Severity::Low => "low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_task_error_leaves_document_loading_independent() {
        let mut app = TuiApp::from_snapshot(DashboardSnapshot {
            project: serde_json::json!({"url": "https://paperless.example.com"}),
            documents: vec![],
            latest_task: None,
            security: vec![],
        });
        app.loading_task = true;

        app.on_latest_task(Err("HTTP 403 Forbidden".to_string()));
        assert!(app.loading_documents);
        assert!(!app.loading_task);
        assert!(app
            .selected_metadata()
            .iter()
            .any(|line| { line == "latest task unavailable: HTTP 403 Forbidden" }));
        assert_eq!(
            app.on_documents_page(
                vec![DocumentSummary {
                    id: 42,
                    title: "Invoice".to_string(),
                    created: "2026-11-18".to_string(),
                }],
                Some(1),
                true,
            ),
            Some(42)
        );
        assert_eq!(app.current_document_id(), Some(42));
        assert!(!app.loading_documents);

        app.on_latest_task(Ok(None));
        assert!(app.task_error.is_none());
    }
}

#[cfg(test)]
mod loader_tests {
    use super::*;
    use crate::api::{Endpoint, RequestData, ResponseData};
    use serde_json::json;

    type PendingResponse = (RequestData, Sender<Value>);

    #[derive(Clone)]
    struct ControlledTransport(Sender<PendingResponse>);

    impl Transport for ControlledTransport {
        fn send(&self, request: RequestData) -> Result<ResponseData, AppError> {
            let (respond, response) = mpsc::channel();
            self.0.send((request, respond)).unwrap();
            let data = response.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(ResponseData {
                status: 200,
                url: "http://localhost/api/documents/".to_string(),
                headers: Default::default(),
                body: serde_json::to_vec(&data).unwrap(),
            })
        }
    }

    fn test_app(count: u64) -> TuiApp {
        TuiApp::from_snapshot(DashboardSnapshot {
            project: Value::Null,
            documents: (1..=count)
                .map(|id| DocumentSummary {
                    id,
                    title: format!("Document {id}"),
                    created: String::new(),
                })
                .collect(),
            latest_task: None,
            security: vec![],
        })
    }

    fn inspector(id: u64, bytes: usize) -> DocumentInspector {
        DocumentInspector {
            id,
            title: format!("Document {id}"),
            text: "a".repeat(bytes),
            metadata: vec![],
        }
    }

    #[test]
    fn inspector_cache_caps_retained_text_and_preserves_recent_visits() {
        let mut app = test_app(1000);
        for id in 1..=1000 {
            app.on_inspector_loaded(id, inspector(id, 100 * 1024));
            if id < 1000 {
                app.select_next();
            }
        }
        let retained: usize = app.inspector_cache.values().map(inspector_size).sum();
        assert_eq!(app.inspector_bytes, retained);
        assert!(retained <= INSPECTOR_CACHE_BYTES);
        assert!(!app.inspector_cache.contains_key(&1));
        assert!(app.select_previous());
        assert_eq!(app.current_inspector().map(|value| value.id), Some(999));
        assert_eq!(app.maybe_request_inspector(), None);
        assert!(app.select_first());
        assert_eq!(app.maybe_request_inspector(), Some(1));
    }

    #[test]
    fn oversized_inspector_is_viewable_and_dropped_on_navigation() {
        let mut app = test_app(2);
        app.on_inspector_loaded(1, inspector(1, INSPECTOR_CACHE_BYTES + 1));
        assert_eq!(app.current_inspector().map(|value| value.id), Some(1));
        assert!(app.inspector_cache.is_empty());
        assert!(app.select_next());
        assert!(app.uncached_inspector.is_none());
    }

    #[test]
    fn a_late_inspector_that_cannot_fit_leaves_the_cache_untouched() {
        let mut app = test_app(3);
        app.on_inspector_loaded(1, inspector(1, INSPECTOR_CACHE_BYTES - 2048));
        app.on_inspector_loaded(2, inspector(2, 512));
        let previous_size = app.inspector_bytes;

        app.on_inspector_loaded(3, inspector(3, 4096));

        assert_eq!(app.inspector_bytes, previous_size);
        assert!(app.inspector_cache.contains_key(&2));
        assert!(!app.inspector_cache.contains_key(&3));
    }

    #[test]
    fn stale_inspector_does_not_reset_current_selection_or_scroll() {
        let mut app = test_app(2);
        assert_eq!(app.maybe_request_inspector(), Some(1));
        assert!(app.select_next());
        assert_eq!(app.maybe_request_inspector(), Some(2));
        app.on_inspector_loaded(1, inspector(1, 100));
        assert!(app.loading_inspector);
        assert!(app.current_inspector().is_none());
        app.on_inspector_loaded(2, inspector(2, 100));
        app.inspector_scroll = 5;
        app.on_inspector_loaded(1, inspector(1, 200));
        assert_eq!(app.inspector_scroll, 5);
        assert_eq!(app.current_inspector().map(|value| value.id), Some(2));
    }

    #[test]
    fn reloads_coalesce_while_an_old_page_is_in_flight() {
        let (requests, started) = mpsc::channel();
        let (events, received) = mpsc::channel();
        let loader = DocumentsLoader::new(ApiClient::new(ControlledTransport(requests)), events);
        let old = loader.reload();
        let (old_request, respond_to_old) = started.recv_timeout(Duration::from_secs(2)).unwrap();
        loader.reload();
        let latest = loader.reload();
        assert_ne!(old, latest);
        assert!(started.try_recv().is_err());
        respond_to_old
            .send(json!({
                "count": 25, "results": [{"id": 88}], "next": "/api/documents/?page=2"
            }))
            .unwrap();
        let (latest_request, respond_to_latest) =
            started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(old_request.query, latest_request.query);
        respond_to_latest
            .send(json!({"count": 1, "results": [{"id": 42}]}))
            .unwrap();
        match received.recv_timeout(Duration::from_secs(2)).unwrap() {
            TuiEvent::DocumentsPage {
                generation,
                documents,
                done,
                ..
            } => {
                assert_eq!(generation, latest);
                assert_eq!(documents.first().map(|value| value.id), Some(42));
                assert!(done);
            }
            event => panic!("unexpected event: {event:?}"),
        }
        drop(loader);
        assert!(started.recv_timeout(Duration::from_secs(2)).is_err());
    }

    #[test]
    fn inspector_requests_stop_at_two_then_fetch_current_selection() {
        let mut app = test_app(3);
        let (requests, started) = mpsc::channel();
        let (events, received) = mpsc::channel();
        let client = ApiClient::new(ControlledTransport(requests));
        let mut active = HashSet::new();
        let generation = 1;

        request_selected_inspector(&mut app, &client, &events, generation, &mut active);
        let (first, release_first) = started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            first.endpoint,
            Endpoint::Relative("documents/1/".to_string())
        );
        assert!(app.select_next());
        request_selected_inspector(&mut app, &client, &events, generation, &mut active);
        let (second, release_second) = started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            second.endpoint,
            Endpoint::Relative("documents/2/".to_string())
        );
        assert!(app.select_next());
        request_selected_inspector(&mut app, &client, &events, generation, &mut active);
        assert_eq!(active.len(), 2);
        assert!(started.try_recv().is_err());
        release_second
            .send(json!({"id": 2, "content": "second"}))
            .unwrap();
        if let TuiEvent::Inspector {
            document_id,
            result,
            ..
        } = received.recv_timeout(Duration::from_secs(2)).unwrap()
        {
            active.remove(&(generation, document_id));
            app.on_inspector_loaded(document_id, result.unwrap());
        } else {
            panic!("expected inspector response");
        }
        request_selected_inspector(&mut app, &client, &events, generation, &mut active);
        let (third, release_third) = started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            third.endpoint,
            Endpoint::Relative("documents/3/".to_string())
        );
        release_first.send(json!({"id": 1})).unwrap();
        release_third.send(json!({"id": 3})).unwrap();
        assert_eq!(active.len(), 2);
    }
}
