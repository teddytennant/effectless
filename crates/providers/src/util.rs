//! Small helpers shared by both adapters.

use std::path::Path;

use serde_json::Value;
use tern_core::*;
use tokio::sync::mpsc;

/// Event sink that never fails: a dropped receiver just means nobody is listening.
#[derive(Clone)]
pub(crate) struct Emitter(pub mpsc::UnboundedSender<ProviderEvent>);

impl Emitter {
    pub fn send(&self, event: ProviderEvent) {
        let _ = self.0.send(event);
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Image types both providers take inline (`isProviderSendTurnSupportedImageMimeType`).
pub(crate) fn is_native_image(att: &ChatAttachment) -> bool {
    matches!(att, ChatAttachment::Image { mime_type, .. }
        if matches!(mime_type.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp"))
}

/// `providerMessageTextWithAttachmentPaths` (packages/provider-core/src/server/attachmentPrompt.ts:160):
/// every attachment's saved path is appended so the agent can open it itself.
pub(crate) fn text_with_attachment_paths(
    text: &str,
    attachments: &[(ChatAttachment, std::path::PathBuf)],
) -> String {
    let mut out = text.to_string();
    for (att, path) in attachments {
        let kind = match att {
            ChatAttachment::Image { .. } => "image",
            ChatAttachment::File { .. } => "file",
        };
        let line = format!(
            "[Attached {kind} \"{}\" is saved at: {}]",
            att.name(),
            path.display()
        );
        if out.is_empty() {
            out = line;
        } else {
            out.push_str("\n\n");
            out.push_str(&line);
        }
    }
    out
}

pub(crate) fn mime_of(att: &ChatAttachment) -> &str {
    match att {
        ChatAttachment::Image { mime_type, .. } | ChatAttachment::File { mime_type, .. } => {
            mime_type
        }
    }
}

pub(crate) fn read_base64(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("Failed to read attachment {}: {e}", path.display()))?;
    Ok(base64(&bytes))
}

pub(crate) fn failure(class: FailureClass, message: impl Into<String>) -> ProviderFailure {
    ProviderFailure {
        class,
        message: message.into(),
        code: None,
        retryable: None,
        reset_at: None,
    }
}

/// Cuts to at most `max` chars, appending `…` when it did.
pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub(crate) fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub(crate) fn u64_of(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| {
        x.as_u64()
            .or_else(|| x.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
    })
}

/// Epoch seconds to an RFC 3339 string.
pub(crate) fn epoch_to_iso(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// `+`/`-` line counts of a unified diff, ignoring the `---`/`+++` headers.
pub(crate) fn diff_counts(diff: &str) -> (u32, u32) {
    let (mut add, mut del) = (0, 0);
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            add += 1;
        } else if line.starts_with('-') {
            del += 1;
        }
    }
    (add, del)
}

/// A unified-diff hunk for an in-place string replacement (Claude `Edit`). Lines the
/// old and new text share at either end become context.
pub(crate) fn replacement_diff(path: &str, old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut out = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
        a.len(),
        b.len()
    );
    for l in &a[..prefix] {
        out.push_str(&format!(" {l}\n"));
    }
    for l in &a[prefix..a.len() - suffix] {
        out.push_str(&format!("-{l}\n"));
    }
    for l in &b[prefix..b.len() - suffix] {
        out.push_str(&format!("+{l}\n"));
    }
    for l in &a[a.len() - suffix..] {
        out.push_str(&format!(" {l}\n"));
    }
    out
}

/// Answers from the UI are `string | string[] | {answers: string[]}` (UserInputAnswerPayload).
pub(crate) fn answer_strings(v: &Value) -> Vec<String> {
    match v {
        Value::Null => vec![],
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        Value::Object(o) => match o.get("answers") {
            Some(inner @ Value::Array(_)) => answer_strings(inner),
            _ => vec![v.to_string()],
        },
        other => vec![other.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn attachment_paths_are_appended() {
        let att = ChatAttachment::File {
            id: "a".into(),
            name: "notes.txt".into(),
            mime_type: "text/plain".into(),
            size_bytes: 3,
        };
        let text = text_with_attachment_paths("hi", &[(att, "/s/a".into())]);
        assert_eq!(
            text,
            "hi\n\n[Attached file \"notes.txt\" is saved at: /s/a]"
        );
    }

    #[test]
    fn diff_counts_skip_headers() {
        assert_eq!(diff_counts("--- a/x\n+++ b/x\n@@\n-a\n+b\n+c\n"), (2, 1));
    }

    #[test]
    fn replacement_diff_keeps_shared_lines_as_context() {
        let d = replacement_diff("x", "a\nb\nz", "a\nc\nz");
        assert_eq!(d, "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\n+c\n z\n");
        assert_eq!(diff_counts(&d), (1, 1));
        assert_eq!(diff_counts(&replacement_diff("x", "", "n\nm")), (2, 0));
    }

    #[test]
    fn answers_normalize() {
        assert_eq!(answer_strings(&serde_json::json!(["a", 1])), vec!["a", "1"]);
        assert_eq!(
            answer_strings(&serde_json::json!({"answers": ["x"]})),
            vec!["x"]
        );
        assert_eq!(answer_strings(&serde_json::json!("y")), vec!["y"]);
    }
}
