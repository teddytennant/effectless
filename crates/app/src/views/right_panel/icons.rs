//! File-type icons for tabs and trees: Pierre's built-in tree icons (`@pierre/trees`
//! 1.0.0, monochrome, tinted per type) plus T3's toml override (`pierre-icons.ts`),
//! bundled as `assets/icons/lucide/file-type-*.svg`.

use gpui::Hsla;

use crate::syntax::language_for_path;
use crate::tokens::c;
use crate::ui::IconName;

/// `--trees-icon-*` as (light, dark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tint {
    Blue,
    Cyan,
    Gray,
    Green,
    Indigo,
    Orange,
    Pink,
    Purple,
    Red,
    Teal,
    Vermilion,
    Yellow,
}

impl Tint {
    fn color(self, dark: bool) -> Hsla {
        let (light, dark_c) = match self {
            Tint::Blue => (0x1a85d4ff, 0x69b1ffff),
            Tint::Cyan => (0x1ca1c7ff, 0x68cdf2ff),
            Tint::Gray => (0x84848aff, 0xadadb1ff),
            Tint::Green => (0x199f43ff, 0x5ecc71ff),
            Tint::Indigo => (0x693acfff, 0x9d6afbff),
            Tint::Orange => (0xd47628ff, 0xffa359ff),
            Tint::Pink => (0xd32a61ff, 0xff678dff),
            Tint::Purple => (0xa631beff, 0xd568eaff),
            Tint::Red => (0xd52c36ff, 0xff6762ff),
            Tint::Teal => (0x17a5afff, 0x64d1dbff),
            // Pierre's own light-dark order for vermilion.
            Tint::Vermilion => (0xff8c5bff, 0xd5512fff),
            Tint::Yellow => (0xd5a910ff, 0xffd452ff),
        };
        c(if dark { dark_c } else { light })
    }
}

fn icon_for(path: &str) -> (IconName, Tint) {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" => {
            return (IconName::FileTypeGit, Tint::Vermilion);
        }
        "dockerfile" | ".dockerignore" => return (IconName::FileTypeDocker, Tint::Blue),
        "package.json" | ".npmrc" => return (IconName::FileTypeNpm, Tint::Red),
        _ => {}
    }
    if lower.ends_with(".lock") || lower.ends_with("-lock.json") || lower.ends_with("-lock.yaml") {
        return (IconName::FileTypeLock, Tint::Gray);
    }
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "tsx" | "jsx" => return (IconName::FileTypeReact, Tint::Cyan),
        "svg" => return (IconName::FileTypeSvg, Tint::Orange),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "avif" => {
            return (IconName::FileTypeImage, Tint::Pink);
        }
        "csv" | "tsv" => return (IconName::FileTypeTable, Tint::Teal),
        "ttf" | "otf" | "woff" | "woff2" => return (IconName::FileTypeFont, Tint::Gray),
        "zip" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "7z" => {
            return (IconName::FileTypeZip, Tint::Orange);
        }
        "db" | "sqlite" | "sqlite3" | "sql" => return (IconName::FileTypeDatabase, Tint::Purple),
        "tf" | "tfvars" => return (IconName::FileTypeTerraform, Tint::Indigo),
        "scss" | "sass" => return (IconName::FileTypeSass, Tint::Pink),
        "vue" => return (IconName::FileTypeVue, Tint::Green),
        "txt" | "log" => return (IconName::FileTypeText, Tint::Gray),
        _ => {}
    }
    match language_for_path(path) {
        "python" => (IconName::FileTypePython, Tint::Blue),
        "rust" => (IconName::FileTypeRust, Tint::Orange),
        "json" => (IconName::FileTypeJson, Tint::Orange),
        "typescript" => (IconName::FileTypeTypescript, Tint::Blue),
        "javascript" => (IconName::FileTypeJavascript, Tint::Yellow),
        "yaml" => (IconName::FileTypeYml, Tint::Red),
        "markdown" => (IconName::FileTypeMarkdown, Tint::Green),
        "bash" => (IconName::FileTypeBash, Tint::Green),
        "html" => (IconName::FileTypeHtml, Tint::Orange),
        "css" => (IconName::FileTypeCss, Tint::Indigo),
        "go" => (IconName::FileTypeGo, Tint::Cyan),
        "c" => (IconName::FileTypeC, Tint::Blue),
        "cpp" => (IconName::FileTypeCpp, Tint::Blue),
        "swift" => (IconName::FileTypeSwift, Tint::Orange),
        "ruby" => (IconName::FileTypeRuby, Tint::Red),
        "zig" => (IconName::FileTypeZig, Tint::Orange),
        "svelte" => (IconName::FileTypeSvelte, Tint::Red),
        "graphql" => (IconName::FileTypeGraphql, Tint::Pink),
        "toml" => (IconName::FileTypeToml, Tint::Gray),
        _ => (IconName::FileTypeDefault, Tint::Gray),
    }
}

/// The icon and tint for a file path.
pub fn file_icon(path: &str, dark: bool) -> (IconName, Hsla) {
    let (icon, tint) = icon_for(path);
    (icon, tint.color(dark))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_types() {
        assert_eq!(icon_for("notes.py").0, IconName::FileTypePython);
        assert_eq!(
            icon_for(".gitignore"),
            (IconName::FileTypeGit, Tint::Vermilion)
        );
        assert_eq!(icon_for("pyproject.toml").0, IconName::FileTypeToml);
        assert_eq!(icon_for("README.md").0, IconName::FileTypeMarkdown);
        assert_eq!(icon_for("Cargo.lock").0, IconName::FileTypeLock);
        assert_eq!(icon_for("src/App.tsx").0, IconName::FileTypeReact);
        assert_eq!(icon_for("LICENSE").0, IconName::FileTypeDefault);
    }
}
