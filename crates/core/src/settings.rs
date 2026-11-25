//! Settings, client settings and keybindings (spec/06 §16, §11.3; spec/02 §5).
//!
//! JSON keys and defaults follow T3 Code (`packages/contracts/src/settings.ts`) so a user can
//! copy `~/.t3/userdata/{settings,client-settings,keybindings}.json` into `~/.tern/userdata`.
//! `settings.json` is sparse (only values that differ from the default are written);
//! `client-settings.json` holds the whole object; `keybindings.json` is a rule array.
//! Unknown keys are kept and written back untouched.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::{
    ModelSelection, OptionSelection, OptionValue, ProjectScript, RuntimeMode, ThreadEnvMode,
};

pub const SETTINGS_FILE: &str = "settings.json";
pub const CLIENT_SETTINGS_FILE: &str = "client-settings.json";
pub const KEYBINDINGS_FILE: &str = "keybindings.json";

// ---------------------------------------------------------------------------------------------
// enums

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $wire:literal),+ $(,)? } default $def:ident) => {
        $(#[$m])*
        #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
        pub enum $name {
            $($(#[$vm])* #[serde(rename = $wire)] $variant),+
        }
        impl Default for $name {
            fn default() -> Self {
                Self::$def
            }
        }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }
        }
    };
}

wire_enum!(NotificationMode {
    Off = "off",
    Notifications = "notifications",
    Sound = "sound",
    NotificationsAndSound = "notifications-and-sound",
} default Off);

wire_enum!(SidebarProjectGroupingMode {
    Repository = "repository",
    RepositoryPath = "repository_path",
    Separate = "separate",
} default Repository);

wire_enum!(SortOrder {
    UpdatedAt = "updated_at",
    CreatedAt = "created_at",
    Manual = "manual",
} default UpdatedAt);

wire_enum!(TimestampFormat {
    Locale = "locale",
    TwelveHour = "12-hour",
    TwentyFourHour = "24-hour",
} default Locale);

wire_enum!(SendShortcut {
    Enter = "enter",
    ModEnterMultiline = "mod-enter-multiline",
    ModEnter = "mod-enter",
} default Enter);

wire_enum!(FollowUpBehavior {
    Queue = "queue",
    Steer = "steer",
} default Steer);

wire_enum!(SideThreadOpenTarget {
    Panel = "panel",
    Main = "main",
} default Panel);

wire_enum!(ToolCallVisibility {
    ShowAll = "showAll",
    HideSuccessful = "hideSuccessful",
    HideAll = "hideAll",
    Zen = "zen",
} default ShowAll);

wire_enum!(ScreenReaderAnnouncements {
    Status = "status",
    Messages = "messages",
    Everything = "everything",
} default Status);

wire_enum!(DiffLayout {
    Stacked = "stacked",
    Split = "split",
} default Stacked);

wire_enum!(DiffColorScheme {
    RedGreen = "red-green",
    BlueOrange = "blue-orange",
} default RedGreen);

wire_enum!(ChatWidth {
    Comfortable = "comfortable",
    Wide = "wide",
    Full = "full",
} default Comfortable);

wire_enum!(HtmlPreviewWidth {
    Chat = "chat",
    Wide = "wide",
    Full = "full",
} default Chat);

wire_enum!(EffortDisplay {
    Label = "label",
    Bars = "bars",
} default Label);

wire_enum!(EnvironmentIdentificationMode {
    Artwork = "artwork",
    Pill = "pill",
    None = "none",
} default Artwork);

wire_enum!(ResponseStreamingMode {
    Turn = "turn",
    #[serde(alias = "token")]
    Paragraph = "paragraph",
} default Paragraph);

wire_enum!(BranchNamingMode {
    Static = "static",
    Semantic = "semantic",
    Custom = "custom",
} default Static);

wire_enum!(WorktreeSubmodules {
    Recursive = "recursive",
    TopLevel = "top-level",
    None = "none",
} default Recursive);

wire_enum!(PullRequestMergeMethod {
    Merge = "merge",
    Squash = "squash",
    Rebase = "rebase",
} default Squash);

wire_enum!(WorktreeKeepWhen {
    AnyLocalFiles = "any-local-files",
    UncommittedChanges = "uncommitted-changes",
    TrackedChanges = "tracked-changes",
} default UncommittedChanges);

wire_enum!(SourceControlWritingMode {
    RepoConventions = "repo_conventions",
    ConventionalCommits = "conventional_commits",
    Custom = "custom",
} default RepoConventions);

wire_enum!(BackgroundActivityProfile {
    Balanced = "balanced",
    Performance = "performance",
    BatterySaver = "battery-saver",
    Custom = "custom",
} default Balanced);

/// `confirmQuit`. Old files stored a bool: true reads as `hold`, false as `direct`.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum QuitConfirmation {
    #[serde(rename = "direct")]
    Direct,
    #[default]
    #[serde(rename = "hold")]
    Hold,
    #[serde(rename = "double-click")]
    DoubleClick,
}

impl<'de> Deserialize<'de> for QuitConfirmation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Bool(true) => Ok(Self::Hold),
            Value::Bool(false) => Ok(Self::Direct),
            Value::String(s) => match s.as_str() {
                "direct" => Ok(Self::Direct),
                "hold" => Ok(Self::Hold),
                "double-click" => Ok(Self::DoubleClick),
                other => Err(serde::de::Error::custom(format!(
                    "unknown confirmQuit {other}"
                ))),
            },
            other => Err(serde::de::Error::custom(format!("bad confirmQuit {other}"))),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// server settings (settings.json)

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SnoozePreset {
    After { minutes: u32 },
    At { day: String, time: String },
}

pub fn default_snooze_presets() -> Vec<SnoozePreset> {
    let at = |day: &str, time: &str| SnoozePreset::At {
        day: day.into(),
        time: time.into(),
    };
    vec![
        SnoozePreset::After { minutes: 60 },
        SnoozePreset::After { minutes: 180 },
        at("today", "18:00"),
        at("tomorrow", "09:00"),
        at("monday", "09:00"),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct StorageCleanup {
    pub worktree_keep_when: WorktreeKeepWhen,
    pub worktree_after_days: Option<u32>,
    pub worktree_settled_after_days: Option<u32>,
    pub worktree_on_merge: bool,
    pub worktree_on_delete: bool,
    pub worktree_unchanged: bool,
    pub browser_artifacts_after_days: Option<u32>,
    pub logs_after_days: Option<u32>,
}

impl Default for StorageCleanup {
    fn default() -> Self {
        Self {
            worktree_keep_when: WorktreeKeepWhen::UncommittedChanges,
            worktree_after_days: None,
            worktree_settled_after_days: None,
            worktree_on_merge: false,
            worktree_on_delete: false,
            worktree_unchanged: false,
            browser_artifacts_after_days: None,
            logs_after_days: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BackgroundActivity {
    pub schema_version: u32,
    pub profile: BackgroundActivityProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_profile: Option<BackgroundActivityProfile>,
    /// automaticGitFetchInterval, providerHealthRefreshInterval (ms), pauseWhen* switches.
    pub overrides: Map<String, Value>,
}

impl Default for BackgroundActivity {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profile: BackgroundActivityProfile::Balanced,
            base_profile: None,
            overrides: Map::new(),
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceControlWritingStyle {
    pub mode: SourceControlWritingMode,
    pub custom_instructions: String,
    pub follow_change_request_templates: bool,
}

impl Default for SourceControlWritingStyle {
    fn default() -> Self {
        Self {
            mode: SourceControlWritingMode::RepoConventions,
            custom_instructions: String::new(),
            follow_change_request_templates: true,
        }
    }
}

impl<'de> Deserialize<'de> for SourceControlWritingStyle {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", default)]
        struct Raw {
            mode: SourceControlWritingMode,
            custom_instructions: String,
            follow_change_request_templates: bool,
        }
        impl Default for Raw {
            fn default() -> Self {
                let d = SourceControlWritingStyle::default();
                Self {
                    mode: d.mode,
                    custom_instructions: d.custom_instructions,
                    follow_change_request_templates: d.follow_change_request_templates,
                }
            }
        }
        match Value::deserialize(d)? {
            // legacy: a bare instruction string
            Value::String(s) => Ok(Self {
                mode: if s.trim().is_empty() {
                    SourceControlWritingMode::RepoConventions
                } else {
                    SourceControlWritingMode::Custom
                },
                custom_instructions: s.trim().to_string(),
                follow_change_request_templates: true,
            }),
            v => {
                let r: Raw = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(Self {
                    mode: r.mode,
                    custom_instructions: r.custom_instructions,
                    follow_change_request_templates: r.follow_change_request_templates,
                })
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub sensitive: bool,
}

/// `providerInstances[id]` (spec/06 §10.8). `config` is kept as raw JSON so unknown drivers and
/// newer fields survive a round trip.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstanceConfig {
    pub driver: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<EnvironmentVariable>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub config: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ProviderInstanceConfig {
    fn config_str(&self, key: &str) -> Option<&str> {
        self.config
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// `config.binaryPath`, or None for "discover on PATH".
    pub fn binary_path(&self) -> Option<&str> {
        self.config_str("binaryPath")
    }

    pub fn home_path(&self) -> Option<&str> {
        self.config_str("homePath")
    }

    pub fn launch_args(&self) -> Option<&str> {
        self.config_str("launchArgs")
    }

    /// Enabled unless `enabled` or `config.enabled` is false (spec/06 §10.8).
    pub fn is_enabled(&self) -> bool {
        let cfg = self.config.get("enabled").and_then(Value::as_bool);
        if self.enabled == Some(false) || cfg == Some(false) {
            return false;
        }
        self.enabled
            .or(cfg)
            .unwrap_or(matches!(self.driver.as_str(), "codex" | "claudeAgent"))
    }
}

pub const DEFAULT_TEXT_GENERATION_MODEL: &str = "gpt-6-luna";

pub fn default_text_generation_model_selection() -> ModelSelection {
    ModelSelection {
        instance_id: "codex".into(),
        model: DEFAULT_TEXT_GENERATION_MODEL.into(),
        options: vec![OptionSelection {
            id: "reasoningEffort".into(),
            value: OptionValue::Str("low".into()),
        }],
    }
}

/// Server settings, `<userdata>/settings.json` (spec/06 §16.3).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub provider_instances: BTreeMap<String, ProviderInstanceConfig>,
    /// Null means "not migrated yet"; the client copy is in `ClientSettings`.
    pub favorites: Option<Vec<Value>>,
    pub provider_model_preferences: Option<Map<String, Value>>,
    /// Milliseconds.
    pub provider_health_refresh_interval: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_compaction: Option<String>,
    /// Legacy mirror of `idleCompaction`: false, true or "off".
    pub compact_idle_threads: Value,
    pub enable_provider_update_checks: bool,
    pub continue_threads_after_server_update: bool,
    pub keep_awake_while_agents_work: bool,
    pub background_activity: BackgroundActivity,
    pub background_activity_profile: BackgroundActivityProfile,
    /// Milliseconds.
    pub automatic_git_fetch_interval: u64,
    pub storage_cleanup: StorageCleanup,
    pub worktree_cleanup: Option<Value>,
    pub worktrees_directory: String,
    pub new_worktrees_start_from_origin: bool,
    pub worktree_submodules: Option<WorktreeSubmodules>,
    pub branch_naming_mode: BranchNamingMode,
    pub branch_name_prefix: String,
    pub branch_name_instructions: String,
    pub default_model_selection: Option<ModelSelection>,
    pub fallback_model_selection: Option<ModelSelection>,
    pub default_runtime_mode: RuntimeMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_thread_env_mode: Option<ThreadEnvMode>,
    pub default_auto_pull: bool,
    pub default_project_scripts: Vec<ProjectScript>,
    pub remove_agent_credits_on_merge: bool,
    pub pull_request_merge_method: Option<PullRequestMergeMethod>,
    pub project_settings_overrides: BTreeMap<String, Map<String, Value>>,
    pub add_project_base_directory: String,
    pub ghq_clone_layout: bool,
    pub text_generation_model_selection: ModelSelection,
    pub thread_title_instructions: String,
    pub response_streaming_mode: ResponseStreamingMode,
    pub snooze_presets: Vec<SnoozePreset>,
    pub snooze_limited_threads: bool,
    pub auto_resume_limited_threads: bool,
    pub sidebar_auto_settle_after_days: Option<u32>,
    pub sidebar_auto_settle_on_merge: bool,
    pub sidebar_auto_settle_keep_pull_requests: bool,
    pub source_control_writing_style: SourceControlWritingStyle,
    pub source_control_writer_model_selection: Option<ModelSelection>,
    pub review_guides_enabled: bool,
    pub review_guide_model_selection: Option<ModelSelection>,
    pub review_guide_instructions: String,
    pub source_control_hosts: Map<String, Value>,
    pub offered_source_control_providers: BTreeMap<String, bool>,
    /// Keys this build does not know, written back unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider_instances: BTreeMap::new(),
            favorites: None,
            provider_model_preferences: None,
            provider_health_refresh_interval: 300_000,
            idle_compaction: None,
            compact_idle_threads: Value::Bool(false),
            enable_provider_update_checks: true,
            continue_threads_after_server_update: true,
            keep_awake_while_agents_work: true,
            background_activity: BackgroundActivity::default(),
            background_activity_profile: BackgroundActivityProfile::Balanced,
            automatic_git_fetch_interval: 30_000,
            storage_cleanup: StorageCleanup::default(),
            worktree_cleanup: None,
            worktrees_directory: String::new(),
            new_worktrees_start_from_origin: true,
            worktree_submodules: None,
            branch_naming_mode: BranchNamingMode::Static,
            branch_name_prefix: "t3".into(),
            branch_name_instructions: String::new(),
            default_model_selection: None,
            fallback_model_selection: None,
            default_runtime_mode: RuntimeMode::FullAccess,
            default_thread_env_mode: None,
            default_auto_pull: false,
            default_project_scripts: Vec::new(),
            remove_agent_credits_on_merge: false,
            pull_request_merge_method: None,
            project_settings_overrides: BTreeMap::new(),
            add_project_base_directory: String::new(),
            ghq_clone_layout: false,
            text_generation_model_selection: default_text_generation_model_selection(),
            thread_title_instructions: String::new(),
            response_streaming_mode: ResponseStreamingMode::Paragraph,
            snooze_presets: default_snooze_presets(),
            snooze_limited_threads: false,
            auto_resume_limited_threads: false,
            sidebar_auto_settle_after_days: Some(7),
            sidebar_auto_settle_on_merge: true,
            sidebar_auto_settle_keep_pull_requests: false,
            source_control_writing_style: SourceControlWritingStyle::default(),
            source_control_writer_model_selection: None,
            review_guides_enabled: false,
            review_guide_model_selection: None,
            review_guide_instructions: String::new(),
            source_control_hosts: Map::new(),
            offered_source_control_providers: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

/// Keys compared whole when writing sparse settings (spec/06 §16.2). Every other object is
/// compared key by key.
const WHOLE_KEYS: &[&str] = &[
    "backgroundActivity",
    "automaticGitFetchInterval",
    "providerHealthRefreshInterval",
    "defaultModelSelection",
    "fallbackModelSelection",
    "textGenerationModelSelection",
    "sourceControlWriterModelSelection",
    "reviewGuideModelSelection",
    "worktreeCleanup",
];

impl Settings {
    /// Instance config for `instance_id`, falling back to an empty default-slot config.
    pub fn provider_instance(&self, instance_id: &str) -> ProviderInstanceConfig {
        self.provider_instances
            .get(instance_id)
            .cloned()
            .unwrap_or_else(|| ProviderInstanceConfig {
                driver: instance_id.to_string(),
                ..Default::default()
            })
    }

    /// The worktree root: `worktreesDirectory` with `~` expanded, else `default_root`.
    pub fn worktrees_root(&self, default_root: &Path, home: Option<&Path>) -> PathBuf {
        let dir = self.worktrees_directory.trim();
        if dir.is_empty() {
            return default_root.to_path_buf();
        }
        expand_home(dir, home)
    }

    /// Mirror `compactIdleThreads` from `idleCompaction` (T3 `settings.ts:1728-1745`).
    fn sync_idle_compaction(&mut self) {
        match self.idle_compaction.as_deref() {
            Some("onReturn") => self.compact_idle_threads = Value::Bool(false),
            Some("beforeCacheExpires") => self.compact_idle_threads = Value::Bool(true),
            Some("off") => self.compact_idle_threads = Value::String("off".into()),
            _ => {
                self.idle_compaction = match &self.compact_idle_threads {
                    Value::Bool(true) => Some("beforeCacheExpires".into()),
                    Value::String(s) if s == "off" => Some("off".into()),
                    _ => None,
                }
            }
        }
    }

    /// Only the values that differ from the defaults, plus unknown keys.
    pub fn to_sparse_json(&self) -> Value {
        let full = serde_json::to_value(self).unwrap_or(Value::Null);
        let defaults = serde_json::to_value(Self::default()).unwrap_or(Value::Null);
        match (full, defaults) {
            (Value::Object(full), Value::Object(defaults)) => {
                Value::Object(sparse(full, &defaults, WHOLE_KEYS))
            }
            (full, _) => full,
        }
    }

    pub fn load(userdata: &Path) -> Loaded<Self> {
        let mut loaded = load_lenient::<Self>(&userdata.join(SETTINGS_FILE), None);
        loaded.value.sync_idle_compaction();
        loaded
    }

    /// Writes `settings.json` sparsely: 2-space indent, trailing newline, atomic.
    pub fn save(&self, userdata: &Path) -> std::io::Result<()> {
        let mut text = serde_json::to_string_pretty(&self.to_sparse_json())?;
        text.push('\n');
        write_atomic(&userdata.join(SETTINGS_FILE), text.as_bytes())
    }
}

fn sparse(
    full: Map<String, Value>,
    defaults: &Map<String, Value>,
    whole: &[&str],
) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in full {
        match defaults.get(&key) {
            Some(d) if *d == value => {}
            Some(Value::Object(d)) if !whole.contains(&key.as_str()) => {
                if let Value::Object(v) = value {
                    let inner = sparse(v, d, &[]);
                    if !inner.is_empty() {
                        out.insert(key, Value::Object(inner));
                    }
                } else {
                    out.insert(key, value);
                }
            }
            _ => {
                out.insert(key, value);
            }
        }
    }
    out
}

pub fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(path),
    }
}

// ---------------------------------------------------------------------------------------------
// client settings (client-settings.json)

/// Device-local settings, `<userdata>/client-settings.json` (spec/06 §2-4, §9, §16.3).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientSettings {
    // General
    pub notification_mode: NotificationMode,
    pub in_app_notifications_enabled: bool,
    pub sidebar_project_grouping_mode: SidebarProjectGroupingMode,
    pub sidebar_project_grouping_overrides: BTreeMap<String, SidebarProjectGroupingMode>,
    pub sidebar_project_sort_order: SortOrder,
    pub sidebar_thread_sort_order: SortOrder,
    pub sidebar_working_shelf_enabled: bool,
    pub confirm_thread_unpin: bool,
    pub confirm_thread_archive: bool,
    pub confirm_thread_delete: bool,
    pub confirm_terminal_close: bool,
    pub confirm_quit: QuitConfirmation,
    pub language: String,
    pub timestamp_format: TimestampFormat,
    // Chat
    pub composer_rich_text_enabled: bool,
    pub send_shortcut: SendShortcut,
    pub show_skills_in_slash_menu: bool,
    pub follow_up_behavior: FollowUpBehavior,
    pub side_thread_open_target: SideThreadOpenTarget,
    pub tool_call_visibility: ToolCallVisibility,
    pub screen_reader_announcements: ScreenReaderAnnouncements,
    pub screen_reader_window_title: bool,
    // Diffs & pull requests
    pub diff_layout: DiffLayout,
    pub diff_ignore_whitespace: bool,
    pub diff_files_collapsed: bool,
    pub diff_color_scheme: DiffColorScheme,
    pub proactive_panels_enabled: bool,
    pub pull_request_links_open_in_app: bool,
    pub pull_request_merge_method_overrides: BTreeMap<String, PullRequestMergeMethod>,
    // Appearance
    pub appearance_contrast: u32,
    pub glass_opacity: u32,
    pub chat_width: ChatWidth,
    pub html_preview_width: HtmlPreviewWidth,
    pub thread_details_items: BTreeMap<String, bool>,
    pub effort_display: EffortDisplay,
    pub math_rendering_enabled: bool,
    pub panel_animation_duration_ms: u32,
    pub environment_identification_mode: EnvironmentIdentificationMode,
    pub persist_composer_context_strip: bool,
    // Typography
    pub font_family_sans: String,
    pub font_family_composer: String,
    pub font_family_code: String,
    pub font_family_terminal: String,
    pub font_size_interface: u32,
    pub font_size_prompt: u32,
    pub font_size_code: u32,
    pub font_size_terminal: u32,
    pub word_wrap: bool,
    pub font_smoothing: bool,
    // Models
    pub favorites: Vec<Value>,
    pub provider_model_preferences: Map<String, Value>,
    pub onboarding_completed_at: Option<String>,
    /// Voice, snap-shot, browser and load balancing keys land here and are preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            notification_mode: NotificationMode::Off,
            in_app_notifications_enabled: false,
            sidebar_project_grouping_mode: SidebarProjectGroupingMode::Repository,
            sidebar_project_grouping_overrides: BTreeMap::new(),
            sidebar_project_sort_order: SortOrder::UpdatedAt,
            sidebar_thread_sort_order: SortOrder::UpdatedAt,
            sidebar_working_shelf_enabled: true,
            confirm_thread_unpin: false,
            confirm_thread_archive: false,
            confirm_thread_delete: true,
            confirm_terminal_close: true,
            confirm_quit: QuitConfirmation::Hold,
            language: "en".into(),
            timestamp_format: TimestampFormat::Locale,
            composer_rich_text_enabled: true,
            send_shortcut: SendShortcut::Enter,
            show_skills_in_slash_menu: true,
            follow_up_behavior: FollowUpBehavior::Steer,
            side_thread_open_target: SideThreadOpenTarget::Panel,
            tool_call_visibility: ToolCallVisibility::ShowAll,
            screen_reader_announcements: ScreenReaderAnnouncements::Status,
            screen_reader_window_title: false,
            diff_layout: DiffLayout::Stacked,
            diff_ignore_whitespace: true,
            diff_files_collapsed: true,
            diff_color_scheme: DiffColorScheme::RedGreen,
            proactive_panels_enabled: false,
            pull_request_links_open_in_app: true,
            pull_request_merge_method_overrides: BTreeMap::new(),
            appearance_contrast: 100,
            glass_opacity: 80,
            chat_width: ChatWidth::Comfortable,
            html_preview_width: HtmlPreviewWidth::Chat,
            thread_details_items: BTreeMap::new(),
            effort_display: EffortDisplay::Label,
            math_rendering_enabled: false,
            panel_animation_duration_ms: 0,
            environment_identification_mode: EnvironmentIdentificationMode::Artwork,
            persist_composer_context_strip: false,
            font_family_sans: String::new(),
            font_family_composer: String::new(),
            font_family_code: String::new(),
            font_family_terminal: String::new(),
            font_size_interface: 16,
            font_size_prompt: 14,
            font_size_code: 13,
            font_size_terminal: 12,
            word_wrap: true,
            font_smoothing: true,
            favorites: Vec::new(),
            provider_model_preferences: Map::new(),
            onboarding_completed_at: None,
            extra: Map::new(),
        }
    }
}

impl ClientSettings {
    /// Reads `client-settings.json`; also accepts the `{"settings": {...}}` wrapper.
    pub fn load(userdata: &Path) -> Loaded<Self> {
        load_lenient::<Self>(&userdata.join(CLIENT_SETTINGS_FILE), Some("settings"))
    }

    /// Writes the full object as compact JSON plus a newline.
    pub fn save(&self, userdata: &Path) -> std::io::Result<()> {
        let mut text = serde_json::to_string(self)?;
        text.push('\n');
        write_atomic(&userdata.join(CLIENT_SETTINGS_FILE), text.as_bytes())
    }
}

// ---------------------------------------------------------------------------------------------
// loading helpers

/// A decoded settings file plus what went wrong while reading it.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded<T> {
    pub value: T,
    /// True when the file existed.
    pub found: bool,
    /// The file could not be parsed at all: callers run on defaults and must not write it
    /// back (spec/06 §16.2).
    pub unreadable: Option<String>,
    /// Keys whose values did not decode and fell back to defaults.
    pub dropped_keys: Vec<String>,
}

fn load_lenient<T: DeserializeOwned + Default>(path: &Path, wrapper: Option<&str>) -> Loaded<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => {
            return Loaded {
                value: T::default(),
                found: false,
                unreadable: None,
                dropped_keys: vec![],
            };
        }
    };
    match decode_lenient::<T>(&text, wrapper) {
        Ok((value, dropped_keys)) => Loaded {
            value,
            found: true,
            unreadable: None,
            dropped_keys,
        },
        Err(e) => Loaded {
            value: T::default(),
            found: true,
            unreadable: Some(format!("{}: {e}", path.display())),
            dropped_keys: vec![],
        },
    }
}

/// Decode a JSON-with-comments object key by key: a key that does not decode falls back to
/// its default instead of failing the whole file.
pub fn decode_lenient<T: DeserializeOwned + Default>(
    text: &str,
    wrapper: Option<&str>,
) -> Result<(T, Vec<String>), String> {
    let cleaned = strip_jsonc(text);
    if cleaned.trim().is_empty() {
        return Ok((T::default(), vec![]));
    }
    let mut value: Value = serde_json::from_str(&cleaned).map_err(|e| e.to_string())?;
    if let Some(w) = wrapper
        && let Some(inner) = value.get(w).filter(|v| v.is_object()).cloned()
    {
        value = inner;
    }
    let Value::Object(map) = value else {
        return Err("expected a JSON object".into());
    };
    if let Ok(v) = serde_json::from_value::<T>(Value::Object(map.clone())) {
        return Ok((v, vec![]));
    }
    let mut good = Map::new();
    let mut dropped = Vec::new();
    for (k, v) in map {
        let mut trial = good.clone();
        trial.insert(k.clone(), v.clone());
        if serde_json::from_value::<T>(Value::Object(trial)).is_ok() {
            good.insert(k, v);
        } else {
            dropped.push(k);
        }
    }
    let v = serde_json::from_value::<T>(Value::Object(good)).map_err(|e| e.to_string())?;
    Ok((v, dropped))
}

/// Remove `//` and `/* */` comments and trailing commas, leaving strings alone.
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_str = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(chars.get(j), Some(']') | Some('}')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Temp file in the same directory, fsync, rename. New files get mode 0600, the directory 0700.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map(|m| m.permissions().mode())
            .unwrap_or(0o600);
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    }
    std::fs::rename(&tmp, path)
}

/// What `load_userdata` found.
#[derive(Clone, Debug, PartialEq)]
pub struct UserData {
    pub settings: Loaded<Settings>,
    pub client: Loaded<ClientSettings>,
    pub keybindings: Loaded<Vec<KeybindingRule>>,
    /// Files copied from T3 Code's userdata on this load.
    pub imported: Vec<String>,
}

/// Load settings from `userdata` (normally `~/.tern/userdata`). When Tern has no
/// `settings.json` yet and `t3_userdata/settings.json` exists, the three files are imported
/// once: decoded, then written into `userdata`. Missing `keybindings.json` is created with the
/// defaults (spec/06 §11.3).
pub fn load_userdata(userdata: &Path, t3_userdata: Option<&Path>) -> UserData {
    let mut imported = Vec::new();
    if !userdata.join(SETTINGS_FILE).exists()
        && let Some(t3) = t3_userdata.filter(|d| d.join(SETTINGS_FILE).exists())
    {
        let s = Settings::load(t3);
        if s.unreadable.is_none() && s.value.save(userdata).is_ok() {
            imported.push(SETTINGS_FILE.to_string());
        }
        let c = ClientSettings::load(t3);
        if c.found
            && c.unreadable.is_none()
            && !userdata.join(CLIENT_SETTINGS_FILE).exists()
            && c.value.save(userdata).is_ok()
        {
            imported.push(CLIENT_SETTINGS_FILE.to_string());
        }
        let k = load_keybinding_file(&t3.join(KEYBINDINGS_FILE));
        if k.found
            && k.unreadable.is_none()
            && !userdata.join(KEYBINDINGS_FILE).exists()
            && save_keybindings(userdata, &k.value).is_ok()
        {
            imported.push(KEYBINDINGS_FILE.to_string());
        }
    }
    let keybindings_path = userdata.join(KEYBINDINGS_FILE);
    if !keybindings_path.exists() {
        let _ = save_keybindings(userdata, &default_keybindings());
    }
    UserData {
        settings: Settings::load(userdata),
        client: ClientSettings::load(userdata),
        keybindings: load_keybindings(userdata),
        imported,
    }
}

// ---------------------------------------------------------------------------------------------
// keybindings (keybindings.json)

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeybindingRule {
    pub key: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
}

impl KeybindingRule {
    pub fn new(key: &str, command: &str, when: Option<&str>) -> Self {
        Self {
            key: key.into(),
            command: command.into(),
            when: when.map(Into::into),
        }
    }
}

pub const MAX_KEYBINDING_RULES: usize = 256;

/// T3's default table (`packages/shared/src/keybindings.ts:58-156`) minus `composer.dictation`
/// (voice is out, spec/02 §5.2). (command, key, when)
pub const DEFAULT_KEYBINDINGS: &[(&str, &str, Option<&str>)] = &[
    ("sidebar.toggle", "mod+b", None),
    ("workspace.focusNext", "f6", None),
    ("workspace.focusPrevious", "shift+f6", None),
    ("workspace.focusNext", "mod+f6", None),
    ("workspace.focusPrevious", "mod+shift+f6", None),
    ("navigation.back", "mod+[", Some("!terminalFocus")),
    ("navigation.forward", "mod+]", Some("!terminalFocus")),
    ("terminal.toggle", "mod+j", None),
    ("rightPanel.toggle", "mod+alt+b", None),
    ("threadPanel.toggle", "mod+shift+b", Some("!terminalFocus")),
    (
        "rightPanel.new",
        "mod+t",
        Some("rightPanelOpen && !terminalFocus"),
    ),
    ("terminal.split", "mod+d", Some("terminalFocus")),
    (
        "terminal.splitVertical",
        "mod+shift+d",
        Some("terminalFocus"),
    ),
    ("terminal.new", "mod+n", Some("terminalFocus")),
    ("terminal.close", "mod+w", Some("terminalFocus")),
    (
        "terminal.previous",
        "ctrl+alt+[",
        Some("terminalFocus && !isMac"),
    ),
    (
        "terminal.next",
        "ctrl+alt+]",
        Some("terminalFocus && !isMac"),
    ),
    ("rightPanel.close", "mod+w", Some("!terminalFocus")),
    ("view.reopenClosed", "mod+shift+t", None),
    ("diff.toggle", "mod+d", Some("!terminalFocus")),
    ("preview.toggle", "mod+shift+j", None),
    ("preview.refresh", "mod+r", Some("previewFocus")),
    ("preview.focusUrl", "mod+l", Some("previewFocus")),
    ("preview.zoomIn", "mod+=", Some("previewFocus")),
    ("preview.zoomIn", "mod++", Some("previewFocus")),
    ("preview.zoomOut", "mod+-", Some("previewFocus")),
    ("preview.resetZoom", "mod+0", Some("previewFocus")),
    ("preview.find", "mod+f", Some("previewFocus")),
    ("commandPalette.toggle", "mod+k", Some("!terminalFocus")),
    ("filePicker.toggle", "mod+p", Some("!terminalFocus")),
    (
        "projectSearch.toggle",
        "mod+shift+f",
        Some("!terminalFocus"),
    ),
    ("usage.open", "mod+u", Some("!terminalFocus")),
    ("theme.select", "mod+alt+a", Some("!terminalFocus")),
    (
        "appearance.cycle",
        "mod+alt+shift+a",
        Some("!terminalFocus"),
    ),
    ("themeEditor.toggle", "mod+alt+shift+t", None),
    ("composer.stash", "mod+s", Some("!terminalFocus")),
    (
        "thread.steerQueuedMessage",
        "mod+shift+enter",
        Some("!terminalFocus"),
    ),
    (
        "thread.editQueuedMessage",
        "alt+arrowup",
        Some("composerFocus"),
    ),
    (
        "composer.sendAlternate",
        "mod+enter",
        Some("composerFocus && turnRunning"),
    ),
    (
        "composer.sendBackground",
        "mod+enter",
        Some("composerFocus && draftThreadRoute"),
    ),
    (
        "composer.sendBackground",
        "mod+alt+enter",
        Some("composerFocus && draftThreadRoute"),
    ),
    (
        "composer.sendAndNewThread",
        "mod+alt+enter",
        Some("composerFocus && !draftThreadRoute"),
    ),
    ("chat.new", "mod+n", Some("!terminalFocus")),
    ("chat.new", "mod+shift+o", Some("!terminalFocus")),
    ("chat.newLocal", "mod+shift+n", Some("!terminalFocus")),
    (
        "chat.newWithoutProject",
        "mod+alt+n",
        Some("!terminalFocus"),
    ),
    (
        "chat.find",
        "mod+f",
        Some("!terminalFocus && !previewFocus"),
    ),
    ("modelPicker.toggle", "mod+shift+m", Some("!terminalFocus")),
    ("composer.host", "mod+shift+h", Some("!terminalFocus")),
    ("composer.effort", "mod+shift+e", Some("!terminalFocus")),
    ("composer.mode", "mod+shift+a", Some("!terminalFocus")),
    ("composer.workspace", "mod+shift+x", Some("!terminalFocus")),
    ("composer.branch", "mod+shift+g", Some("!terminalFocus")),
    (
        "composer.previousWorktree",
        "mod+shift+l",
        Some("!terminalFocus"),
    ),
    (
        "pullRequest.copyNumber",
        "mod+shift+k",
        Some("!terminalFocus"),
    ),
    (
        "modelPicker.previousProvider",
        "mod+shift+arrowup",
        Some("modelPickerOpen"),
    ),
    (
        "modelPicker.nextProvider",
        "mod+shift+arrowdown",
        Some("modelPickerOpen"),
    ),
    ("editor.openFavorite", "mod+o", None),
    ("thread.previous", "mod+shift+[", None),
    ("thread.next", "mod+shift+]", None),
    (
        "terminal.previous",
        "cmd+shift+[",
        Some("terminalFocus && isMac"),
    ),
    (
        "terminal.next",
        "cmd+shift+]",
        Some("terminalFocus && isMac"),
    ),
    (
        "thread.copyReference",
        "mod+shift+c",
        Some("!terminalFocus"),
    ),
    ("thread.settle", "mod+shift+s", Some("!terminalFocus")),
    ("thread.pin", "mod+shift+p", Some("!terminalFocus")),
    (
        "thread.undo",
        "mod+z",
        Some("!terminalFocus && !editableFocus"),
    ),
    (
        "thread.jump.1",
        "mod+1",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.2",
        "mod+2",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.3",
        "mod+3",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.4",
        "mod+4",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.5",
        "mod+5",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.6",
        "mod+6",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.7",
        "mod+7",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.8",
        "mod+8",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "thread.jump.9",
        "mod+9",
        Some("!modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.1",
        "mod+1",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.2",
        "mod+2",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.3",
        "mod+3",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.4",
        "mod+4",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.5",
        "mod+5",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.6",
        "mod+6",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.7",
        "mod+7",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.8",
        "mod+8",
        Some("modelPickerOpen && isDesktop"),
    ),
    (
        "modelPicker.jump.9",
        "mod+9",
        Some("modelPickerOpen && isDesktop"),
    ),
    ("usage.cost", "c", Some("usagePageOpen")),
    ("usage.tokens", "t", Some("usagePageOpen")),
    ("usage.limits", "l", Some("usagePageOpen")),
    ("usage.period.day", "mod+shift+1", Some("usagePageOpen")),
    ("usage.period.week", "mod+shift+2", Some("usagePageOpen")),
    ("usage.period.month", "mod+shift+3", Some("usagePageOpen")),
    ("usage.period.quarter", "mod+shift+4", Some("usagePageOpen")),
];

/// Valid commands with no default key (picker only).
pub const UNBOUND_COMMANDS: &[&str] = &[
    "rightPanel.toggleMaximized",
    "composer.cycleHost",
    "composer.increaseEffort",
    "composer.decreaseEffort",
    "thread.stop",
    "thread.regenerateTitle",
];

pub fn default_keybindings() -> Vec<KeybindingRule> {
    DEFAULT_KEYBINDINGS
        .iter()
        .map(|(command, key, when)| KeybindingRule::new(key, command, *when))
        .collect()
}

/// Settings label for a command (`KeybindingsSettings.logic.ts:453-559`).
pub fn command_label(command: &str) -> String {
    let fixed = match command {
        "sidebar.toggle" => "Sidebar: Toggle",
        "workspace.focusNext" => "Focus Next Workspace Area",
        "workspace.focusPrevious" => "Focus Previous Workspace Area",
        "navigation.back" => "Navigation: Back",
        "navigation.forward" => "Navigation: Forward",
        "terminal.toggle" => "Terminal: Toggle",
        "rightPanel.toggle" => "Right Panel: Toggle",
        "rightPanel.toggleMaximized" => "Right Panel: Toggle Maximized",
        "threadPanel.toggle" => "Thread Panel: Toggle",
        "rightPanel.new" => "Right Panel: New",
        "terminal.split" => "Terminal: Split",
        "terminal.splitVertical" => "Terminal: Split Vertical",
        "terminal.new" => "Terminal: New",
        "terminal.close" => "Terminal: Close",
        "terminal.previous" => "Terminal: Previous",
        "terminal.next" => "Terminal: Next",
        "rightPanel.close" => "Right Panel: Close",
        "view.reopenClosed" => "Reopen Closed Tab",
        "diff.toggle" => "Diff: Toggle",
        "preview.toggle" => "Preview: Toggle",
        "preview.refresh" => "Preview: Refresh",
        "preview.focusUrl" => "Preview: Focus URL",
        "preview.zoomIn" => "Preview: Zoom In",
        "preview.zoomOut" => "Preview: Zoom Out",
        "preview.resetZoom" => "Preview: Reset Zoom",
        "preview.find" => "Preview: Find",
        "commandPalette.toggle" => "Command Palette: Toggle",
        "filePicker.toggle" => "File Picker: Toggle",
        "projectSearch.toggle" => "Project Search: Toggle",
        "usage.open" => "Usage: Open",
        "usage.cost" => "Usage: Cost",
        "usage.tokens" => "Usage: Tokens",
        "usage.limits" => "Usage: Limits",
        "usage.period.day" => "Usage: Period: Day",
        "usage.period.week" => "Usage: Period: Week",
        "usage.period.month" => "Usage: Period: Month",
        "usage.period.quarter" => "Usage: Period: Quarter",
        "theme.select" => "Theme: Select",
        "appearance.cycle" => "Appearance: Cycle",
        "themeEditor.toggle" => "Theme Editor: Toggle",
        "composer.stash" => "Composer: Stash",
        "thread.steerQueuedMessage" => "Queue: Send First Queued Message",
        "thread.editQueuedMessage" => "Queue: Edit Last Queued Message",
        "composer.sendAlternate" => "Composer: Opposite Queue or Steer Action",
        "composer.sendBackground" => "Composer: Start in Background",
        "composer.sendAndNewThread" => "Composer: Send and Start New Thread",
        "composer.cycleHost" => "Composer: Cycle Host",
        "composer.increaseEffort" => "Composer: Increase Effort",
        "composer.decreaseEffort" => "Composer: Decrease Effort",
        "chat.new" => "Chat: New",
        "chat.newLocal" => "Chat: New Local",
        "chat.newWithoutProject" => "Chat: New Without Project",
        "chat.find" => "Chat: Find",
        "modelPicker.toggle" => "Model Picker: Toggle",
        "composer.host" => "Composer: Host",
        "composer.effort" => "Composer: Effort",
        "composer.mode" => "Composer: Mode",
        "composer.workspace" => "Composer: Workspace",
        "composer.branch" => "Composer: Branch",
        "composer.previousWorktree" => "Composer: Previous Worktree",
        "pullRequest.copyNumber" => "Pull Request: Copy Number",
        "modelPicker.previousProvider" => "Model Picker: Previous Provider",
        "modelPicker.nextProvider" => "Model Picker: Next Provider",
        "editor.openFavorite" => "Editor: Open Favorite",
        "thread.previous" => "Thread: Previous",
        "thread.next" => "Thread: Next",
        "thread.copyReference" => "Pull Request: Copy Link or Thread",
        "thread.settle" => "Thread: Settle",
        "thread.pin" => "Thread: Pin",
        "thread.undo" => "Thread: Undo",
        "thread.stop" => "Thread: Stop",
        "thread.regenerateTitle" => "Thread: Regenerate Title",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    if let Some(n) = command.strip_prefix("thread.jump.") {
        return format!("Thread: Jump: {n}");
    }
    if let Some(n) = command.strip_prefix("modelPicker.jump.") {
        return format!("Model Picker: Jump: {n}");
    }
    if let Some(id) = command
        .strip_prefix("script.")
        .and_then(|s| s.strip_suffix(".run"))
    {
        return format!("Run Script: {}", title_case_id(id));
    }
    command.to_string()
}

fn title_case_id(id: &str) -> String {
    id.split('-')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn is_known_command(command: &str) -> bool {
    if DEFAULT_KEYBINDINGS.iter().any(|(c, _, _)| *c == command)
        || UNBOUND_COMMANDS.contains(&command)
    {
        return true;
    }
    command
        .strip_prefix("script.")
        .and_then(|s| s.strip_suffix(".run"))
        .is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 24
                && id
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// A parsed key string (spec/06 §11.3). `mod` resolves per platform when matching.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct KeyChord {
    pub mod_key: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// Lowercase key token: a single character, `space`, `escape`, `enter`, `arrowup`, `f6`, ...
    pub key: String,
}

impl KeyChord {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_lowercase();
        if text.is_empty() || text.len() > 64 {
            return None;
        }
        let mut parts: Vec<String> = text.split('+').map(str::to_string).collect();
        // "mod++" splits into [.., "", ""]: the key is "+"
        if text.ends_with("++") {
            parts.truncate(parts.len() - 2);
            parts.push("+".into());
        }
        let mut chord = KeyChord::default();
        let (key, mods) = parts.split_last()?;
        for m in mods {
            match m.as_str() {
                "mod" => chord.mod_key = true,
                "ctrl" | "control" => chord.ctrl = true,
                "alt" | "option" => chord.alt = true,
                "shift" => chord.shift = true,
                "cmd" | "meta" => chord.meta = true,
                _ => return None,
            }
        }
        let key = match key.as_str() {
            "" => return None,
            "esc" => "escape".to_string(),
            "space" => "space".to_string(),
            "up" => "arrowup".into(),
            "down" => "arrowdown".into(),
            "left" => "arrowleft".into(),
            "right" => "arrowright".into(),
            k => k.to_string(),
        };
        if matches!(
            key.as_str(),
            "mod" | "ctrl" | "control" | "alt" | "option" | "shift" | "cmd" | "meta"
        ) {
            return None;
        }
        chord.key = key;
        Some(chord)
    }

    /// Display label: `⇧⌘K` on macOS, `Ctrl+Shift+K` elsewhere (spec/06 §11.3).
    pub fn label(&self, mac: bool) -> String {
        let key = match self.key.as_str() {
            "space" => "Space".to_string(),
            "escape" => "Esc".to_string(),
            "arrowup" => "Up".to_string(),
            "arrowdown" => "Down".to_string(),
            "arrowleft" => "Left".to_string(),
            "arrowright" => "Right".to_string(),
            "pageup" => "PageUp".to_string(),
            "pagedown" => "PageDown".to_string(),
            k if k.chars().count() == 1 => k.to_uppercase(),
            k => {
                let mut c = k.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
        };
        let ctrl = self.ctrl || (self.mod_key && !mac);
        let meta = self.meta || (self.mod_key && mac);
        if mac {
            let mut s = String::new();
            if ctrl {
                s.push('⌃');
            }
            if self.alt {
                s.push('⌥');
            }
            if self.shift {
                s.push('⇧');
            }
            if meta {
                s.push('⌘');
            }
            s + &key
        } else {
            let mut parts = Vec::new();
            if ctrl {
                parts.push("Ctrl".to_string());
            }
            if self.alt {
                parts.push("Alt".to_string());
            }
            if self.shift {
                parts.push("Shift".to_string());
            }
            if meta {
                parts.push("Meta".to_string());
            }
            parts.push(key);
            parts.join("+")
        }
    }
}

/// A `when` expression: identifiers, `!`, `&&`, `||`, parentheses. Unknown identifiers are false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum When {
    Ident(String),
    Not(Box<When>),
    And(Box<When>, Box<When>),
    Or(Box<When>, Box<When>),
}

impl When {
    pub fn parse(text: &str) -> Option<Self> {
        let tokens = when_tokens(text)?;
        let mut pos = 0;
        let expr = parse_or(&tokens, &mut pos, 0)?;
        (pos == tokens.len()).then_some(expr)
    }

    pub fn eval(&self, ctx: &dyn Fn(&str) -> bool) -> bool {
        match self {
            When::Ident(s) if s == "true" => true,
            When::Ident(s) if s == "false" => false,
            When::Ident(s) => ctx(s),
            When::Not(e) => !e.eval(ctx),
            When::And(a, b) => a.eval(ctx) && b.eval(ctx),
            When::Or(a, b) => a.eval(ctx) || b.eval(ctx),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum WhenTok {
    Ident(String),
    Not,
    And,
    Or,
    LParen,
    RParen,
}

fn when_tokens(text: &str) -> Option<Vec<WhenTok>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '!' => {
                out.push(WhenTok::Not);
                i += 1;
            }
            '(' => {
                out.push(WhenTok::LParen);
                i += 1;
            }
            ')' => {
                out.push(WhenTok::RParen);
                i += 1;
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                out.push(WhenTok::And);
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                out.push(WhenTok::Or);
                i += 2;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.' | '-'))
                {
                    i += 1;
                }
                out.push(WhenTok::Ident(chars[start..i].iter().collect()));
            }
            _ => return None,
        }
    }
    Some(out)
}

const MAX_WHEN_DEPTH: usize = 64;

fn parse_or(t: &[WhenTok], pos: &mut usize, depth: usize) -> Option<When> {
    let mut left = parse_and(t, pos, depth)?;
    while t.get(*pos) == Some(&WhenTok::Or) {
        *pos += 1;
        let right = parse_and(t, pos, depth)?;
        left = When::Or(Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_and(t: &[WhenTok], pos: &mut usize, depth: usize) -> Option<When> {
    let mut left = parse_not(t, pos, depth)?;
    while t.get(*pos) == Some(&WhenTok::And) {
        *pos += 1;
        let right = parse_not(t, pos, depth)?;
        left = When::And(Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_not(t: &[WhenTok], pos: &mut usize, depth: usize) -> Option<When> {
    if depth > MAX_WHEN_DEPTH {
        return None;
    }
    match t.get(*pos)? {
        WhenTok::Not => {
            *pos += 1;
            Some(When::Not(Box::new(parse_not(t, pos, depth + 1)?)))
        }
        WhenTok::LParen => {
            *pos += 1;
            let e = parse_or(t, pos, depth + 1)?;
            if t.get(*pos) != Some(&WhenTok::RParen) {
                return None;
            }
            *pos += 1;
            Some(e)
        }
        WhenTok::Ident(s) => {
            *pos += 1;
            Some(When::Ident(s.clone()))
        }
        _ => None,
    }
}

/// Is this rule well formed? (key parses, command known, when parses, lengths in range)
pub fn validate_rule(rule: &KeybindingRule) -> Result<(), String> {
    if rule.key.is_empty() || rule.key.len() > 64 || KeyChord::parse(&rule.key).is_none() {
        return Err(format!("invalid key \"{}\"", rule.key));
    }
    if !is_known_command(&rule.command) {
        return Err(format!("unknown command \"{}\"", rule.command));
    }
    if let Some(w) = &rule.when
        && (w.is_empty() || w.len() > 256 || When::parse(w).is_none())
    {
        return Err(format!("invalid when \"{w}\""));
    }
    Ok(())
}

/// Defaults plus user rules (spec/06 §11.3): a command named in the file drops all of its
/// defaults; remaining defaults go first, file rules after. Later rules win when matching.
pub fn merge_keybindings(user: &[KeybindingRule]) -> Vec<KeybindingRule> {
    let defaults = default_keybindings();
    if user.is_empty() {
        return defaults;
    }
    let overridden: std::collections::HashSet<&str> =
        user.iter().map(|r| r.command.as_str()).collect();
    defaults
        .into_iter()
        .filter(|d| !overridden.contains(d.command.as_str()))
        .chain(user.iter().cloned())
        .collect()
}

fn load_keybinding_file(path: &Path) -> Loaded<Vec<KeybindingRule>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => {
            return Loaded {
                value: vec![],
                found: false,
                unreadable: None,
                dropped_keys: vec![],
            };
        }
    };
    let cleaned = strip_jsonc(&text);
    if cleaned.trim().is_empty() {
        return Loaded {
            value: vec![],
            found: true,
            unreadable: None,
            dropped_keys: vec![],
        };
    }
    match serde_json::from_str::<Vec<Value>>(&cleaned) {
        Ok(entries) => {
            let mut rules = Vec::new();
            let mut dropped = Vec::new();
            for (i, e) in entries.into_iter().enumerate() {
                match serde_json::from_value::<KeybindingRule>(e) {
                    Ok(r) => match validate_rule(&r) {
                        Ok(()) if rules.len() < MAX_KEYBINDING_RULES => rules.push(r),
                        Ok(()) => {
                            dropped.push(format!("[{i}]: more than {MAX_KEYBINDING_RULES} rules"))
                        }
                        Err(e) => dropped.push(format!("[{i}]: {e}")),
                    },
                    Err(e) => dropped.push(format!("[{i}]: {e}")),
                }
            }
            Loaded {
                value: rules,
                found: true,
                unreadable: None,
                dropped_keys: dropped,
            }
        }
        Err(e) => Loaded {
            value: vec![],
            found: true,
            unreadable: Some(format!(
                "Unable to parse keybindings config at {}: {e}",
                path.display()
            )),
            dropped_keys: vec![],
        },
    }
}

/// Effective bindings from `<userdata>/keybindings.json` merged over the defaults.
pub fn load_keybindings(userdata: &Path) -> Loaded<Vec<KeybindingRule>> {
    let mut loaded = load_keybinding_file(&userdata.join(KEYBINDINGS_FILE));
    loaded.value = merge_keybindings(&loaded.value);
    loaded
}

/// Pretty JSON array, 2 spaces, trailing newline, atomic.
pub fn save_keybindings(userdata: &Path, rules: &[KeybindingRule]) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(rules)?;
    text.push('\n');
    write_atomic(&userdata.join(KEYBINDINGS_FILE), text.as_bytes())
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("tern-settings-{}", crate::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_serialize_with_t3_keys() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(v["branchNamePrefix"], "t3");
        assert_eq!(v["defaultRuntimeMode"], "full-access");
        assert_eq!(v["providerHealthRefreshInterval"], 300_000);
        assert_eq!(v["textGenerationModelSelection"]["model"], "gpt-6-luna");
        assert_eq!(v["sidebarAutoSettleAfterDays"], 7);
        assert_eq!(
            v["storageCleanup"]["worktreeKeepWhen"],
            "uncommitted-changes"
        );
        assert!(v.get("defaultThreadEnvMode").is_none());

        let c = serde_json::to_value(ClientSettings::default()).unwrap();
        assert_eq!(c["sendShortcut"], "enter");
        assert_eq!(c["followUpBehavior"], "steer");
        assert_eq!(c["confirmQuit"], "hold");
        assert_eq!(c["toolCallVisibility"], "showAll");
        assert_eq!(c["diffFilesCollapsed"], true);
        assert_eq!(c["fontSizeCode"], 13);
    }

    #[test]
    fn default_settings_write_empty_object() {
        assert_eq!(Settings::default().to_sparse_json(), serde_json::json!({}));
    }

    #[test]
    fn sparse_write_keeps_only_changes_and_unknown_keys() {
        let mut s = Settings::default();
        s.branch_name_prefix = "tern".into();
        s.storage_cleanup.worktree_on_delete = true;
        s.sidebar_auto_settle_after_days = None;
        s.background_activity.profile = BackgroundActivityProfile::Performance;
        s.extra
            .insert("voiceThing".into(), serde_json::json!({"a": 1}));
        let v = s.to_sparse_json();
        assert_eq!(
            v,
            serde_json::json!({
                "branchNamePrefix": "tern",
                "storageCleanup": {"worktreeOnDelete": true},
                "sidebarAutoSettleAfterDays": null,
                "backgroundActivity": {"schemaVersion": 1, "profile": "performance", "overrides": {}},
                "voiceThing": {"a": 1}
            })
        );
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let dir = tempdir();
        let mut s = Settings::default();
        s.default_runtime_mode = RuntimeMode::ApprovalRequired;
        s.default_thread_env_mode = Some(ThreadEnvMode::Worktree);
        s.worktrees_directory = "~/wt".into();
        s.provider_instances.insert(
            "codex".into(),
            ProviderInstanceConfig {
                driver: "codex".into(),
                config: serde_json::from_value(serde_json::json!({"binaryPath": "/opt/codex"}))
                    .unwrap(),
                ..Default::default()
            },
        );
        s.extra
            .insert("futureKey".into(), serde_json::json!([1, 2]));
        s.save(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(text.ends_with("}\n"));
        assert!(text.contains("  \"defaultRuntimeMode\": \"approval-required\""));
        let loaded = Settings::load(&dir);
        assert!(loaded.found);
        assert_eq!(loaded.value, s);
        assert_eq!(
            loaded.value.provider_instance("codex").binary_path(),
            Some("/opt/codex")
        );
        assert_eq!(
            loaded.value.provider_instance("claudeAgent").binary_path(),
            None
        );
        assert_eq!(
            loaded
                .value
                .worktrees_root(Path::new("/d"), Some(Path::new("/home/u"))),
            PathBuf::from("/home/u/wt")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn client_settings_round_trip_and_wrapper() {
        let dir = tempdir();
        let mut c = ClientSettings::default();
        c.diff_layout = DiffLayout::Split;
        c.thread_details_items.insert("usage".into(), true);
        c.extra
            .insert("voiceMicrophones".into(), serde_json::json!([]));
        c.save(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join(CLIENT_SETTINGS_FILE)).unwrap();
        assert!(!text.contains("\n  "), "compact");
        assert_eq!(ClientSettings::load(&dir).value, c);

        std::fs::write(
            dir.join(CLIENT_SETTINGS_FILE),
            r#"{"settings": {"chatWidth": "wide", "confirmQuit": false}}"#,
        )
        .unwrap();
        let c = ClientSettings::load(&dir).value;
        assert_eq!(c.chat_width, ChatWidth::Wide);
        assert_eq!(c.confirm_quit, QuitConfirmation::Direct);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn lenient_read_drops_bad_keys_and_accepts_comments() {
        let text = r#"{
            // comment
            "branchNamePrefix": "x", /* block */
            "defaultRuntimeMode": "nonsense",
            "responseStreamingMode": "token",
            "sourceControlWritingStyle": "Use julius/ prefixes",
            "url": "http://a//b",
        }"#;
        let (s, dropped) = decode_lenient::<Settings>(text, None).unwrap();
        assert_eq!(s.branch_name_prefix, "x");
        assert_eq!(s.default_runtime_mode, RuntimeMode::FullAccess);
        assert_eq!(s.response_streaming_mode, ResponseStreamingMode::Paragraph);
        assert_eq!(
            s.source_control_writing_style.mode,
            SourceControlWritingMode::Custom
        );
        assert_eq!(s.extra["url"], "http://a//b");
        assert_eq!(dropped, vec!["defaultRuntimeMode".to_string()]);
    }

    #[test]
    fn unreadable_file_reports_and_uses_defaults() {
        let dir = tempdir();
        std::fs::write(dir.join(SETTINGS_FILE), "{ not json").unwrap();
        let l = Settings::load(&dir);
        assert!(l.unreadable.is_some());
        assert_eq!(l.value, Settings::default());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn idle_compaction_mirrors_legacy_key() {
        let (mut s, _) =
            decode_lenient::<Settings>(r#"{"compactIdleThreads": "off"}"#, None).unwrap();
        s.sync_idle_compaction();
        assert_eq!(s.idle_compaction.as_deref(), Some("off"));
        s.idle_compaction = Some("beforeCacheExpires".into());
        s.sync_idle_compaction();
        assert_eq!(s.compact_idle_threads, Value::Bool(true));
    }

    #[test]
    fn imports_t3_userdata_once() {
        let t3 = tempdir();
        let tern = tempdir().join("userdata");
        std::fs::write(
            t3.join(SETTINGS_FILE),
            r#"{"branchNamePrefix": "me", "workflowsEnabled": true}"#,
        )
        .unwrap();
        std::fs::write(t3.join(CLIENT_SETTINGS_FILE), r#"{"diffLayout": "split"}"#).unwrap();
        std::fs::write(
            t3.join(KEYBINDINGS_FILE),
            r#"[{"key": "mod+shift+y", "command": "terminal.toggle"}]"#,
        )
        .unwrap();

        let u = load_userdata(&tern, Some(&t3));
        assert_eq!(u.imported.len(), 3);
        assert_eq!(u.settings.value.branch_name_prefix, "me");
        assert_eq!(u.settings.value.extra["workflowsEnabled"], true);
        assert_eq!(u.client.value.diff_layout, DiffLayout::Split);
        let toggles: Vec<_> = u
            .keybindings
            .value
            .iter()
            .filter(|r| r.command == "terminal.toggle")
            .collect();
        assert_eq!(toggles.len(), 1);
        assert_eq!(toggles[0].key, "mod+shift+y");

        // second load does not import again, even if T3 changed
        std::fs::write(t3.join(SETTINGS_FILE), r#"{"branchNamePrefix": "other"}"#).unwrap();
        let u = load_userdata(&tern, Some(&t3));
        assert!(u.imported.is_empty());
        assert_eq!(u.settings.value.branch_name_prefix, "me");
        std::fs::remove_dir_all(t3).ok();
        std::fs::remove_dir_all(tern.parent().unwrap()).ok();
    }

    #[test]
    fn fresh_userdata_writes_default_keybindings() {
        let dir = tempdir();
        let u = load_userdata(&dir, None);
        assert!(u.imported.is_empty());
        assert!(dir.join(KEYBINDINGS_FILE).exists());
        assert!(!dir.join(SETTINGS_FILE).exists());
        assert_eq!(u.keybindings.value, default_keybindings());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn default_table_is_valid_and_has_no_dictation() {
        for r in default_keybindings() {
            validate_rule(&r).unwrap_or_else(|e| panic!("{r:?}: {e}"));
            assert!(!command_label(&r.command).is_empty());
        }
        assert!(
            DEFAULT_KEYBINDINGS
                .iter()
                .all(|(c, _, _)| *c != "composer.dictation")
        );
        let k = DEFAULT_KEYBINDINGS
            .iter()
            .find(|(c, _, _)| *c == "commandPalette.toggle")
            .unwrap();
        assert_eq!((k.1, k.2), ("mod+k", Some("!terminalFocus")));
    }

    #[test]
    fn merge_drops_defaults_of_overridden_commands() {
        let user = vec![KeybindingRule::new(
            "mod+shift+d",
            "diff.toggle",
            Some("false"),
        )];
        let merged = merge_keybindings(&user);
        let diffs: Vec<_> = merged
            .iter()
            .filter(|r| r.command == "diff.toggle")
            .collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(merged.last(), Some(&user[0]));
        assert_eq!(merged.len(), DEFAULT_KEYBINDINGS.len());
    }

    #[test]
    fn key_parse_and_labels() {
        let k = KeyChord::parse("mod+shift+k").unwrap();
        assert_eq!(k.label(false), "Ctrl+Shift+K");
        assert_eq!(k.label(true), "⇧⌘K");
        assert_eq!(KeyChord::parse("mod++").unwrap().key, "+");
        assert_eq!(KeyChord::parse("esc").unwrap().label(false), "Esc");
        assert_eq!(
            KeyChord::parse("alt+arrowup").unwrap().label(false),
            "Alt+Up"
        );
        assert!(KeyChord::parse("mod+shift").is_none());
        assert!(KeyChord::parse("hyper+k").is_none());
    }

    #[test]
    fn when_grammar() {
        let w = When::parse("composerFocus && !draftThreadRoute || (isMac && x)").unwrap();
        let ctx = |s: &str| s == "composerFocus";
        assert!(w.eval(&ctx));
        assert!(!When::parse("false").unwrap().eval(&|_| true));
        assert!(When::parse("a &&").is_none());
        assert!(When::parse("a & b").is_none());
        assert!(When::parse("(a").is_none());
    }

    #[test]
    fn invalid_keybinding_entries_are_dropped_and_reported() {
        let dir = tempdir();
        std::fs::write(
            dir.join(KEYBINDINGS_FILE),
            r#"[
              {"key": "mod+y", "command": "sidebar.toggle"},
              {"key": "mod+y", "command": "not.a.command"},
              {"key": "", "command": "terminal.toggle"},
              {"key": "mod+1", "command": "script.build-all.run"},
            ]"#,
        )
        .unwrap();
        let l = load_keybindings(&dir);
        assert_eq!(l.dropped_keys.len(), 2);
        assert!(l.value.iter().any(|r| r.command == "script.build-all.run"));
        assert_eq!(
            command_label("script.build-all.run"),
            "Run Script: Build All"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
