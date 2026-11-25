//! Attachment intake (spec 04 §5): the paperclip picker, paste and drop. Files are
//! copied to `<userdata>/attachments/<id>`, where the engine reads them on send.

use std::path::{Path, PathBuf};

use gpui::{
    AppContext as _, ClipboardEntry, ClipboardItem, Context, ExternalPaths, PathPromptOptions,
    Window,
};
use tern_core::{ChatAttachment, MAX_ATTACHMENTS, MAX_FILE_BYTES, MAX_IMAGE_BYTES};

use super::ComposerView;
use super::drafts::DraftAttachment;
use crate::ui::{Toast, Toasts};

/// Pasted text this large becomes a `pasted-text.txt` attachment.
pub const LARGE_PASTE_BYTES: usize = 32 * 1024;

const IMAGE_EXTENSIONS: [(&str, &str); 7] = [
    ("gif", "image/gif"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("png", "image/png"),
    ("webp", "image/webp"),
];

/// The image MIME type for a file name, if it is a supported image.
pub fn image_mime(name: &str) -> Option<&'static str> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    IMAGE_EXTENSIONS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, m)| *m)
}

fn file_mime(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "md" | "rs" | "py" | "ts" | "tsx" | "js" | "toml" | "yaml" | "yml" | "sh" => {
            "text/plain".into()
        }
        "json" => "application/json".into(),
        "pdf" => "application/pdf".into(),
        _ => "application/octet-stream".into(),
    }
}

/// Checks a file against the limits; Err is the message to show.
pub fn validate(name: &str, size: u64, image: bool, count: usize) -> Result<(), String> {
    if count >= MAX_ATTACHMENTS {
        return Err(format!(
            "You can attach up to {MAX_ATTACHMENTS} files per message."
        ));
    }
    if size == 0 {
        return Err(format!("'{name}' is empty or could not be read."));
    }
    if image && size > MAX_IMAGE_BYTES {
        return Err(format!(
            "'{name}' is too large to attach, even after compression."
        ));
    }
    if !image && size > MAX_FILE_BYTES {
        return Err(format!("'{name}' is too large to attach."));
    }
    Ok(())
}

/// The next free `pasted-text.txt`, `pasted-text-2.txt`, ... name.
pub fn pasted_text_name(existing: &[String]) -> String {
    let mut n = 1;
    loop {
        let name = if n == 1 {
            "pasted-text.txt".to_string()
        } else {
            format!("pasted-text-{n}.txt")
        };
        if !existing.contains(&name) {
            return name;
        }
        n += 1;
    }
}

fn attachments_dir(cx: &gpui::App) -> PathBuf {
    let store = crate::store::Store::global(cx);
    let store = store.read(cx);
    match store.engine() {
        Some(engine) => engine.paths().attachments(),
        None => tern_engine::Paths::from_env().attachments(),
    }
}

/// Copies `bytes` into the attachments folder and returns the draft entry.
fn store_bytes(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<DraftAttachment> {
    std::fs::create_dir_all(dir)?;
    let id = tern_core::new_id();
    let path = dir.join(&id);
    std::fs::write(&path, bytes)?;
    let size_bytes = bytes.len() as u64;
    let attachment = match image_mime(name) {
        Some(mime) => ChatAttachment::Image {
            id,
            name: name.to_string(),
            mime_type: mime.to_string(),
            size_bytes,
        },
        None => ChatAttachment::File {
            id,
            name: name.to_string(),
            mime_type: file_mime(name),
            size_bytes,
        },
    };
    Ok(DraftAttachment { attachment, path })
}

impl ComposerView {
    fn add_attachment(&mut self, attachment: DraftAttachment, cx: &mut Context<Self>) {
        self.attachments.push(attachment);
        let list = self.attachments.clone();
        if let Some(key) = self.key.clone() {
            self.drafts.update(&key, |d| d.attachments = list);
            self.schedule_save(cx);
        }
        cx.notify();
    }

    pub(crate) fn remove_attachment(&mut self, id: &str, cx: &mut Context<Self>) {
        self.attachments.retain(|a| a.attachment.id() != id);
        let list = self.attachments.clone();
        if let Some(key) = self.key.clone() {
            self.drafts.update(&key, |d| d.attachments = list);
            self.schedule_save(cx);
        }
        cx.notify();
    }

    /// Reads files from disk on a background thread and attaches the ones that fit.
    pub(crate) fn attach_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let dir = attachments_dir(cx);
        let start = self.attachments.len();
        let task = cx.background_spawn(async move {
            let mut out: Vec<Result<DraftAttachment, String>> = Vec::new();
            for (i, path) in paths.into_iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into());
                if path.is_dir() {
                    continue;
                }
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                if let Err(e) = validate(&name, size, image_mime(&name).is_some(), start + i) {
                    out.push(Err(e));
                    continue;
                }
                match std::fs::read(&path) {
                    Ok(bytes) => out.push(
                        store_bytes(&dir, &name, &bytes).map_err(|e| format!("'{name}': {e}")),
                    ),
                    Err(_) => out.push(Err(format!("'{name}' is empty or could not be read."))),
                }
            }
            out
        });
        cx.spawn(async move |this, cx| {
            let results = task.await;
            this.update(cx, |this, cx| {
                for r in results {
                    match r {
                        Ok(a) => this.add_attachment(a, cx),
                        Err(e) => {
                            Toasts::push(Toast::error(e), cx);
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn pick_files(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |this, cx| this.attach_paths(paths, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(crate) fn on_drop_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        self.attach_paths(paths.paths().to_vec(), cx);
    }

    /// Paste hook: images and copied files attach; very large text becomes a file.
    /// Returns true when the paste was consumed.
    pub(crate) fn on_paste(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
        let dir = attachments_dir(cx);
        let mut consumed = false;
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(image) => {
                    consumed = true;
                    let ext = match image.format {
                        gpui::ImageFormat::Png => "png",
                        gpui::ImageFormat::Jpeg => "jpg",
                        gpui::ImageFormat::Webp => "webp",
                        gpui::ImageFormat::Gif => "gif",
                        _ => "png",
                    };
                    let name = format!("pasted-image.{ext}");
                    if let Err(e) = validate(
                        &name,
                        image.bytes.len() as u64,
                        true,
                        self.attachments.len(),
                    ) {
                        Toasts::push(Toast::error(e), cx);
                        continue;
                    }
                    match store_bytes(&dir, &name, &image.bytes) {
                        Ok(a) => self.add_attachment(a, cx),
                        Err(e) => {
                            Toasts::push(Toast::error(e.to_string()), cx);
                        }
                    }
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    consumed = true;
                    self.attach_paths(paths.paths().to_vec(), cx);
                }
                ClipboardEntry::String(s) => {
                    let text = s.text();
                    let total = self.text(cx).len() + text.len();
                    if text.len() >= LARGE_PASTE_BYTES || total > tern_core::MAX_INPUT_CHARS {
                        consumed = true;
                        let names: Vec<String> = self
                            .attachments
                            .iter()
                            .map(|a| a.attachment.name().to_string())
                            .collect();
                        let name = pasted_text_name(&names);
                        match store_bytes(&dir, &name, text.as_bytes()) {
                            Ok(a) => {
                                self.add_attachment(a, cx);
                                Toasts::push(
                                    Toast::info(format!("Large paste attached as {name}")),
                                    cx,
                                );
                            }
                            Err(e) => {
                                Toasts::push(Toast::error(e.to_string()), cx);
                            }
                        }
                    }
                }
            }
        }
        consumed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_types_by_extension() {
        assert_eq!(image_mime("a.PNG"), Some("image/png"));
        assert_eq!(image_mime("photo.jpg"), Some("image/jpeg"));
        assert_eq!(image_mime("notes.txt"), None);
        assert_eq!(image_mime("noext"), None);
    }

    #[test]
    fn limits() {
        assert!(validate("a.png", 10, true, 0).is_ok());
        assert!(validate("a.png", 0, true, 0).unwrap_err().contains("empty"));
        assert!(validate("a.png", MAX_IMAGE_BYTES + 1, true, 0).is_err());
        assert!(validate("a.bin", MAX_IMAGE_BYTES + 1, false, 0).is_ok());
        assert!(
            validate("a.bin", 1, false, MAX_ATTACHMENTS)
                .unwrap_err()
                .contains("up to 100")
        );
    }

    #[test]
    fn pasted_names_count_up() {
        assert_eq!(pasted_text_name(&[]), "pasted-text.txt");
        assert_eq!(
            pasted_text_name(&["pasted-text.txt".into(), "pasted-text-2.txt".into()]),
            "pasted-text-3.txt"
        );
    }

    #[test]
    fn stores_bytes_under_an_id() {
        let dir = std::env::temp_dir().join(format!("tern-attach-{}", std::process::id()));
        let a = store_bytes(&dir, "x.png", b"png").unwrap();
        assert!(matches!(a.attachment, ChatAttachment::Image { .. }));
        assert_eq!(std::fs::read(&a.path).unwrap(), b"png");
        assert_eq!(a.path, dir.join(a.attachment.id()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
