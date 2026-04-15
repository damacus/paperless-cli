use std::fmt;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SecurityFinding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub remediation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityAgentProfile {
    pub name: String,
    pub model: String,
    pub operating_guide: String,
}

impl SecurityAgentProfile {
    pub fn security_reviewer() -> Self {
        Self {
            name: "security-reviewer".to_string(),
            model: "gpt-5.4".to_string(),
            operating_guide:
                "Scope, scan, review, classify, and report. Check auth first and redact secrets."
                    .to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuditState {
    pub base_url: Option<String>,
    pub config_permissions_restricted: bool,
    pub last_download_path: Option<String>,
}

impl AuditState {
    pub fn new(
        base_url: Option<String>,
        config_permissions_restricted: bool,
        last_download_path: Option<String>,
    ) -> Self {
        Self {
            base_url,
            config_permissions_restricted,
            last_download_path,
        }
    }
}

pub type SharedAuditState = Arc<Mutex<AuditState>>;

pub struct SecurityAuditor {
    profile: SecurityAgentProfile,
    interval: Duration,
}

impl fmt::Debug for SecurityAuditor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecurityAuditor")
            .field("profile", &self.profile)
            .field("interval", &self.interval)
            .finish()
    }
}

impl SecurityAuditor {
    pub fn new(profile: SecurityAgentProfile, interval: Duration) -> Self {
        Self { profile, interval }
    }

    pub fn profile(&self) -> &SecurityAgentProfile {
        &self.profile
    }

    pub fn spawn(&self, shared_state: SharedAuditState) -> Receiver<Vec<SecurityFinding>> {
        let (sender, receiver) = mpsc::channel();
        let profile = self.profile.clone();
        let interval = self.interval;

        thread::spawn(move || loop {
            let state = shared_state.lock().map(|guard| guard.clone());
            let findings = match state {
                Ok(state) => review_state(&profile, &state),
                Err(_) => vec![SecurityFinding {
                    severity: Severity::High,
                    title: "Security monitor failed".to_string(),
                    detail: "The security reviewer lost access to shared state.".to_string(),
                    remediation: "Recreate the shared audit state and restart the TUI.".to_string(),
                }],
            };

            if sender.send(findings).is_err() {
                break;
            }

            thread::sleep(interval);
        });

        receiver
    }

    pub fn review_once(&self, state: &AuditState) -> Vec<SecurityFinding> {
        review_state(&self.profile, state)
    }
}

fn review_state(profile: &SecurityAgentProfile, state: &AuditState) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    if profile.model != "gpt-5.4" {
        findings.push(SecurityFinding {
            severity: Severity::High,
            title: "Security reviewer downgraded".to_string(),
            detail: format!(
                "The configured security agent model is `{}` instead of `gpt-5.4`.",
                profile.model
            ),
            remediation: "Restore the security agent profile to gpt-5.4.".to_string(),
        });
    }

    if let Some(url) = &state.base_url {
        if url.starts_with("http://") && !url.contains("127.0.0.1") && !url.contains("localhost") {
            findings.push(SecurityFinding {
                severity: Severity::Medium,
                title: "Paperless API is using plain HTTP".to_string(),
                detail: format!("Traffic to `{url}` may expose tokens in transit."),
                remediation: "Use HTTPS for remote Paperless servers.".to_string(),
            });
        }
    }

    if !state.config_permissions_restricted {
        findings.push(SecurityFinding {
            severity: Severity::High,
            title: "Config permissions are too broad".to_string(),
            detail: "The config file is readable by users other than the owner.".to_string(),
            remediation: "Rewrite the config file and enforce 0600 permissions.".to_string(),
        });
    }

    if let Some(path) = &state.last_download_path {
        if path.contains("..") {
            findings.push(SecurityFinding {
                severity: Severity::High,
                title: "Suspicious download path observed".to_string(),
                detail: format!("The last download path `{path}` contains parent traversal."),
                remediation: "Reject the filename and re-download into a sanitized path."
                    .to_string(),
            });
        }
    }

    findings
}
