//! Claude models and options from T3's bundled manifest (spec/07 §4.5, spec/04 §3.2),
//! and how a selection compiles into CLI flags.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use tern_core::*;

use crate::discovery::version_at_least;

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
/// Text generation default (`model.ts:161-169`).
pub const TEXT_GENERATION_MODEL: &str = "claude-haiku-5-5";

#[derive(Deserialize)]
struct Manifest {
    defaults: Defaults,
    models: Vec<ManifestModel>,
    profiles: HashMap<String, Profile>,
}

#[derive(Deserialize)]
struct Defaults {
    chat: String,
}

#[derive(Deserialize)]
struct ManifestModel {
    slug: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    status: String,
    profile: String,
    #[serde(default)]
    adapter: Option<Value>,
}

#[derive(Deserialize)]
struct Profile {
    capabilities: ProfileCaps,
    #[serde(default)]
    adapter: Option<ProfileAdapter>,
}

#[derive(Deserialize)]
struct ProfileCaps {
    #[serde(rename = "optionDescriptors")]
    option_descriptors: Vec<Descriptor>,
}

#[derive(Deserialize)]
struct Descriptor {
    id: String,
    label: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    options: Vec<DescriptorOption>,
    #[serde(default, rename = "promptInjectedValues")]
    prompt_injected: Vec<String>,
}

#[derive(Deserialize)]
struct DescriptorOption {
    id: String,
    label: String,
    #[serde(default, rename = "isDefault")]
    is_default: bool,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct ProfileAdapter {
    #[serde(rename = "claudeCode")]
    claude_code: Option<ClaudeCodeAdapter>,
}

#[derive(Deserialize, Default)]
struct ClaudeCodeAdapter {
    #[serde(default, rename = "effortMap")]
    effort_map: HashMap<String, Option<String>>,
    #[serde(default, rename = "fixedContextWindowTokens")]
    fixed_context_window: Option<u64>,
    #[serde(default, rename = "contextWindowTokens")]
    context_window_tokens: HashMap<String, u64>,
    #[serde(default, rename = "modelSuffixes")]
    model_suffixes: HashMap<String, HashMap<String, String>>,
}

fn manifest() -> &'static Manifest {
    static M: OnceLock<Manifest> = OnceLock::new();
    M.get_or_init(|| serde_json::from_str(include_str!("models.json")).expect("bundled claude manifest parses"))
}

fn min_version(m: &ManifestModel) -> Option<&str> {
    m.adapter.as_ref()?.get("claudeCode")?.get("minVersion")?.as_str()
}

fn find_model(slug: &str) -> Option<&'static ManifestModel> {
    let m = manifest();
    m.models.iter().find(|x| x.slug == slug || x.aliases.iter().any(|a| a == slug))
}

fn adapter_of(profile: &Profile) -> Option<&ClaudeCodeAdapter> {
    profile.adapter.as_ref().and_then(|a| a.claude_code.as_ref())
}

/// `Claude Opus 5.5` -> `Opus 5.5`.
fn short_name(name: &str) -> String {
    name.strip_prefix("Claude ").unwrap_or(name).to_string()
}

/// Picker models. Models newer than the installed CLI (`minVersion`) are left out.
pub fn models(cli_version: Option<&str>) -> Vec<ProviderModel> {
    let m = manifest();
    m.models
        .iter()
        .filter(|x| match (cli_version, min_version(x)) {
            (Some(v), Some(min)) => version_at_least(v, min),
            _ => true,
        })
        .map(|x| {
            let options = m
                .profiles
                .get(&x.profile)
                .map(|p| {
                    p.capabilities
                        .option_descriptors
                        .iter()
                        .map(|d| ModelOptionDescriptor {
                            id: d.id.clone(),
                            label: d.label.clone(),
                            boolean: d.ty == "boolean",
                            choices: d
                                .options
                                .iter()
                                .map(|o| ModelOptionChoice {
                                    value: o.id.clone(),
                                    label: o.label.clone(),
                                    is_default: o.is_default,
                                    description: o.description.clone(),
                                    prompt_injected: d.prompt_injected.contains(&o.id),
                                })
                                .collect(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            ProviderModel {
                slug: x.slug.clone(),
                name: x.name.clone(),
                short_name: Some(short_name(&x.name)),
                description: None,
                is_default: x.slug == m.defaults.chat,
                is_custom: false,
                is_legacy: x.status != "current",
                options,
            }
        })
        .collect()
}

/// A model selection compiled for the CLI (`claudeModelOptions.ts:32-87`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CompiledSelection {
    /// `--model`
    pub api_model: String,
    /// `--effort`
    pub effort: Option<String>,
    /// Prepended to the prompt (`ultrathink`).
    pub prompt_prefix: Option<&'static str>,
    /// Merged into `--settings`.
    pub settings: Map<String, Value>,
    pub context_window: Option<u64>,
}

pub fn compile(sel: &ModelSelection) -> CompiledSelection {
    let model = find_model(&sel.model);
    let profile = model.and_then(|m| manifest().profiles.get(&m.profile));
    let adapter = profile.and_then(adapter_of);
    let default_of = |id: &str| -> Option<String> {
        profile?
            .capabilities
            .option_descriptors
            .iter()
            .find(|d| d.id == id)?
            .options
            .iter()
            .find(|o| o.is_default)
            .map(|o| o.id.clone())
    };
    let mut out = CompiledSelection { api_model: model.map(|m| m.slug.clone()).unwrap_or_else(|| sel.model.clone()), ..Default::default() };

    // Context window: picks a model suffix (`[1m]`) and the meter's max.
    let window = sel.option_str("contextWindow").map(str::to_string).or_else(|| default_of("contextWindow"));
    if let (Some(a), Some(w)) = (adapter, &window) {
        if let Some(suffix) = a.model_suffixes.get("contextWindow").and_then(|m| m.get(w)) {
            out.api_model.push_str(suffix);
        }
        out.context_window = a.context_window_tokens.get(w).copied();
    }
    if out.context_window.is_none() {
        out.context_window = adapter.and_then(|a| a.fixed_context_window);
    }

    let effort = sel.option_str("effort").map(str::to_string);
    if let Some(e) = effort {
        if e == "ultrathink" {
            out.prompt_prefix = Some("Ultrathink:\n");
        }
        if e == "ultracode" {
            out.settings.insert("ultracode".into(), json!(true));
        }
        let mapped = match adapter.and_then(|a| a.effort_map.get(&e)) {
            Some(m) => m.clone(),
            None => Some(e),
        };
        out.effort = mapped;
    }
    if let Some(OptionValue::Bool(b)) = sel.option("fastMode") {
        out.settings.insert("fastMode".into(), json!(b));
    }
    if let Some(OptionValue::Bool(b)) = sel.option("thinking") {
        out.settings.insert("alwaysThinkingEnabled".into(), json!(b));
    }
    out
}

/// Context window for the usage meter when the CLI doesn't report one.
pub fn context_window(slug: &str) -> Option<u64> {
    compile(&ModelSelection { instance_id: "claudeAgent".into(), model: slug.into(), options: vec![] }).context_window
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(model: &str, opts: &[(&str, OptionValue)]) -> ModelSelection {
        ModelSelection {
            instance_id: "claudeAgent".into(),
            model: model.into(),
            options: opts.iter().map(|(id, v)| OptionSelection { id: id.to_string(), value: v.clone() }).collect(),
        }
    }

    #[test]
    fn picker_lists_match_composer_spec() {
        let all = models(Some("2.1.332"));
        let names: Vec<&str> = all.iter().filter(|m| !m.is_legacy).map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Claude Opus 5.5", "Claude Sonnet 5.5", "Claude Fable 5.2", "Claude Haiku 5.5"]);
        let opus = all.iter().find(|m| m.slug == "claude-opus-5-5").unwrap();
        assert!(opus.is_default);
        assert_eq!(opus.short_name.as_deref(), Some("Opus 5.5"));
        let effort = &opus.options[0];
        assert_eq!(effort.id, "effort");
        let labels: Vec<&str> = effort.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Low", "Medium", "High", "Extra High", "Max", "Ultracode", "Ultrathink"]);
        assert!(effort.choices.iter().find(|c| c.value == "medium").unwrap().is_default);
        assert!(effort.choices.iter().find(|c| c.value == "ultrathink").unwrap().prompt_injected);
        assert!(opus.options[1].boolean && opus.options[1].id == "fastMode");
        let haiku45 = all.iter().find(|m| m.slug == "claude-haiku-4-5").unwrap();
        assert_eq!(haiku45.options.len(), 1);
        assert_eq!(haiku45.options[0].id, "thinking");
        // Fable 5.2 needs 2.1.321.
        assert!(!models(Some("2.1.300")).iter().any(|m| m.slug == "claude-fable-5-2"));
    }

    #[test]
    fn selections_compile_to_flags() {
        let c = compile(&sel("claude-opus-5-5", &[("effort", OptionValue::Str("ultracode".into())), ("fastMode", OptionValue::Bool(true))]));
        assert_eq!(c.api_model, "claude-opus-5-5");
        assert_eq!(c.effort.as_deref(), Some("xhigh"));
        assert_eq!(c.settings.get("ultracode"), Some(&json!(true)));
        assert_eq!(c.settings.get("fastMode"), Some(&json!(true)));
        assert_eq!(c.context_window, Some(1_000_000));

        let c = compile(&sel("opus", &[("effort", OptionValue::Str("ultrathink".into()))]));
        assert_eq!((c.api_model.as_str(), c.effort.as_deref(), c.prompt_prefix), ("claude-opus-5-5", None, Some("Ultrathink:\n")));

        let c = compile(&sel("claude-opus-4-6", &[]));
        assert_eq!(c.api_model, "claude-opus-4-6[1m]");
        let c = compile(&sel("claude-opus-4-6", &[("contextWindow", OptionValue::Str("200k".into()))]));
        assert_eq!((c.api_model.as_str(), c.context_window), ("claude-opus-4-6", Some(200_000)));

        let c = compile(&sel("claude-opus-4-7", &[("effort", OptionValue::Str("xhigh".into()))]));
        assert_eq!(c.effort.as_deref(), Some("max"));

        let c = compile(&sel("some-custom-model", &[("effort", OptionValue::Str("high".into()))]));
        assert_eq!((c.api_model.as_str(), c.effort.as_deref()), ("some-custom-model", Some("high")));
    }
}
