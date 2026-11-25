//! Unsent drafts the sidebar lists (spec 02 §2.9) and marks (the draft pen on a thread row).
//!
//! The composer owns the text; it reports a preview here whenever the draft changes, and the
//! sidebar observes the entity:
//!
//! ```ignore
//! Drafts::update(cx, |d, cx| d.set_project(project_id, text, attachments, cx)); // /draft/<project>
//! Drafts::update(cx, |d, cx| d.set_thread(thread_id, text, cx));               // a thread's composer
//! ```
//!
//! An empty text with no attachments removes the draft.

use std::collections::BTreeMap;

use gpui::{App, AppContext as _, Context, Entity, Global};
use tern_core::{ProjectId, ThreadId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftPreview {
    pub text: String,
    pub attachments: usize,
}

impl DraftPreview {
    /// Line 2 of a draft row: the prompt, else "N attachments".
    pub fn summary(&self) -> String {
        let first = self.text.lines().find(|l| !l.trim().is_empty());
        match first {
            Some(line) => line.trim().to_string(),
            None if self.attachments == 1 => "1 attachment".into(),
            None => format!("{} attachments", self.attachments),
        }
    }
}

#[derive(Default)]
pub struct Drafts {
    /// New-thread drafts by project, in the order they were started.
    projects: BTreeMap<ProjectId, (u64, DraftPreview)>,
    threads: BTreeMap<ThreadId, DraftPreview>,
    seq: u64,
}

struct GlobalDrafts(Entity<Drafts>);
impl Global for GlobalDrafts {}

impl Drafts {
    pub fn global(cx: &mut App) -> Entity<Drafts> {
        if let Some(g) = cx.try_global::<GlobalDrafts>() {
            return g.0.clone();
        }
        let entity = cx.new(|_| Drafts::default());
        cx.set_global(GlobalDrafts(entity.clone()));
        entity
    }

    pub fn update(cx: &mut App, f: impl FnOnce(&mut Drafts, &mut Context<Drafts>)) {
        Self::global(cx).update(cx, f);
    }

    pub fn set_project(
        &mut self,
        project_id: ProjectId,
        text: String,
        attachments: usize,
        cx: &mut Context<Self>,
    ) {
        if text.trim().is_empty() && attachments == 0 {
            if self.projects.remove(&project_id).is_some() {
                cx.notify();
            }
            return;
        }
        let preview = DraftPreview { text, attachments };
        match self.projects.get_mut(&project_id) {
            Some((_, existing)) if *existing == preview => return,
            Some((_, existing)) => *existing = preview,
            None => {
                self.seq += 1;
                self.projects.insert(project_id, (self.seq, preview));
            }
        }
        cx.notify();
    }

    pub fn set_thread(&mut self, thread_id: ThreadId, text: String, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            if self.threads.remove(&thread_id).is_some() {
                cx.notify();
            }
            return;
        }
        let preview = DraftPreview {
            text,
            attachments: 0,
        };
        if self.threads.get(&thread_id) != Some(&preview) {
            self.threads.insert(thread_id, preview);
            cx.notify();
        }
    }

    pub fn discard_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if self.projects.remove(project_id).is_some() {
            cx.notify();
        }
    }

    pub fn discard_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        if self.threads.remove(thread_id).is_some() {
            cx.notify();
        }
    }

    /// New-thread drafts, oldest first.
    pub fn project_drafts(&self) -> Vec<(ProjectId, DraftPreview)> {
        let mut v: Vec<_> = self
            .projects
            .iter()
            .map(|(id, (seq, p))| (*seq, id.clone(), p.clone()))
            .collect();
        v.sort_by_key(|(seq, _, _)| *seq);
        v.into_iter().map(|(_, id, p)| (id, p)).collect()
    }

    pub fn project_draft(&self, project_id: &str) -> Option<&DraftPreview> {
        self.projects.get(project_id).map(|(_, p)| p)
    }

    pub fn has_thread_draft(&self, thread_id: &str) -> bool {
        self.threads.contains_key(thread_id)
    }
}
