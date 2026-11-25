//! The settings model (spec 06 §16): `settings.json`, `client-settings.json`,
//! `keybindings.json` and `ui-state.json` under `<TERN_HOME>/userdata`.
//!
//! One entity per app. Every change is written back right away and its live effect is
//! applied: theme mode, named theme, contrast, glass opacity and fonts go to `Theme`;
//! server settings are re-read by the engine from disk on use; views read client
//! settings here:
//!
//! ```ignore
//! let model = SettingsModel::global(cx);
//! let width = model.read(cx).client().chat_width;
//! cx.observe(&model, |_, _, cx| cx.notify()).detach();
//! ```

use std::path::PathBuf;

use gpui::{App, AppContext as _, Context, Entity, Global, SharedString, Window};
use serde_json::{Map, Value};
use tern_core::settings::{
    self, ClientSettings, KEYBINDINGS_FILE, KeybindingRule, Settings, load_keybindings,
    save_keybindings, strip_jsonc, write_atomic,
};

use super::themes;
use crate::app_state::AppState;
use crate::theme::{Theme, ThemeMode, Typography};
use crate::ui::{Toast, Toasts};

pub const UI_STATE_FILE: &str = "ui-state.json";

/// `ui-state.json` keys (T3's `t3code:*` localStorage keys minus the prefix, §9.5).
pub mod ui_keys {
    pub const THEME: &str = "theme";
    pub const APPEARANCE_MODE: &str = "theme-appearance-mode";
    pub const HALVES: &str = "theme-halves:v1";
    pub const TYPOGRAPHY_ADVANCED: &str = "typography-advanced";
}

pub struct SettingsModel {
    /// None when there is nowhere to write (no home directory).
    userdata: Option<PathBuf>,
    settings: Settings,
    client: ClientSettings,
    /// The rules in `keybindings.json` (not merged with the defaults).
    keybinding_rules: Vec<KeybindingRule>,
    /// Entries of `keybindings.json` that were ignored.
    keybinding_issues: Vec<String>,
    ui: Map<String, Value>,
    /// `settings.json` did not parse: run on defaults and refuse writes (§16.2).
    settings_error: Option<String>,
    /// Bumped by Restore device defaults so the page drops its drafts.
    epoch: u64,
}

struct GlobalSettingsModel(Entity<SettingsModel>);
impl Global for GlobalSettingsModel {}

fn read_ui_state(path: &std::path::Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&strip_jsonc(&t)).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

impl SettingsModel {
    pub fn load(userdata: Option<PathBuf>) -> Self {
        let Some(dir) = userdata else {
            return Self {
                userdata: None,
                settings: Settings::default(),
                client: ClientSettings::default(),
                keybinding_rules: Vec::new(),
                keybinding_issues: Vec::new(),
                ui: Map::new(),
                settings_error: None,
                epoch: 0,
            };
        };
        let s = Settings::load(&dir);
        let c = ClientSettings::load(&dir);
        let k = settings::load_keybindings(&dir);
        let file_rules = read_keybinding_file(&dir);
        let mut issues = k.dropped_keys.clone();
        if let Some(e) = k.unreadable {
            issues.push(e);
        }
        Self {
            settings: s.value,
            client: c.value,
            keybinding_rules: file_rules,
            keybinding_issues: issues,
            ui: read_ui_state(&dir.join(UI_STATE_FILE)),
            settings_error: s.unreadable,
            userdata: Some(dir),
            epoch: 0,
        }
    }

    /// The model, created from `TERN_HOME` on first use.
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(g) = cx.try_global::<GlobalSettingsModel>() {
            return g.0.clone();
        }
        let dir = tern_engine::Paths::from_env().userdata();
        let entity = cx.new(|_| Self::load(Some(dir)));
        cx.set_global(GlobalSettingsModel(entity.clone()));
        entity
    }

    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalSettingsModel>().map(|g| g.0.clone())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn client(&self) -> &ClientSettings {
        &self.client
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn userdata(&self) -> Option<&PathBuf> {
        self.userdata.as_ref()
    }

    pub fn keybinding_rules(&self) -> &[KeybindingRule] {
        &self.keybinding_rules
    }

    pub fn keybinding_issues(&self) -> &[String] {
        &self.keybinding_issues
    }

    /// Defaults plus `keybindings.json`, the bindings in effect.
    pub fn effective_keybindings(&self) -> Vec<KeybindingRule> {
        settings::merge_keybindings(&self.keybinding_rules)
    }

    pub fn ui_str(&self, key: &str) -> Option<&str> {
        self.ui.get(key).and_then(Value::as_str)
    }

    pub fn ui_bool(&self, key: &str) -> bool {
        self.ui.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn ui_value(&self, key: &str) -> Option<&Value> {
        self.ui.get(key)
    }

    // --- writes ----------------------------------------------------------------

    /// Changes client settings, saves, and applies live effects.
    pub fn update_client(
        &mut self,
        f: impl FnOnce(&mut ClientSettings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.client.clone();
        f(&mut self.client);
        if before == self.client {
            return;
        }
        if let Some(dir) = &self.userdata
            && let Err(e) = self.client.save(dir)
        {
            save_failed(&e.to_string(), cx);
        }
        self.apply_client_effects(window, cx);
        cx.notify();
    }

    /// Changes server settings and saves `settings.json` (sparse).
    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        if let Some(err) = &self.settings_error {
            Toasts::push(
                Toast::error("Settings not saved").description(format!(
                    "settings.json could not be read, so it is not overwritten: {err}"
                )),
                cx,
            );
            return;
        }
        let before = self.settings.clone();
        f(&mut self.settings);
        if before == self.settings {
            return;
        }
        if let Some(dir) = &self.userdata
            && let Err(e) = self.settings.save(dir)
        {
            save_failed(&e.to_string(), cx);
            self.settings = before;
        }
        cx.notify();
    }

    /// Replaces the rules in `keybindings.json`.
    pub fn set_keybinding_rules(
        &mut self,
        rules: Vec<KeybindingRule>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(dir) = &self.userdata {
            save_keybindings(dir, &rules).map_err(|e| e.to_string())?;
        }
        self.keybinding_rules = rules;
        cx.notify();
        Ok(())
    }

    /// Re-reads `keybindings.json` (after an external edit).
    pub fn reload_keybindings(&mut self, cx: &mut Context<Self>) {
        if let Some(dir) = &self.userdata {
            let k = load_keybindings(dir);
            self.keybinding_rules = read_keybinding_file(dir);
            self.keybinding_issues = k.dropped_keys;
            if let Some(e) = k.unreadable {
                self.keybinding_issues.push(e);
            }
            cx.notify();
        }
    }

    pub fn keybindings_path(&self) -> Option<PathBuf> {
        self.userdata.as_ref().map(|d| d.join(KEYBINDINGS_FILE))
    }

    /// Sets (or with `None` removes) a `ui-state.json` key, keeping keys other code wrote.
    pub fn set_ui(&mut self, key: &str, value: Option<Value>, cx: &mut Context<Self>) {
        if self.ui.get(key) == value.as_ref() {
            return;
        }
        match value {
            Some(v) => self.ui.insert(key.to_string(), v),
            None => self.ui.remove(key),
        };
        if let Some(dir) = &self.userdata {
            let path = dir.join(UI_STATE_FILE);
            let mut on_disk = read_ui_state(&path);
            match self.ui.get(key) {
                Some(v) => {
                    on_disk.insert(key.to_string(), v.clone());
                }
                None => {
                    on_disk.remove(key);
                }
            }
            let mut text =
                serde_json::to_string_pretty(&Value::Object(on_disk)).unwrap_or_default();
            text.push('\n');
            if let Err(e) =
                std::fs::create_dir_all(dir).and_then(|_| write_atomic(&path, text.as_bytes()))
            {
                save_failed(&e.to_string(), cx);
            }
        }
        cx.notify();
    }

    // --- theme -------------------------------------------------------------------

    /// `theme`: `system`, `light`, `dark` or a theme id.
    pub fn theme_pref(&self) -> &str {
        self.ui_str(ui_keys::THEME).unwrap_or("system")
    }

    /// The color scheme tile that is active (`theme-appearance-mode`, else derived).
    pub fn appearance_mode(&self) -> ThemeMode {
        self.ui_str(ui_keys::APPEARANCE_MODE)
            .and_then(ThemeMode::parse)
            .or_else(|| ThemeMode::parse(self.theme_pref()))
            .unwrap_or(ThemeMode::System)
    }

    /// The theme id painting `dark` (or light) mode: a half, else the theme pref.
    pub fn theme_for(&self, dark: bool) -> String {
        let half = self
            .ui_value(ui_keys::HALVES)
            .and_then(|h| h.get(if dark { "dark" } else { "light" }))
            .and_then(Value::as_str);
        if let Some(id) = half {
            return id.to_string();
        }
        match self.theme_pref() {
            "system" | "light" | "dark" => themes::DEFAULT_ID.to_string(),
            id => themes::canonical_id(id).to_string(),
        }
    }

    pub fn set_appearance_mode(
        &mut self,
        mode: ThemeMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = match mode {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        };
        self.set_ui(ui_keys::APPEARANCE_MODE, Some(Value::from(s)), cx);
        // A bare mode string means the stock theme; keep a named theme pick.
        if matches!(self.theme_pref(), "system" | "light" | "dark") {
            self.set_ui(ui_keys::THEME, Some(Value::from(s)), cx);
        }
        self.apply_theme(window, cx);
    }

    /// Uses `id` for both modes (`default` stores the bare mode string).
    pub fn set_theme(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let value = if id == themes::DEFAULT_ID {
            match self.appearance_mode() {
                ThemeMode::System => "system",
                ThemeMode::Light => "light",
                ThemeMode::Dark => "dark",
            }
            .to_string()
        } else {
            id.to_string()
        };
        self.set_ui(ui_keys::THEME, Some(Value::from(value)), cx);
        self.set_ui(ui_keys::HALVES, None, cx);
        self.apply_theme(window, cx);
    }

    /// Assigns `id` to one mode only ("halves").
    pub fn set_theme_half(
        &mut self,
        id: &str,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut halves = match self.ui_value(ui_keys::HALVES) {
            Some(Value::Object(m)) => m.clone(),
            _ => {
                let mut m = Map::new();
                m.insert("light".into(), Value::from(self.theme_for(false)));
                m.insert("dark".into(), Value::from(self.theme_for(true)));
                m
            }
        };
        halves.insert(
            if dark { "dark" } else { "light" }.into(),
            Value::from(id.to_string()),
        );
        self.set_ui(ui_keys::HALVES, Some(Value::Object(halves)), cx);
        self.apply_theme(window, cx);
    }

    /// Pushes mode and named-theme colors into `Theme`.
    pub fn apply_theme(&self, window: &mut Window, cx: &mut App) {
        let mode = self.appearance_mode();
        let light = self.theme_for(false);
        let dark = self.theme_for(true);
        Theme::set_palette_override(themes::palette_override(&light, &dark), window, cx);
        let app = AppState::global(cx);
        if app.read(cx).theme_mode() != mode {
            app.update(cx, |s, cx| s.set_theme_mode(mode, window, cx));
        } else {
            Theme::set_mode(mode, window, cx);
        }
    }

    // --- effects -----------------------------------------------------------------

    pub fn typography(&self) -> Typography {
        let c = &self.client;
        let separate = self.ui_bool(ui_keys::TYPOGRAPHY_ADVANCED);
        let mono: SharedString = c.font_family_code.clone().into();
        Typography {
            sans: c.font_family_sans.clone().into(),
            prompt: if separate {
                c.font_family_composer.clone().into()
            } else {
                "".into()
            },
            code: mono.clone(),
            terminal: if separate {
                c.font_family_terminal.clone().into()
            } else {
                mono
            },
            interface_size: c.font_size_interface.clamp(12, 20) as f32,
            prompt_size: if separate {
                c.font_size_prompt.clamp(12, 20) as f32
            } else {
                crate::theme::font::PROMPT_SIZE
            },
            code_size: c.font_size_code.clamp(10, 18) as f32,
            terminal_size: if separate {
                c.font_size_terminal.clamp(8, 20) as f32
            } else {
                c.font_size_code.clamp(8, 20) as f32
            },
            word_wrap: c.word_wrap,
        }
    }

    /// Contrast, glass opacity and fonts.
    pub fn apply_client_effects(&self, window: &mut Window, cx: &mut App) {
        let c = &self.client;
        let contrast = c.appearance_contrast.clamp(50, 200) as u16;
        if cx.global::<Theme>().contrast != contrast {
            Theme::set_contrast(contrast, window, cx);
        }
        Theme::set_glass_opacity(c.glass_opacity.clamp(40, 100) as u16, window, cx);
        Theme::set_typography(self.typography(), window, cx);
    }

    /// Startup: client effects, plus the saved theme unless `--theme` picked one.
    pub fn apply_startup(&self, window: &mut Window, cx: &mut App) {
        self.apply_client_effects(window, cx);
        let flag_mode = AppState::global(cx).read(cx).theme_mode();
        let light = self.theme_for(false);
        let dark = self.theme_for(true);
        Theme::set_palette_override(themes::palette_override(&light, &dark), window, cx);
        if flag_mode == ThemeMode::System && self.appearance_mode() != ThemeMode::System {
            let mode = self.appearance_mode();
            AppState::global(cx).update(cx, |s, cx| s.set_theme_mode(mode, window, cx));
        }
    }

    // --- restore device defaults -------------------------------------------------

    /// Labels of device settings that differ from their defaults (§1.2).
    pub fn changed_device_labels(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut push = |label: &'static str| {
            if !out.contains(&label) {
                out.push(label);
            }
        };
        if self.theme_pref() != "system"
            || self.ui_value(ui_keys::HALVES).is_some()
            || self.appearance_mode() != ThemeMode::System
        {
            push("Theme");
        }
        let current = to_map(&self.client);
        let defaults = to_map(&ClientSettings::default());
        for (key, label) in CLIENT_RESTORE_KEYS {
            if current.get(*key) != defaults.get(*key) {
                push(label);
            }
        }
        if self.ui_bool(ui_keys::TYPOGRAPHY_ADVANCED) {
            push("Separate fonts");
        }
        let current = to_map(&self.settings);
        let defaults = to_map(&Settings::default());
        for (key, label) in SERVER_RESTORE_KEYS {
            if current.get(*key) != defaults.get(*key) {
                push(label);
            }
        }
        out
    }

    /// Resets the theme and every device key (plus the listed server keys).
    pub fn restore_device_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_ui(ui_keys::THEME, Some(Value::from("system")), cx);
        self.set_ui(ui_keys::APPEARANCE_MODE, Some(Value::from("system")), cx);
        self.set_ui(ui_keys::HALVES, None, cx);
        self.set_ui(ui_keys::TYPOGRAPHY_ADVANCED, None, cx);
        self.update_client(
            |c| {
                let mut map = to_map(c);
                let defaults = to_map(&ClientSettings::default());
                for (key, _) in CLIENT_RESTORE_KEYS {
                    if let Some(d) = defaults.get(*key) {
                        map.insert((*key).to_string(), d.clone());
                    }
                }
                if let Ok(next) = serde_json::from_value(Value::Object(map)) {
                    *c = next;
                }
            },
            window,
            cx,
        );
        self.update_settings(
            |s| {
                let mut map = to_map(s);
                let defaults = to_map(&Settings::default());
                for (key, _) in SERVER_RESTORE_KEYS {
                    match defaults.get(*key) {
                        Some(d) => {
                            map.insert((*key).to_string(), d.clone());
                        }
                        None => {
                            map.remove(*key);
                        }
                    }
                }
                if let Ok(next) = serde_json::from_value(Value::Object(map)) {
                    *s = next;
                }
            },
            cx,
        );
        self.apply_theme(window, cx);
        self.epoch += 1;
        cx.notify();
    }
}

fn save_failed(detail: &str, cx: &mut App) {
    Toasts::push(
        Toast::error("Could not save settings").description(detail.to_string()),
        cx,
    );
}

fn to_map<T: serde::Serialize>(value: &T) -> Map<String, Value> {
    match serde_json::to_value(value) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// The rules written in `keybindings.json` (valid entries only), before merging.
fn read_keybinding_file(dir: &std::path::Path) -> Vec<KeybindingRule> {
    let Ok(text) = std::fs::read_to_string(dir.join(KEYBINDINGS_FILE)) else {
        return Vec::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<Value>>(&strip_jsonc(&text)) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|e| serde_json::from_value::<KeybindingRule>(e).ok())
        .filter(|r| settings::validate_rule(r).is_ok())
        .collect()
}

/// Device keys Restore device defaults resets, with the label it lists.
pub const CLIENT_RESTORE_KEYS: &[(&str, &str)] = &[
    ("notificationMode", "Thread notifications"),
    ("inAppNotificationsEnabled", "In-app notifications"),
    ("sidebarProjectGroupingMode", "Project grouping"),
    ("sidebarProjectSortOrder", "Project order"),
    ("sidebarThreadSortOrder", "Thread order"),
    ("sidebarWorkingShelfEnabled", "Working section"),
    ("confirmThreadUnpin", "Unpin confirmation"),
    ("confirmThreadArchive", "Archive confirmation"),
    ("confirmThreadDelete", "Delete confirmation"),
    ("confirmTerminalClose", "Terminal close confirmation"),
    ("confirmQuit", "Quit shortcut"),
    ("timestampFormat", "Time format"),
    ("composerRichTextEnabled", "Rich text composer"),
    ("sendShortcut", "Send shortcut"),
    ("showSkillsInSlashMenu", "Show skills in slash menu"),
    ("followUpBehavior", "Follow-up behavior"),
    ("sideThreadOpenTarget", "Open subagents and side chats in"),
    ("toolCallVisibility", "Tool calls in chat"),
    ("screenReaderAnnouncements", "Screen reader announcements"),
    ("screenReaderWindowTitle", "Screen reader window title"),
    ("diffLayout", "Diff layout"),
    ("diffIgnoreWhitespace", "Hide whitespace changes"),
    ("diffFilesCollapsed", "Default diff file state"),
    ("diffColorScheme", "Diff colors"),
    (
        "proactivePanelsEnabled",
        "Open pull requests and changes panels proactively",
    ),
    ("pullRequestLinksOpenInApp", "Open pull requests in Tern"),
    ("appearanceContrast", "Contrast"),
    ("glassOpacity", "Glass opacity"),
    ("chatWidth", "Chat width"),
    ("threadDetailsItems", "Thread details"),
    ("effortDisplay", "Effort display"),
    ("mathRenderingEnabled", "Render math"),
    ("panelAnimationDurationMs", "Panel animations"),
    ("persistComposerContextStrip", "Composer context"),
    ("fontFamilySans", "Interface font"),
    ("fontSizeInterface", "Interface font"),
    ("fontFamilyComposer", "Prompt font"),
    ("fontSizePrompt", "Prompt font"),
    ("fontFamilyCode", "Code font"),
    ("fontSizeCode", "Code font"),
    ("fontFamilyTerminal", "Terminal font"),
    ("fontSizeTerminal", "Terminal font"),
    ("wordWrap", "Word wrap"),
    ("fontSmoothing", "Font smoothing"),
];

/// Server keys Restore device defaults also resets (§1.2).
pub const SERVER_RESTORE_KEYS: &[(&str, &str)] = &[
    ("responseStreamingMode", "Response streaming"),
    ("enableProviderUpdateChecks", "Provider update checks"),
    (
        "continueThreadsAfterServerUpdate",
        "Continue threads after restarts",
    ),
    ("backgroundActivity", "Background activity"),
    ("keepAwakeWhileAgentsWork", "Keep awake while agents work"),
    ("backgroundActivityProfile", "Background activity"),
    ("automaticGitFetchInterval", "Git fetch interval"),
    ("providerHealthRefreshInterval", "Health check interval"),
    ("defaultThreadEnvMode", "Workspace"),
    ("newWorktreesStartFromOrigin", "Start from remote"),
    ("addProjectBaseDirectory", "Add project starts in"),
    ("ghqCloneLayout", "Clone into host folders"),
    ("textGenerationModelSelection", "Text generation model"),
    ("threadTitleInstructions", "Thread title instructions"),
    ("sidebarAutoSettleAfterDays", "Auto-settle inactive threads"),
    ("sidebarAutoSettleOnMerge", "Auto-settle merged threads"),
    (
        "sidebarAutoSettleKeepPullRequests",
        "Keep threads with a PR",
    ),
    ("autoResumeLimitedThreads", "Resume after usage limits"),
    ("snoozeLimitedThreads", "Snooze until limits reset"),
    ("snoozePresets", "Snooze options"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tern-settings-model-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_list_nothing_to_restore() {
        let m = SettingsModel::load(None);
        assert!(m.changed_device_labels().is_empty());
    }

    #[test]
    fn changed_keys_list_their_labels_once() {
        let mut m = SettingsModel::load(None);
        m.client.font_size_code = 15;
        m.client.font_family_code = "Iosevka".into();
        m.client.diff_layout = settings::DiffLayout::Split;
        m.settings.ghq_clone_layout = true;
        assert_eq!(
            m.changed_device_labels(),
            vec!["Diff layout", "Code font", "Clone into host folders"]
        );
    }

    #[test]
    fn ui_state_keeps_foreign_keys() {
        let dir = temp_dir();
        std::fs::write(dir.join(UI_STATE_FILE), "{\"sidebarWidth\": 300}").unwrap();
        let m = SettingsModel::load(Some(dir.clone()));
        assert_eq!(m.theme_pref(), "system");
        let on_disk = read_ui_state(&dir.join(UI_STATE_FILE));
        assert_eq!(on_disk.get("sidebarWidth"), Some(&Value::from(300)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn halves_pick_per_mode() {
        let mut m = SettingsModel::load(None);
        m.ui.insert("theme".into(), Value::from("t3-grove"));
        assert_eq!(m.theme_for(true), "grove");
        let mut h = Map::new();
        h.insert("dark".into(), Value::from("iris"));
        m.ui.insert(ui_keys::HALVES.into(), Value::Object(h));
        assert_eq!(m.theme_for(true), "iris");
        assert_eq!(m.theme_for(false), "grove");
    }
}
