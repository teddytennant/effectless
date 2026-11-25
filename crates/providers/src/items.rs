//! Per-turn item bookkeeping shared by the adapters: stable Tern ids for native items,
//! in-turn ordinals, delta coalescing and settling everything that is still open.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tern_core::*;

use crate::util::Emitter;

pub(crate) struct ItemTracker {
    thread_id: String,
    run_id: Option<String>,
    next_ordinal: u32,
    items: HashMap<String, TurnItem>,
    order: Vec<String>,
    /// Keys whose latest snapshot hasn't been emitted (text deltas, 50 ms flush).
    dirty: HashSet<String>,
    /// Plan deltas flush on their own, slower clock (400 ms).
    dirty_slow: HashSet<String>,
}

impl ItemTracker {
    pub fn new(thread_id: &str, run_id: Option<String>) -> Self {
        Self {
            thread_id: thread_id.to_string(),
            run_id,
            next_ordinal: 1,
            items: HashMap::new(),
            order: Vec::new(),
            dirty: HashSet::new(),
            dirty_slow: HashSet::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&TurnItem> {
        self.items.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.items.contains_key(key)
    }

    /// Drops an item that was never emitted (e.g. an empty thinking block).
    pub fn discard(&mut self, key: &str) {
        self.items.remove(key);
        self.order.retain(|k| k != key);
        self.dirty.remove(key);
        self.dirty_slow.remove(key);
    }

    /// Creates the item on first sight (fresh Tern id, next ordinal), otherwise replaces
    /// its body and status. Returns the snapshot to emit.
    pub fn upsert(
        &mut self,
        key: &str,
        native_id: Option<&str>,
        body: TurnItemBody,
        status: ItemStatus,
        title: Option<String>,
    ) -> TurnItem {
        let now = now();
        let item = self.items.entry(key.to_string()).or_insert_with(|| {
            let ordinal = self.next_ordinal;
            self.next_ordinal += 1;
            self.order.push(key.to_string());
            TurnItem {
                id: new_id(),
                thread_id: self.thread_id.clone(),
                run_id: self.run_id.clone(),
                native_item_id: native_id.map(str::to_string),
                parent_item_id: None,
                ordinal,
                status,
                title: None,
                started_at: Some(now),
                completed_at: None,
                updated_at: now,
                body: body.clone(),
            }
        });
        item.body = body;
        item.status = status;
        if title.is_some() {
            item.title = title;
        }
        item.updated_at = now;
        if !status.is_open() && item.completed_at.is_none() {
            item.completed_at = Some(now);
        }
        self.dirty.remove(key);
        self.dirty_slow.remove(key);
        item.clone()
    }

    /// Edits an existing item in place and returns the new snapshot.
    pub fn update(&mut self, key: &str, f: impl FnOnce(&mut TurnItem)) -> Option<TurnItem> {
        let item = self.items.get_mut(key)?;
        f(item);
        let now = now();
        item.updated_at = now;
        if !item.status.is_open() && item.completed_at.is_none() {
            item.completed_at = Some(now);
        }
        self.dirty.remove(key);
        self.dirty_slow.remove(key);
        Some(item.clone())
    }

    /// Edits without emitting; the change goes out on the next `flush`.
    pub fn update_quiet(&mut self, key: &str, slow: bool, f: impl FnOnce(&mut TurnItem)) -> bool {
        let Some(item) = self.items.get_mut(key) else {
            return false;
        };
        f(item);
        item.updated_at = now();
        if slow {
            self.dirty_slow.insert(key.to_string());
        } else {
            self.dirty.insert(key.to_string());
        }
        true
    }

    pub fn take_dirty(&mut self, include_slow: bool) -> Vec<TurnItem> {
        let mut keys: Vec<String> = self.dirty.drain().collect();
        if include_slow {
            keys.extend(self.dirty_slow.drain());
        }
        let mut out: Vec<TurnItem> = keys
            .iter()
            .filter_map(|k| self.items.get(k).cloned())
            .collect();
        out.sort_by_key(|i| i.ordinal);
        out
    }

    /// Closes every open item: streaming text stops streaming, tools and requests take
    /// `status`. Returns the snapshots that changed, in ordinal order.
    pub fn settle_open(&mut self, status: ItemStatus) -> Vec<TurnItem> {
        let mut out = Vec::new();
        let now = now();
        for key in &self.order {
            let Some(item) = self.items.get_mut(key) else {
                continue;
            };
            let mut changed = self.dirty.remove(key) | self.dirty_slow.remove(key);
            match &mut item.body {
                TurnItemBody::AssistantMessage { streaming, .. }
                | TurnItemBody::Reasoning { streaming, .. }
                | TurnItemBody::ProposedPlan { streaming, .. }
                    if *streaming =>
                {
                    *streaming = false;
                    changed = true;
                }
                _ => {}
            }
            if item.status.is_open() {
                let text_like = matches!(
                    item.body,
                    TurnItemBody::AssistantMessage { .. }
                        | TurnItemBody::Reasoning { .. }
                        | TurnItemBody::ProposedPlan { .. }
                );
                item.status = if text_like && status == ItemStatus::Interrupted {
                    ItemStatus::Completed
                } else {
                    status
                };
                changed = true;
            }
            if changed {
                item.updated_at = now;
                if item.completed_at.is_none() {
                    item.completed_at = Some(now);
                }
                out.push(item.clone());
            }
        }
        out
    }
}

/// A pending approval or question as the engine sees it.
pub(crate) fn runtime_request(
    id: String,
    thread_id: &str,
    run_id: Option<String>,
    turn_item_id: &str,
    native_request_id: Value,
    kind: RequestKind,
    live_session_id: &str,
) -> RuntimeRequest {
    RuntimeRequest {
        id,
        thread_id: thread_id.to_string(),
        run_id,
        turn_item_id: turn_item_id.to_string(),
        native_request_id: Some(native_request_id),
        kind,
        status: RequestStatus::Pending,
        live_session_id: Some(live_session_id.to_string()),
        created_at: now(),
        resolved_at: None,
        decision: None,
        feedback: None,
        answers: None,
    }
}

/// Stamps the request id into an approval/question body.
pub(crate) fn with_request_id(mut body: TurnItemBody, id: &str) -> TurnItemBody {
    match &mut body {
        TurnItemBody::ApprovalRequest { request_id, .. }
        | TurnItemBody::UserInputRequest { request_id, .. } => {
            *request_id = id.to_string();
        }
        _ => {}
    }
    body
}

pub(crate) fn emit_items(emitter: &Emitter, items: Vec<TurnItem>) {
    for item in items {
        emitter.send(ProviderEvent::ItemUpserted(item));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> TurnItemBody {
        TurnItemBody::AssistantMessage {
            message_id: "m".into(),
            text: t.into(),
            streaming: true,
            phase: None,
        }
    }

    #[test]
    fn ids_are_stable_and_ordinals_grow() {
        let mut t = ItemTracker::new("th", Some("r".into()));
        let a = t.upsert("a", Some("na"), text(""), ItemStatus::Running, None);
        let b = t.upsert("b", None, text(""), ItemStatus::Running, None);
        let a2 = t.upsert("a", Some("na"), text("x"), ItemStatus::Running, None);
        assert_eq!(a.id, a2.id);
        assert_eq!((a.ordinal, b.ordinal), (1, 2));
    }

    #[test]
    fn coalesced_updates_flush_once() {
        let mut t = ItemTracker::new("th", None);
        t.upsert("a", None, text(""), ItemStatus::Running, None);
        for d in ["a", "b", "c"] {
            t.update_quiet("a", false, |i| {
                if let TurnItemBody::AssistantMessage { text, .. } = &mut i.body {
                    text.push_str(d);
                }
            });
        }
        let out = t.take_dirty(false);
        assert_eq!(out.len(), 1);
        assert!(
            matches!(&out[0].body, TurnItemBody::AssistantMessage { text, .. } if text == "abc")
        );
        assert!(t.take_dirty(true).is_empty());
    }

    #[test]
    fn settle_closes_tools_and_stops_streaming() {
        let mut t = ItemTracker::new("th", None);
        t.upsert("msg", None, text("hi"), ItemStatus::Running, None);
        t.upsert(
            "cmd",
            None,
            TurnItemBody::CommandExecution {
                input: "ls".into(),
                output: None,
                exit_code: None,
                output_indicates_failure: None,
            },
            ItemStatus::Running,
            None,
        );
        let out = t.settle_open(ItemStatus::Interrupted);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].status, ItemStatus::Completed);
        assert!(matches!(
            out[0].body,
            TurnItemBody::AssistantMessage {
                streaming: false,
                ..
            }
        ));
        assert_eq!(out[1].status, ItemStatus::Interrupted);
        assert!(t.settle_open(ItemStatus::Interrupted).is_empty());
    }
}
