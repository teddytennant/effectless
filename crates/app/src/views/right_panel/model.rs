//! The right panel's per-thread tab strip (spec 05 §1.1-1.2, §1.8 store semantics).
//!
//! Open/closed and maximized are window-wide in Tern (`AppState::right_panel`), the
//! tabs are per thread. Closing the last tab closes the panel; the caller applies
//! that to `AppState`.

/// One tab ("surface"). Tern keeps diff, files, file and terminal; browser
/// preview, device mirroring and issues are dropped (spec 05 §1.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Diff,
    /// The file browser (tree, no file open).
    Files,
    /// A workspace file, by path relative to the thread folder.
    File(String),
}

impl Surface {
    /// The id T3 stores (`diff`, `files`, `file:<relPath>`).
    pub fn id(&self) -> String {
        match self {
            Self::Diff => "diff".into(),
            Self::Files => "files".into(),
            Self::File(path) => format!("file:{path}"),
        }
    }

    /// Tab title: "Diff", "Files", or the basename after the last `/` or `\`.
    pub fn title(&self) -> String {
        match self {
            Self::Diff => "Diff".into(),
            Self::Files => "Files".into(),
            Self::File(path) => path.rsplit(['/', '\\']).next().unwrap_or(path).to_string(),
        }
    }
}

/// What `rightPanel.toggle` / `diff.toggle` decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelChange {
    Open,
    Close,
    Keep,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelTabs {
    pub surfaces: Vec<Surface>,
    pub active: Option<Surface>,
    /// Surfaces closed by a close action, most recent last (`view.reopenClosed`).
    pub closed: Vec<Surface>,
}

impl PanelTabs {
    pub fn active_index(&self) -> Option<usize> {
        let active = self.active.as_ref()?;
        self.surfaces.iter().position(|s| s == active)
    }

    /// `open(kind)`: append the surface if missing (at the end) and activate it.
    pub fn open(&mut self, surface: Surface) {
        if !self.surfaces.contains(&surface) {
            self.surfaces.push(surface.clone());
        }
        self.active = Some(surface);
    }

    /// `toggle(kind)`: if the panel is open and that surface is active, close the
    /// panel (tabs kept); else open it.
    pub fn toggle(&mut self, surface: Surface, panel_open: bool) -> PanelChange {
        if panel_open && self.active.as_ref() == Some(&surface) {
            return PanelChange::Close;
        }
        self.open(surface);
        PanelChange::Open
    }

    /// `closeSurface(id)`: remove it; if it was active, activate the tab now at the
    /// same index, else the new last. The panel closes when the last tab goes.
    pub fn close(&mut self, surface: &Surface) -> PanelChange {
        let Some(ix) = self.surfaces.iter().position(|s| s == surface) else {
            return PanelChange::Keep;
        };
        let removed = self.surfaces.remove(ix);
        self.closed.retain(|s| s != &removed);
        self.closed.push(removed);
        if self.active.as_ref() == Some(surface) {
            self.active = self
                .surfaces
                .get(ix)
                .or_else(|| self.surfaces.last())
                .cloned();
        }
        if self.surfaces.is_empty() {
            self.active = None;
            PanelChange::Close
        } else {
            PanelChange::Keep
        }
    }

    /// "Close others": keep only `surface`, which becomes active.
    pub fn close_others(&mut self, surface: &Surface) {
        let others: Vec<Surface> = self
            .surfaces
            .iter()
            .filter(|s| *s != surface)
            .cloned()
            .collect();
        self.surfaces.retain(|s| s == surface);
        self.closed.extend(others);
        self.active = self.surfaces.first().cloned();
    }

    /// "Close to the right": the active tab falls back to the clicked one when it
    /// was among those closed.
    pub fn close_to_right(&mut self, surface: &Surface) {
        let Some(ix) = self.surfaces.iter().position(|s| s == surface) else {
            return;
        };
        let removed: Vec<Surface> = self.surfaces.drain(ix + 1..).collect();
        if self
            .active
            .as_ref()
            .is_some_and(|active| removed.contains(active))
        {
            self.active = Some(surface.clone());
        }
        self.closed.extend(removed);
    }

    /// "Close all": the panel closes.
    pub fn close_all(&mut self) -> PanelChange {
        let removed = std::mem::take(&mut self.surfaces);
        self.closed.extend(removed);
        self.active = None;
        PanelChange::Close
    }

    /// `moveSurface(id, toIndex)`: splice reorder.
    pub fn move_surface(&mut self, from: usize, to: usize) {
        if from >= self.surfaces.len() {
            return;
        }
        let surface = self.surfaces.remove(from);
        let to = to.min(self.surfaces.len());
        self.surfaces.insert(to, surface);
    }

    /// `view.reopenClosed`: reopen the most recently closed surface.
    pub fn reopen_closed(&mut self) -> Option<Surface> {
        while let Some(surface) = self.closed.pop() {
            if !self.surfaces.contains(&surface) {
                self.open(surface.clone());
                return Some(surface);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(p: &str) -> Surface {
        Surface::File(p.into())
    }

    #[test]
    fn titles() {
        assert_eq!(Surface::Diff.title(), "Diff");
        assert_eq!(file("src/notes.py").title(), "notes.py");
        assert_eq!(file("a\\b.rs").title(), "b.rs");
        assert_eq!(file("src/notes.py").id(), "file:src/notes.py");
    }

    #[test]
    fn open_appends_once_and_activates() {
        let mut t = PanelTabs::default();
        t.open(Surface::Diff);
        t.open(file("a.py"));
        t.open(Surface::Diff);
        assert_eq!(t.surfaces, vec![Surface::Diff, file("a.py")]);
        assert_eq!(t.active, Some(Surface::Diff));
    }

    #[test]
    fn toggle_closes_only_when_active_and_open() {
        let mut t = PanelTabs::default();
        assert_eq!(t.toggle(Surface::Diff, false), PanelChange::Open);
        assert_eq!(t.toggle(Surface::Diff, true), PanelChange::Close);
        // Tabs are kept when the panel closes.
        assert_eq!(t.surfaces, vec![Surface::Diff]);
        t.open(file("a.py"));
        assert_eq!(t.toggle(Surface::Diff, true), PanelChange::Open);
        assert_eq!(t.active, Some(Surface::Diff));
    }

    #[test]
    fn close_activates_same_index_then_last() {
        let mut t = PanelTabs::default();
        for s in [Surface::Diff, file("a"), file("b")] {
            t.open(s);
        }
        t.active = Some(file("a"));
        assert_eq!(t.close(&file("a")), PanelChange::Keep);
        assert_eq!(t.active, Some(file("b")));
        assert_eq!(t.close(&file("b")), PanelChange::Keep);
        assert_eq!(t.active, Some(Surface::Diff));
        assert_eq!(t.close(&Surface::Diff), PanelChange::Close);
        assert_eq!(t.active, None);
        assert_eq!(t.reopen_closed(), Some(Surface::Diff));
        assert_eq!(t.active, Some(Surface::Diff));
    }

    #[test]
    fn close_inactive_keeps_active() {
        let mut t = PanelTabs::default();
        t.open(Surface::Diff);
        t.open(file("a"));
        t.close(&Surface::Diff);
        assert_eq!(t.active, Some(file("a")));
    }

    #[test]
    fn close_others_and_to_right() {
        let mut t = PanelTabs::default();
        for s in [Surface::Diff, file("a"), file("b"), file("c")] {
            t.open(s);
        }
        t.close_to_right(&file("a"));
        assert_eq!(t.surfaces, vec![Surface::Diff, file("a")]);
        assert_eq!(t.active, Some(file("a")));
        t.close_others(&Surface::Diff);
        assert_eq!(t.surfaces, vec![Surface::Diff]);
        assert_eq!(t.active, Some(Surface::Diff));
        assert_eq!(t.close_all(), PanelChange::Close);
        assert!(t.surfaces.is_empty());
    }

    #[test]
    fn move_reorders() {
        let mut t = PanelTabs::default();
        for s in [Surface::Diff, file("a"), file("b")] {
            t.open(s);
        }
        t.move_surface(0, 2);
        assert_eq!(t.surfaces, vec![file("a"), file("b"), Surface::Diff]);
        t.move_surface(2, 0);
        assert_eq!(t.surfaces, vec![Surface::Diff, file("a"), file("b")]);
    }
}
