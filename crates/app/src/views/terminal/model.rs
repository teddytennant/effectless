//! Per-thread terminal UI state (spec 05 §3.1): which terminals a thread has, how they
//! are grouped into tabs and splits, which one is active, and the drawer's open flag
//! and height. Pure data; the drawer view owns one per thread and the PTYs separately.

use crate::app_state::drawer;

pub const DEFAULT_TERMINAL_ID: &str = "term-1";
pub const MAX_TERMINALS_PER_GROUP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    /// Side by side (columns).
    Horizontal,
    /// Stacked (rows).
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalGroup {
    pub id: String,
    pub terminal_ids: Vec<String>,
    pub split: Option<SplitDirection>,
}

/// What a group header shows in the sidebar (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    Single,
    Stacked,
    SideBySide,
}

impl GroupKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::Stacked => "Stacked",
            Self::SideBySide => "Side by side",
        }
    }
}

impl TerminalGroup {
    pub fn kind(&self) -> GroupKind {
        if self.terminal_ids.len() <= 1 {
            return GroupKind::Single;
        }
        match self.split {
            Some(SplitDirection::Vertical) => GroupKind::Stacked,
            _ => GroupKind::SideBySide,
        }
    }

    /// Panes lay out side by side unless the group is stacked.
    pub fn direction(&self) -> SplitDirection {
        self.split.unwrap_or(SplitDirection::Horizontal)
    }
}

/// `ThreadTerminalUiState`.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadTerminals {
    pub open: bool,
    /// The user's preferred height in px; rendering clamps it.
    pub height: f32,
    /// Creation order; drives the sidebar order.
    pub terminal_ids: Vec<String>,
    pub active_terminal_id: String,
    pub groups: Vec<TerminalGroup>,
    pub active_group_id: String,
}

impl Default for ThreadTerminals {
    fn default() -> Self {
        Self {
            open: false,
            height: f32::from(drawer::DEFAULT_HEIGHT),
            terminal_ids: Vec::new(),
            active_terminal_id: String::new(),
            groups: Vec::new(),
            active_group_id: String::new(),
        }
    }
}

/// Lowest unused `term-N` (N ≥ 1) not in `taken`.
pub fn next_terminal_id<'a>(taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
    (1..)
        .map(|n| format!("term-{n}"))
        .find(|id| !taken.clone().into_iter().any(|t| t == id))
        .expect("ids are unbounded")
}

/// The label a terminal shows (§3.4): the running foreground command when there is
/// one, else `Terminal N` for `term-N` ids, else the raw id.
pub fn terminal_label(id: &str, running_command: Option<&str>) -> String {
    if let Some(cmd) = running_command.filter(|c| !c.is_empty()) {
        return cmd.to_string();
    }
    match id
        .strip_prefix("term-")
        .and_then(|n| n.split('-').next())
        .and_then(|n| n.parse::<u32>().ok())
    {
        Some(n) => format!("Terminal {n}"),
        None => id.to_string(),
    }
}

impl ThreadTerminals {
    pub fn is_empty(&self) -> bool {
        self.terminal_ids.is_empty()
    }

    pub fn len(&self) -> usize {
        self.terminal_ids.len()
    }

    pub fn has(&self, id: &str) -> bool {
        self.terminal_ids.iter().any(|t| t == id)
    }

    pub fn group_of(&self, id: &str) -> Option<&TerminalGroup> {
        self.groups
            .iter()
            .find(|g| g.terminal_ids.iter().any(|t| t == id))
    }

    pub fn active_group(&self) -> Option<&TerminalGroup> {
        self.groups
            .iter()
            .find(|g| g.id == self.active_group_id)
            .or_else(|| self.group_of(&self.active_terminal_id))
            .or_else(|| self.groups.first())
    }

    /// Whether the active group can take another split.
    pub fn can_split(&self) -> bool {
        self.active_group()
            .is_none_or(|g| g.terminal_ids.len() < MAX_TERMINALS_PER_GROUP)
    }

    fn unique_group_id(&self, terminal_id: &str) -> String {
        let base = format!("group-{terminal_id}");
        if !self.groups.iter().any(|g| g.id == base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !self.groups.iter().any(|g| &g.id == id))
            .expect("ids are unbounded")
    }

    /// Appends `id` as a new single-terminal group, makes it active and opens the
    /// drawer (`newTerminal`).
    pub fn new_terminal(&mut self, id: &str) {
        if self.has(id) {
            self.set_active(id);
            self.open = true;
            return;
        }
        self.terminal_ids.push(id.to_string());
        let group_id = self.unique_group_id(id);
        self.groups.push(TerminalGroup {
            id: group_id.clone(),
            terminal_ids: vec![id.to_string()],
            split: None,
        });
        self.active_terminal_id = id.to_string();
        self.active_group_id = group_id;
        self.open = true;
        self.normalize();
    }

    /// Inserts `id` into the active group right after the active terminal and sets the
    /// whole group's direction (`splitTerminal`). Refuses at 4 per group. With no
    /// terminals yet it behaves as [`Self::new_terminal`].
    pub fn split(&mut self, id: &str, direction: SplitDirection) -> bool {
        if self.is_empty() {
            self.new_terminal(id);
            return true;
        }
        if self.has(id) || !self.can_split() {
            return false;
        }
        let active = self.active_terminal_id.clone();
        let Some(group_id) = self.active_group().map(|g| g.id.clone()) else {
            self.new_terminal(id);
            return true;
        };
        let group = self
            .groups
            .iter_mut()
            .find(|g| g.id == group_id)
            .expect("active group exists");
        let at = group
            .terminal_ids
            .iter()
            .position(|t| *t == active)
            .map_or(group.terminal_ids.len(), |i| i + 1);
        group.terminal_ids.insert(at, id.to_string());
        group.split = Some(direction);
        self.terminal_ids.push(id.to_string());
        self.active_terminal_id = id.to_string();
        self.active_group_id = group_id;
        self.open = true;
        self.normalize();
        true
    }

    /// Activates `id` and its group.
    pub fn set_active(&mut self, id: &str) {
        if !self.has(id) {
            return;
        }
        self.active_terminal_id = id.to_string();
        if let Some(g) = self.group_of(id) {
            self.active_group_id = g.id.clone();
        }
    }

    /// Activates a group's remembered active member, else its first.
    pub fn activate_group(&mut self, group_id: &str) {
        let Some(group) = self.groups.iter().find(|g| g.id == group_id) else {
            return;
        };
        let id = if group.terminal_ids.contains(&self.active_terminal_id) {
            self.active_terminal_id.clone()
        } else {
            group.terminal_ids[0].clone()
        };
        self.set_active(&id);
    }

    /// Removes `id` (`closeTerminal`). The new active terminal is the next one in the
    /// same group, else the previous one, else the one at the same index overall, else
    /// the first. Closing the last terminal resets the record, which closes the drawer
    /// but keeps the height preference.
    pub fn close(&mut self, id: &str) {
        let Some(index) = self.terminal_ids.iter().position(|t| t == id) else {
            return;
        };
        let was_active = self.active_terminal_id == id;
        let mut replacement = None;
        if let Some(group) = self
            .groups
            .iter_mut()
            .find(|g| g.terminal_ids.iter().any(|t| t == id))
        {
            let gi = group.terminal_ids.iter().position(|t| t == id).unwrap();
            group.terminal_ids.remove(gi);
            replacement = group
                .terminal_ids
                .get(gi)
                .or_else(|| gi.checked_sub(1).and_then(|p| group.terminal_ids.get(p)))
                .cloned();
            if group.terminal_ids.len() <= 1 {
                group.split = None;
            }
        }
        self.terminal_ids.remove(index);
        if self.terminal_ids.is_empty() {
            *self = Self {
                height: self.height,
                ..Self::default()
            };
            return;
        }
        if was_active {
            let next = replacement
                .or_else(|| self.terminal_ids.get(index).cloned())
                .unwrap_or_else(|| self.terminal_ids[0].clone());
            self.active_terminal_id = next;
            self.active_group_id.clear();
        }
        self.normalize();
    }

    /// Adds `id` as a new group if unknown (`ensureTerminal`).
    pub fn ensure(&mut self, id: &str, open: bool, active: bool) {
        if !self.has(id) {
            let (prev_active, prev_open) = (self.active_terminal_id.clone(), self.open);
            self.new_terminal(id);
            if !active && !prev_active.is_empty() {
                self.set_active(&prev_active);
            }
            self.open = prev_open;
        } else if active {
            self.set_active(id);
        }
        if open {
            self.open = true;
        }
    }

    /// Groups ordered by the earliest creation index of their members.
    pub fn sorted_groups(&self) -> Vec<&TerminalGroup> {
        let rank = |g: &TerminalGroup| {
            g.terminal_ids
                .iter()
                .filter_map(|id| self.terminal_ids.iter().position(|t| t == id))
                .min()
                .unwrap_or(usize::MAX)
        };
        let mut groups: Vec<&TerminalGroup> = self.groups.iter().collect();
        groups.sort_by_key(|g| rank(g));
        groups
    }

    /// The terminal `delta` steps from the active one in group-sorted order, wrapping
    /// (`terminal.previous` / `terminal.next`).
    pub fn cycle(&self, delta: i32) -> Option<String> {
        let order: Vec<&String> = self
            .sorted_groups()
            .into_iter()
            .flat_map(|g| g.terminal_ids.iter())
            .collect();
        if order.is_empty() {
            return None;
        }
        let at = order
            .iter()
            .position(|t| **t == self.active_terminal_id)
            .unwrap_or(0) as i32;
        let n = order.len() as i32;
        Some(order[(at + delta).rem_euclid(n) as usize].clone())
    }

    /// Trims and dedupes ids, drops group members that aren't terminals, gives every
    /// ungrouped terminal its own group, and repairs the active ids.
    pub fn normalize(&mut self) {
        let mut seen: Vec<String> = Vec::new();
        for id in self.terminal_ids.drain(..) {
            let id = id.trim().to_string();
            if !id.is_empty() && !seen.contains(&id) {
                seen.push(id);
            }
        }
        self.terminal_ids = seen;
        let ids = self.terminal_ids.clone();
        let mut grouped: Vec<String> = Vec::new();
        for g in &mut self.groups {
            g.terminal_ids
                .retain(|t| ids.contains(t) && !grouped.contains(t) && {
                    grouped.push(t.clone());
                    true
                });
            if g.terminal_ids.len() <= 1 {
                g.split = None;
            }
        }
        self.groups.retain(|g| !g.terminal_ids.is_empty());
        for id in &ids {
            if !grouped.contains(id) {
                let group_id = self.unique_group_id(id);
                self.groups.push(TerminalGroup {
                    id: group_id,
                    terminal_ids: vec![id.clone()],
                    split: None,
                });
            }
        }
        if !self.has(&self.active_terminal_id.clone()) {
            self.active_terminal_id = ids.first().cloned().unwrap_or_default();
        }
        let active_group_ok = self
            .groups
            .iter()
            .any(|g| g.id == self.active_group_id && g.terminal_ids.contains(&self.active_terminal_id));
        if !active_group_ok {
            self.active_group_id = self
                .group_of(&self.active_terminal_id)
                .or_else(|| self.groups.first())
                .map(|g| g.id.clone())
                .unwrap_or_default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(ids: &[&str]) -> ThreadTerminals {
        let mut s = ThreadTerminals::default();
        for id in ids {
            s.new_terminal(id);
        }
        s
    }

    #[test]
    fn next_id_fills_gaps() {
        assert_eq!(next_terminal_id(Vec::<&str>::new()), "term-1");
        assert_eq!(next_terminal_id(["term-1", "term-2"]), "term-3");
        assert_eq!(next_terminal_id(["term-1", "term-3"]), "term-2");
        assert_eq!(next_terminal_id(["term-2"]), "term-1");
    }

    #[test]
    fn labels() {
        assert_eq!(terminal_label("term-1", None), "Terminal 1");
        assert_eq!(terminal_label("term-12", Some("")), "Terminal 12");
        assert_eq!(terminal_label("term-3", Some("vim")), "vim");
        assert_eq!(terminal_label("term-2-abc", None), "Terminal 2");
        assert_eq!(terminal_label("setup-lint", None), "setup-lint");
    }

    #[test]
    fn new_terminal_opens_and_activates() {
        let s = with(&["term-1", "term-2"]);
        assert!(s.open);
        assert_eq!(s.terminal_ids, ["term-1", "term-2"]);
        assert_eq!(s.groups.len(), 2);
        assert_eq!(s.active_terminal_id, "term-2");
        assert_eq!(s.active_group_id, "group-term-2");
    }

    #[test]
    fn split_inserts_after_active_and_flips_direction() {
        let mut s = with(&["term-1"]);
        assert!(s.split("term-2", SplitDirection::Horizontal));
        s.set_active("term-1");
        assert!(s.split("term-3", SplitDirection::Vertical));
        let g = s.active_group().unwrap();
        assert_eq!(g.terminal_ids, ["term-1", "term-3", "term-2"]);
        assert_eq!(g.kind(), GroupKind::Stacked);
        assert_eq!(s.terminal_ids, ["term-1", "term-2", "term-3"]);
        assert_eq!(s.active_terminal_id, "term-3");
    }

    #[test]
    fn split_refuses_past_four() {
        let mut s = with(&["term-1"]);
        for id in ["term-2", "term-3", "term-4"] {
            assert!(s.split(id, SplitDirection::Horizontal));
        }
        assert!(!s.can_split());
        assert!(!s.split("term-5", SplitDirection::Horizontal));
        assert_eq!(s.len(), 4);
        // A new tab is still fine.
        s.new_terminal("term-5");
        assert!(s.can_split());
    }

    #[test]
    fn split_with_nothing_is_new() {
        let mut s = ThreadTerminals::default();
        assert!(s.split("term-1", SplitDirection::Vertical));
        assert_eq!(s.groups[0].split, None);
        assert!(s.open);
    }

    #[test]
    fn close_picks_next_then_previous_in_group() {
        let mut s = with(&["term-1"]);
        s.split("term-2", SplitDirection::Horizontal);
        s.split("term-3", SplitDirection::Horizontal);
        // group: 1, 2, 3; active 3
        s.set_active("term-2");
        s.close("term-2");
        assert_eq!(s.active_terminal_id, "term-3");
        s.close("term-3");
        assert_eq!(s.active_terminal_id, "term-1");
        assert_eq!(s.active_group().unwrap().split, None);
    }

    #[test]
    fn close_falls_back_to_same_index_overall() {
        let mut s = with(&["term-1", "term-2", "term-3"]);
        s.set_active("term-2");
        s.close("term-2");
        assert_eq!(s.active_terminal_id, "term-3");
        s.close("term-3");
        assert_eq!(s.active_terminal_id, "term-1");
    }

    #[test]
    fn closing_inactive_keeps_active() {
        let mut s = with(&["term-1", "term-2"]);
        s.close("term-1");
        assert_eq!(s.active_terminal_id, "term-2");
    }

    #[test]
    fn closing_last_resets_but_keeps_height() {
        let mut s = with(&["term-1"]);
        s.height = 400.;
        s.close("term-1");
        assert!(!s.open);
        assert!(s.is_empty());
        assert_eq!(s.height, 400.);
    }

    #[test]
    fn cycle_wraps_in_group_order() {
        let mut s = with(&["term-1", "term-2"]);
        s.set_active("term-1");
        s.split("term-3", SplitDirection::Horizontal);
        // groups by min index: [1, 3], [2]
        s.set_active("term-1");
        assert_eq!(s.cycle(1).as_deref(), Some("term-3"));
        s.set_active("term-3");
        assert_eq!(s.cycle(1).as_deref(), Some("term-2"));
        s.set_active("term-2");
        assert_eq!(s.cycle(1).as_deref(), Some("term-1"));
        assert_eq!(s.cycle(-1).as_deref(), Some("term-3"));
    }

    #[test]
    fn activate_group_uses_member() {
        let mut s = with(&["term-1", "term-2"]);
        s.activate_group("group-term-1");
        assert_eq!(s.active_terminal_id, "term-1");
    }

    #[test]
    fn ensure_adds_without_stealing_focus() {
        let mut s = with(&["term-1"]);
        s.open = false;
        s.ensure("setup-x", false, false);
        assert!(s.has("setup-x"));
        assert_eq!(s.active_terminal_id, "term-1");
        assert!(!s.open);
        s.ensure("setup-x", true, true);
        assert_eq!(s.active_terminal_id, "setup-x");
        assert!(s.open);
    }

    #[test]
    fn normalize_repairs() {
        let mut s = ThreadTerminals {
            terminal_ids: vec![" term-1 ".into(), "term-1".into(), "term-2".into()],
            groups: vec![TerminalGroup {
                id: "group-term-1".into(),
                terminal_ids: vec!["term-1".into(), "gone".into()],
                split: Some(SplitDirection::Vertical),
            }],
            active_terminal_id: "gone".into(),
            ..Default::default()
        };
        s.normalize();
        assert_eq!(s.terminal_ids, ["term-1", "term-2"]);
        assert_eq!(s.groups.len(), 2);
        assert_eq!(s.groups[0].terminal_ids, ["term-1"]);
        assert_eq!(s.groups[0].split, None);
        assert_eq!(s.active_terminal_id, "term-1");
        assert_eq!(s.active_group_id, "group-term-1");
    }

    #[test]
    fn group_ids_avoid_collisions() {
        let mut s = ThreadTerminals::default();
        s.groups.push(TerminalGroup {
            id: "group-term-1".into(),
            terminal_ids: vec!["term-9".into()],
            split: None,
        });
        s.terminal_ids.push("term-9".into());
        s.new_terminal("term-1");
        assert!(s.groups.iter().any(|g| g.id == "group-term-1-2"));
    }
}
