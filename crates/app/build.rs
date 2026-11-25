//! Embeds the bundled icon SVGs and generates `IconName` from the file names in
//! `assets/icons/lucide` (one variant per lucide icon, PascalCase of the file stem).

use std::fmt::Write as _;
use std::path::Path;

fn pascal(stem: &str) -> String {
    stem.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn svgs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "svg").then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    names
}

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let icons = Path::new(&manifest).join("assets/icons");
    println!("cargo:rerun-if-changed=assets/icons");

    let lucide = svgs(&icons.join("lucide"));
    let window = svgs(&icons.join("window"));

    let mut out = String::new();
    out.push_str(
        "/// Every bundled lucide icon. Generated from `assets/icons/lucide` by build.rs.\n",
    );
    out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum IconName {\n");
    for name in &lucide {
        writeln!(out, "    {},", pascal(name)).unwrap();
    }
    out.push_str("}\n\nimpl IconName {\n    /// The lucide file stem, e.g. `panel-left-close`.\n");
    out.push_str("    pub const fn stem(self) -> &'static str {\n        match self {\n");
    for name in &lucide {
        writeln!(out, "            IconName::{} => \"{name}\",", pascal(name)).unwrap();
    }
    out.push_str("        }\n    }\n}\n\n");

    out.push_str("/// (asset path under `icons/`, file contents)\npub(crate) static EMBEDDED_ICONS: &[(&str, &[u8])] = &[\n");
    for name in &lucide {
        writeln!(
            out,
            "    (\"lucide/{name}.svg\", include_bytes!(\"{}/assets/icons/lucide/{name}.svg\")),",
            manifest
        )
        .unwrap();
    }
    for name in &window {
        writeln!(
            out,
            "    (\"window/{name}.svg\", include_bytes!(\"{}/assets/icons/window/{name}.svg\")),",
            manifest
        )
        .unwrap();
    }
    out.push_str("];\n");

    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("icons.rs");
    std::fs::write(dest, out).unwrap();
}
