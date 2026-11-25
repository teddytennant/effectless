//! What a provider adapter emits (spec/07 §2.4) and what it reports about itself.

use serde::{Deserialize, Serialize};

use crate::model::*;

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderEvent {
    SessionUpdated {
        status: SessionStatus,
        model: Option<String>,
        startup_phase: Option<String>,
        last_error: Option<String>,
    },
    BindingUpdated {
        native_thread_id: Option<String>,
        status: ProviderThreadStatus,
        context_usage: Option<ContextUsage>,
    },
    TurnStarted {
        run_id: String,
        native_turn_id: Option<String>,
    },
    TurnUsage {
        run_id: String,
        usage: TurnTokenUsage,
        model: Option<String>,
    },
    /// Full snapshot of an item, idempotent by `item.id`.
    ItemUpserted(TurnItem),
    /// Live command output; not persisted.
    CommandOutputDelta {
        item_id: String,
        chunk: String,
    },
    RequestOpened(RuntimeRequest),
    RequestClosed {
        request_id: String,
        status: RequestStatus,
    },
    PlanUpdated {
        plan_id: String,
        markdown: String,
        streaming: bool,
    },
    TodoUpdated {
        plan_id: String,
        steps: Vec<PlanStep>,
        explanation: Option<String>,
    },
    /// (option id, from, to), e.g. Claude leaving plan mode.
    ModelSelectionChanged {
        changes: Vec<(String, String, String)>,
    },
    TurnTerminal {
        run_id: String,
        status: TerminalStatus,
        failure: Option<ProviderFailure>,
        reason: Option<String>,
        broken: bool,
    },
}

/// One selectable value of a model option (reasoning effort, thinking, ...).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptionChoice {
    pub value: String,
    pub label: String,
    #[serde(default)]
    pub is_default: bool,
    /// Tooltip text, e.g. Codex effort descriptions or Claude's Ultracode note.
    #[serde(default)]
    pub description: Option<String>,
    /// Sent as a prompt prefix rather than an option (Claude `ultrathink`).
    #[serde(default)]
    pub prompt_injected: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptionDescriptor {
    /// e.g. "reasoningEffort", "fastMode", "thinking"
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub choices: Vec<ModelOptionChoice>,
    /// true for boolean toggles (choices empty)
    #[serde(default)]
    pub boolean: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    /// value sent to the provider
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub short_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_custom: bool,
    /// Shown behind "Show more" in the picker (manifest status `legacy`).
    #[serde(default)]
    pub is_legacy: bool,
    #[serde(default)]
    pub options: Vec<ModelOptionDescriptor>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthState {
    Unknown,
    NotInstalled,
    SignedOut,
    SignedIn,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub instance_id: String,
    pub driver: Driver,
    pub display_name: String,
    pub enabled: bool,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub auth: ProviderAuthState,
    pub account_label: Option<String>,
    pub models: Vec<ProviderModel>,
    pub capabilities: Capabilities,
    pub message: Option<String>,
}
