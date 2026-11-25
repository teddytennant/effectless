//! What the thread is waiting on, as the composer sees it (spec 04 §7, §8.1):
//! open approvals, the open question request, queued follow-ups, the live run.

use std::collections::HashMap;

use tern_core::{
    RequestKind, RequestStatus, RunStatus, ThreadDetail, TurnItemBody, UserInputQuestion,
};

/// The oldest pending user-input request and its questions.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingQuestion {
    pub request_id: String,
    pub questions: Vec<UserInputQuestion>,
}

pub fn pending_question(detail: &ThreadDetail) -> Option<PendingQuestion> {
    let request = detail
        .requests
        .iter()
        .filter(|r| r.kind == RequestKind::UserInput && r.status == RequestStatus::Pending)
        .min_by_key(|r| r.created_at)?;
    let questions = detail.items.iter().find_map(|item| match &item.body {
        TurnItemBody::UserInputRequest {
            request_id,
            questions,
            ..
        } if *request_id == request.id => Some(questions.clone()),
        _ => None,
    })?;
    (!questions.is_empty()).then(|| PendingQuestion {
        request_id: request.id.clone(),
        questions,
    })
}

/// Pending approval request ids (commands, file changes, permissions).
pub fn pending_approvals(detail: &ThreadDetail) -> Vec<String> {
    detail
        .requests
        .iter()
        .filter(|r| r.kind != RequestKind::UserInput && r.status == RequestStatus::Pending)
        .map(|r| r.id.clone())
        .collect()
}

/// The run a Stop would interrupt.
pub fn live_run(detail: &ThreadDetail) -> Option<&tern_core::Run> {
    detail
        .runs
        .iter()
        .rev()
        .find(|r| r.status.is_live() && r.status != RunStatus::Queued)
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueuedRun {
    pub run_id: String,
    pub text: String,
    pub image_count: usize,
}

/// Queued follow-ups in queue order.
pub fn queued_runs(detail: &ThreadDetail) -> Vec<QueuedRun> {
    let mut runs: Vec<&tern_core::Run> = detail
        .runs
        .iter()
        .filter(|r| r.status == RunStatus::Queued)
        .collect();
    runs.sort_by_key(|r| (r.queue_position.unwrap_or(u32::MAX), r.ordinal));
    runs.into_iter()
        .map(|run| {
            let (text, image_count) = detail
                .items
                .iter()
                .find_map(|i| match &i.body {
                    TurnItemBody::UserMessage {
                        message_id,
                        text,
                        attachments,
                        ..
                    } if *message_id == run.user_message_id => Some((
                        text.clone(),
                        attachments
                            .iter()
                            .filter(|a| matches!(a, tern_core::ChatAttachment::Image { .. }))
                            .count(),
                    )),
                    _ => None,
                })
                .unwrap_or_default();
            QueuedRun {
                run_id: run.id.clone(),
                text,
                image_count,
            }
        })
        .collect()
}

/// Previously sent user messages, newest last, for ↑ history (skips consecutive
/// duplicates and strips the `Ultrathink:` prefix).
pub fn prompt_history(detail: &ThreadDetail) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in &detail.items {
        if let TurnItemBody::UserMessage { text, .. } = &item.body {
            let text = text
                .strip_prefix("Ultrathink:\n")
                .unwrap_or(text)
                .to_string();
            if text.trim().is_empty() || out.last() == Some(&text) {
                continue;
            }
            out.push(text);
        }
    }
    out
}

/// A question's answer state: chosen option indexes and custom text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answer {
    pub selected: Vec<usize>,
    pub custom: String,
}

/// Answers keyed by question id in the RespondToRequest shape: a string for
/// single-select or free text, an array for multi-select. Custom text overrides
/// the options. Questions left blank are omitted.
pub fn answers_payload(
    questions: &[UserInputQuestion],
    answers: &HashMap<String, Answer>,
) -> HashMap<String, serde_json::Value> {
    let mut out = HashMap::new();
    for q in questions {
        let Some(a) = answers.get(&q.id) else {
            continue;
        };
        let value_of = |i: &usize| {
            q.options.get(*i).map(|o| {
                o.value
                    .clone()
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| o.label.clone())
            })
        };
        let custom = a.custom.trim();
        let value = if !custom.is_empty() {
            serde_json::Value::String(custom.to_string())
        } else if q.multi_select.unwrap_or(false) {
            let vals: Vec<serde_json::Value> = a
                .selected
                .iter()
                .filter_map(value_of)
                .map(serde_json::Value::String)
                .collect();
            if vals.is_empty() {
                continue;
            }
            serde_json::Value::Array(vals)
        } else {
            match a.selected.first().and_then(value_of) {
                Some(v) => serde_json::Value::String(v),
                None => continue,
            }
        };
        out.insert(q.id.clone(), value);
    }
    out
}

/// Whether a question has an answer (option or custom text).
pub fn is_answered(q: &UserInputQuestion, a: Option<&Answer>) -> bool {
    a.is_some_and(|a| !a.custom.trim().is_empty() || !a.selected.is_empty())
        || q.options.is_empty() && q.required == Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tern_core::QuestionOption;

    fn q(id: &str, multi: bool, opts: &[&str]) -> UserInputQuestion {
        UserInputQuestion {
            id: id.into(),
            header: "H".into(),
            question: "Q?".into(),
            options: opts
                .iter()
                .map(|l| QuestionOption {
                    label: l.to_string(),
                    description: None,
                    value: None,
                    preview: None,
                })
                .collect(),
            multi_select: Some(multi),
            allow_custom_answer: Some(true),
            initial_answer: None,
            required: None,
        }
    }

    #[test]
    fn payload_shapes() {
        let qs = vec![
            q("a", false, &["x", "y"]),
            q("b", true, &["p", "q", "r"]),
            q("c", false, &[]),
        ];
        let mut answers = HashMap::new();
        answers.insert(
            "a".to_string(),
            Answer {
                selected: vec![1],
                custom: String::new(),
            },
        );
        answers.insert(
            "b".to_string(),
            Answer {
                selected: vec![0, 2],
                custom: String::new(),
            },
        );
        answers.insert(
            "c".to_string(),
            Answer {
                selected: vec![],
                custom: " typed ".into(),
            },
        );
        let p = answers_payload(&qs, &answers);
        assert_eq!(p["a"], serde_json::json!("y"));
        assert_eq!(p["b"], serde_json::json!(["p", "r"]));
        assert_eq!(p["c"], serde_json::json!("typed"));
    }

    #[test]
    fn custom_text_overrides_options() {
        let qs = vec![q("a", false, &["x"])];
        let mut answers = HashMap::new();
        answers.insert(
            "a".to_string(),
            Answer {
                selected: vec![0],
                custom: "mine".into(),
            },
        );
        assert_eq!(
            answers_payload(&qs, &answers)["a"],
            serde_json::json!("mine")
        );
        assert!(is_answered(&qs[0], answers.get("a")));
        assert!(!is_answered(&qs[0], None));
    }
}
