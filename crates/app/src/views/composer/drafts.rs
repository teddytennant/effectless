//! Per-thread composer drafts (spec 04 §8.3): one draft per thread or new-thread
//! project, the sticky model per provider, favorites. Kept in memory and written to
//! `<userdata>/composer-drafts.json` (300ms debounce) outside demo mode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tern_core::{ChatAttachment, ModelSelection, RuntimeMode, ThreadEnvMode};

pub const FILE_NAME: &str = "composer-drafts.json";
const VERSION: u32 = 1;

/// Which draft the composer shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DraftKey {
    /// A new thread in a project (`/draft/<project>`).
    New(String),
    Thread(String),
}

impl DraftKey {
    pub fn storage_key(&self) -> String {
        match self {
            Self::New(project) => format!("new:{project}"),
            Self::Thread(id) => format!("thread:{id}"),
        }
    }

    pub fn thread_id(&self) -> Option<&str> {
        match self {
            Self::Thread(id) => Some(id),
            Self::New(_) => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DraftAttachment {
    pub attachment: ChatAttachment,
    /// The stored copy (`<userdata>/attachments/<id>`).
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    pub prompt: String,
    pub attachments: Vec<DraftAttachment>,
    /// An explicit model pick for this draft or thread.
    pub model_selection: Option<ModelSelection>,
    pub runtime_mode: Option<RuntimeMode>,
    /// New-thread drafts: local checkout or a new worktree.
    pub env_mode: Option<ThreadEnvMode>,
    /// New-thread drafts: the branch to run on (local) or branch from (worktree).
    pub branch: Option<String>,
}

impl Draft {
    /// Nothing worth keeping beyond the defaults.
    pub fn is_empty(&self) -> bool {
        self == &Draft::default()
    }

    pub fn has_content(&self) -> bool {
        !self.prompt.trim().is_empty() || !self.attachments.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DraftFile {
    pub version: u32,
    pub drafts: HashMap<String, Draft>,
    /// Last picked selection per provider instance; seeds new drafts.
    pub sticky_model_selection_by_provider: HashMap<String, ModelSelection>,
    /// Provider instance of the last pick.
    pub active_provider: Option<String>,
    /// `instanceId:slug`.
    pub favorites: Vec<String>,
}

pub struct DraftStore {
    pub file: DraftFile,
    path: Option<PathBuf>,
}

impl DraftStore {
    /// In memory only (demo mode, tests).
    pub fn ephemeral() -> Self {
        Self {
            file: DraftFile {
                version: VERSION,
                ..Default::default()
            },
            path: None,
        }
    }

    /// Loads `<dir>/composer-drafts.json`; a missing or unreadable file starts empty.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let file = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<DraftFile>(&bytes).ok())
            .unwrap_or(DraftFile {
                version: VERSION,
                ..Default::default()
            });
        Self {
            file,
            path: Some(path),
        }
    }

    pub fn get(&self, key: &DraftKey) -> Draft {
        self.file
            .drafts
            .get(&key.storage_key())
            .cloned()
            .unwrap_or_default()
    }

    pub fn put(&mut self, key: &DraftKey, draft: Draft) {
        let k = key.storage_key();
        if draft.is_empty() {
            self.file.drafts.remove(&k);
        } else {
            self.file.drafts.insert(k, draft);
        }
    }

    pub fn update(&mut self, key: &DraftKey, f: impl FnOnce(&mut Draft)) {
        let mut draft = self.get(key);
        f(&mut draft);
        self.put(key, draft);
    }

    pub fn remember_pick(&mut self, sel: &ModelSelection) {
        self.file
            .sticky_model_selection_by_provider
            .insert(sel.instance_id.clone(), sel.clone());
        self.file.active_provider = Some(sel.instance_id.clone());
    }

    /// The last pick, used to seed new drafts.
    pub fn sticky(&self) -> Option<&ModelSelection> {
        let active = self.file.active_provider.as_ref()?;
        self.file.sticky_model_selection_by_provider.get(active)
    }

    pub fn is_favorite(&self, instance: &str, slug: &str) -> bool {
        let key = format!("{instance}:{slug}");
        self.file.favorites.contains(&key)
    }

    pub fn toggle_favorite(&mut self, instance: &str, slug: &str) {
        let key = format!("{instance}:{slug}");
        if let Some(i) = self.file.favorites.iter().position(|f| f == &key) {
            self.file.favorites.remove(i);
        } else {
            self.file.favorites.push(key);
        }
    }

    pub fn persists(&self) -> bool {
        self.path.is_some()
    }

    /// The JSON to write, and where. None in memory-only mode.
    pub fn snapshot(&self) -> Option<(PathBuf, Vec<u8>)> {
        let path = self.path.clone()?;
        let bytes = serde_json::to_vec_pretty(&self.file).ok()?;
        Some((path, bytes))
    }
}

/// Writes atomically (temp file then rename).
pub fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_drafts_are_dropped() {
        let mut store = DraftStore::ephemeral();
        let key = DraftKey::Thread("t1".into());
        store.update(&key, |d| d.prompt = "hello".into());
        assert_eq!(store.get(&key).prompt, "hello");
        store.update(&key, |d| d.prompt.clear());
        assert!(store.file.drafts.is_empty());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("tern-drafts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = DraftStore::load(&dir);
        let key = DraftKey::New("p1".into());
        store.update(&key, |d| {
            d.prompt = "draft".into();
            d.runtime_mode = Some(RuntimeMode::ApprovalRequired);
        });
        store.toggle_favorite("claudeAgent", "opus");
        let sel = ModelSelection {
            instance_id: "codex".into(),
            model: "gpt".into(),
            options: vec![],
        };
        store.remember_pick(&sel);
        let (path, bytes) = store.snapshot().unwrap();
        write_file(&path, &bytes).unwrap();
        let again = DraftStore::load(&dir);
        assert_eq!(again.get(&key).prompt, "draft");
        assert!(again.is_favorite("claudeAgent", "opus"));
        assert_eq!(again.sticky(), Some(&sel));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_file_starts_empty() {
        let dir = std::env::temp_dir().join(format!("tern-drafts-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), "{not json").unwrap();
        let store = DraftStore::load(&dir);
        assert!(store.file.drafts.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
