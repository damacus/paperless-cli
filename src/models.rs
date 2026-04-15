use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Document {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub tags: Vec<i64>,
    #[serde(default)]
    pub document_type: Option<i64>,
    #[serde(default)]
    pub correspondent: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SimpleItem {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Task {
    pub id: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub task_file_name: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Paginated<T> {
    pub count: usize,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub previous: Option<String>,
    #[serde(default)]
    pub results: Vec<T>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionState {
    #[serde(default)]
    pub last_query: String,
    #[serde(default)]
    pub selected_docs: Vec<i64>,
    #[serde(default)]
    pub history: Vec<String>,
}

impl SessionState {
    pub fn add_history(&mut self, command: impl Into<String>) {
        self.history.push(command.into());
        if self.history.len() > 500 {
            let split_index = self.history.len() - 500;
            self.history.drain(0..split_index);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_docs.clear();
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DownloadResult {
    pub doc_id: i64,
    pub path: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DashboardData {
    pub connected: bool,
    pub url: Option<String>,
    pub documents: Vec<Document>,
    pub tasks: Vec<Task>,
    pub tags: Vec<SimpleItem>,
    pub message: String,
}
