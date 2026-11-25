//! Model and effort helpers for the composer (spec 04 §2-3): short names, the
//! reasoning descriptor, the trigger's effort suffix, default selections and the
//! picker's search ranking. Pure functions over `ProviderStatus` data.

use tern_core::provider::{ModelOptionDescriptor, ProviderModel, ProviderStatus};
use tern_core::{Driver, ModelSelection, OptionValue};

use crate::assets::IconName;

/// Family prefixes a short name drops (`modelShortName.ts`).
const FAMILY_PREFIXES: [&str; 6] = ["Claude ", "GPT-", "GPT ", "Gemini ", "Muse ", "Grok "];

fn is_version(word: &str) -> bool {
    let w = word.strip_prefix(['v', 'V']).unwrap_or(word);
    let w = w
        .strip_suffix(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(w);
    !w.is_empty()
        && w.split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// `Claude Opus 5.5` -> `Opus 5.5`, `GPT-6 Astra` -> `6 Astra`, `GPT-5.5` stays.
pub fn derive_short_name(name: &str) -> String {
    for prefix in FAMILY_PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            let rest = rest.trim();
            if rest.split_whitespace().any(|w| !is_version(w)) {
                return if rest.contains(char::is_whitespace) {
                    rest.to_string()
                } else {
                    rest.replace('-', " ")
                };
            }
            return name.to_string();
        }
    }
    name.to_string()
}

/// The server's short name, or one derived from the display name.
pub fn short_name(model: &ProviderModel) -> String {
    model
        .short_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| derive_short_name(&model.name))
}

/// The reasoning control: the select with id effort / reasoningEffort / variant, or
/// the boolean `thinking`.
pub fn reasoning_descriptor(model: &ProviderModel) -> Option<&ModelOptionDescriptor> {
    model
        .options
        .iter()
        .find(|o| {
            !o.boolean
                && !o.choices.is_empty()
                && matches!(o.id.as_str(), "effort" | "reasoningEffort" | "variant")
        })
        .or_else(|| {
            model
                .options
                .iter()
                .find(|o| o.boolean && o.id == "thinking")
        })
}

/// Fast-mode style boolean options shown in the picker footer's speed control.
pub fn fast_descriptor(model: &ProviderModel) -> Option<&ModelOptionDescriptor> {
    model
        .options
        .iter()
        .find(|o| o.boolean && o.id == "fastMode")
}

/// The chosen value of a select descriptor: the explicit pick, else the default.
pub fn selected_choice(desc: &ModelOptionDescriptor, sel: &ModelSelection) -> Option<String> {
    sel.option_str(&desc.id)
        .map(str::to_string)
        .or_else(|| {
            desc.choices
                .iter()
                .find(|c| c.is_default)
                .map(|c| c.value.clone())
        })
        .or_else(|| desc.choices.first().map(|c| c.value.clone()))
}

/// The value of a boolean descriptor (unset = off, except `thinking` which is on).
pub fn bool_value(desc: &ModelOptionDescriptor, sel: &ModelSelection) -> bool {
    match sel.option(&desc.id) {
        Some(OptionValue::Bool(b)) => *b,
        Some(OptionValue::Str(s)) => s == "true",
        None => desc.id == "thinking",
    }
}

/// The effort suffix on the trigger (label mode): `Low`, `Extra High`, `Thinking On`,
/// plus ` ⚡` when fast mode is on. None when the model has no reasoning control.
pub fn effort_suffix(model: &ProviderModel, sel: &ModelSelection) -> Option<String> {
    let desc = reasoning_descriptor(model)?;
    let mut label = if desc.boolean {
        format!(
            "{} {}",
            desc.label,
            if bool_value(desc, sel) { "On" } else { "Off" }
        )
    } else {
        let value = selected_choice(desc, sel)?;
        desc.choices
            .iter()
            .find(|c| c.value == value)
            .map(|c| c.label.clone())
            .unwrap_or(value)
    };
    if fast_descriptor(model).is_some_and(|d| bool_value(d, sel)) {
        label.push_str(" ⚡");
    }
    Some(label)
}

/// Finds the provider and model a selection names.
pub fn find_model<'a>(
    providers: &'a [ProviderStatus],
    sel: &ModelSelection,
) -> Option<(&'a ProviderStatus, &'a ProviderModel)> {
    let provider = providers
        .iter()
        .find(|p| p.instance_id == sel.instance_id)?;
    let model = provider.models.iter().find(|m| m.slug == sel.model)?;
    Some((provider, model))
}

/// The provider's default model as a selection with no explicit options.
pub fn default_selection(provider: &ProviderStatus) -> Option<ModelSelection> {
    let model = provider
        .models
        .iter()
        .find(|m| m.is_default)
        .or_else(|| provider.models.first())?;
    Some(ModelSelection {
        instance_id: provider.instance_id.clone(),
        model: model.slug.clone(),
        options: Vec::new(),
    })
}

/// Providers the composer can use: enabled with at least one model.
pub fn usable(providers: &[ProviderStatus]) -> impl Iterator<Item = &ProviderStatus> {
    providers
        .iter()
        .filter(|p| p.enabled && !p.models.is_empty())
}

/// A selection that names a usable model: `preferred` if it does, else the first
/// usable provider's default.
pub fn resolve_selection(
    providers: &[ProviderStatus],
    preferred: Option<&ModelSelection>,
) -> Option<ModelSelection> {
    if let Some(sel) = preferred
        && find_model(providers, sel).is_some_and(|(p, _)| p.enabled)
    {
        return Some(sel.clone());
    }
    usable(providers).find_map(default_selection)
}

/// The brand glyph for a provider instance.
pub fn provider_icon(driver: Driver) -> IconName {
    match driver {
        Driver::Codex => IconName::ProviderOpenai,
        Driver::Claude => IconName::ProviderClaude,
        Driver::Acp => IconName::Bot,
    }
}

/// Claude's mark is painted in its brand orange (`fill-[#d97757]`).
pub fn provider_icon_color(driver: Driver) -> Option<gpui::Hsla> {
    match driver {
        Driver::Claude => Some(gpui::rgb(0xd97757).into()),
        _ => None,
    }
}

/// Lower is better; None = no match. Exact < prefix < word boundary < substring <
/// fuzzy (fuzzy only for queries of 3+ chars).
fn field_score(field: &str, token: &str) -> Option<u32> {
    let field = field.to_lowercase();
    if field == token {
        return Some(0);
    }
    if field.starts_with(token) {
        return Some(1);
    }
    if let Some(i) = field.find(token) {
        let before = field[..i].chars().last();
        if before.is_some_and(|c| !c.is_alphanumeric()) {
            return Some(2);
        }
        return Some(3);
    }
    if token.chars().count() >= 3 && is_subsequence(token, &field) {
        return Some(4);
    }
    None
}

pub(crate) fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

/// Model search across providers (spec 04 §2.3). Every token must match one of
/// name, short name, provider display name or slug; results keep catalog order on
/// ties. Returns (instance id, slug).
pub fn search_models(providers: &[ProviderStatus], query: &str) -> Vec<(String, String)> {
    let query = query.trim().to_lowercase();
    let tokens: Vec<&str> = query.split_whitespace().collect();
    let mut scored: Vec<(u32, usize, String, String)> = Vec::new();
    let mut order = 0usize;
    for provider in usable(providers) {
        for model in &provider.models {
            order += 1;
            let short = short_name(model);
            let fields = [
                model.name.as_str(),
                short.as_str(),
                provider.display_name.as_str(),
                model.slug.as_str(),
            ];
            let mut total = 0u32;
            let mut all = true;
            for token in &tokens {
                let best = fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| field_score(f, token).map(|s| s * 10 + i as u32))
                    .min();
                match best {
                    Some(s) => total += s,
                    None => {
                        all = false;
                        break;
                    }
                }
            }
            if !all {
                continue;
            }
            let exact = model.name.to_lowercase() == query || short.to_lowercase() == query;
            let score = if exact { 0 } else { total + 1 };
            scored.push((
                score,
                order,
                provider.instance_id.clone(),
                model.slug.clone(),
            ));
        }
    }
    scored.sort_by_key(|(score, order, _, _)| (*score, *order));
    scored.into_iter().map(|(_, _, i, s)| (i, s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tern_core::provider::{ModelOptionChoice, ProviderAuthState};

    fn choice(value: &str, label: &str, default: bool) -> ModelOptionChoice {
        ModelOptionChoice {
            value: value.into(),
            label: label.into(),
            is_default: default,
            description: None,
            prompt_injected: false,
        }
    }

    fn model(slug: &str, name: &str, options: Vec<ModelOptionDescriptor>) -> ProviderModel {
        ProviderModel {
            slug: slug.into(),
            name: name.into(),
            short_name: None,
            description: None,
            is_default: false,
            is_custom: false,
            is_legacy: false,
            options,
        }
    }

    fn effort() -> ModelOptionDescriptor {
        ModelOptionDescriptor {
            id: "effort".into(),
            label: "Reasoning".into(),
            choices: vec![
                choice("low", "Low", false),
                choice("medium", "Medium", true),
                choice("xhigh", "Extra High", false),
            ],
            boolean: false,
        }
    }

    fn provider(id: &str, name: &str, models: Vec<ProviderModel>) -> ProviderStatus {
        ProviderStatus {
            instance_id: id.into(),
            driver: Driver::Claude,
            display_name: name.into(),
            enabled: true,
            binary_path: None,
            version: None,
            auth: ProviderAuthState::SignedIn,
            account_label: None,
            models,
            capabilities: Default::default(),
            message: None,
        }
    }

    #[test]
    fn short_names_follow_the_family_rule() {
        assert_eq!(derive_short_name("Claude Opus 5.5"), "Opus 5.5");
        assert_eq!(derive_short_name("GPT-6 Astra"), "6 Astra");
        assert_eq!(derive_short_name("GPT-5.5"), "GPT-5.5");
        assert_eq!(derive_short_name("Gemini 3.8 Flash"), "3.8 Flash");
        assert_eq!(derive_short_name("Auto"), "Auto");
    }

    #[test]
    fn effort_suffix_uses_pick_then_default() {
        let m = model("opus", "Claude Opus 5.5", vec![effort()]);
        let mut sel = ModelSelection {
            instance_id: "claudeAgent".into(),
            model: "opus".into(),
            options: vec![],
        };
        assert_eq!(effort_suffix(&m, &sel).as_deref(), Some("Medium"));
        sel.set_option("effort", OptionValue::Str("xhigh".into()));
        assert_eq!(effort_suffix(&m, &sel).as_deref(), Some("Extra High"));
        let plain = model("haiku", "Claude Haiku", vec![]);
        assert_eq!(effort_suffix(&plain, &sel), None);
    }

    #[test]
    fn thinking_bool_suffix() {
        let thinking = ModelOptionDescriptor {
            id: "thinking".into(),
            label: "Thinking".into(),
            choices: vec![],
            boolean: true,
        };
        let m = model("h", "Claude Haiku 4.5", vec![thinking]);
        let mut sel = ModelSelection {
            instance_id: "c".into(),
            model: "h".into(),
            options: vec![],
        };
        assert_eq!(effort_suffix(&m, &sel).as_deref(), Some("Thinking On"));
        sel.set_option("thinking", OptionValue::Bool(false));
        assert_eq!(effort_suffix(&m, &sel).as_deref(), Some("Thinking Off"));
    }

    #[test]
    fn search_ranks_exact_then_prefix_and_needs_every_token() {
        let providers = vec![
            provider(
                "claudeAgent",
                "Claude",
                vec![
                    model("opus", "Claude Opus 5.5", vec![]),
                    model("sonnet", "Claude Sonnet 5.5", vec![]),
                ],
            ),
            provider(
                "codex",
                "Codex",
                vec![model("astra", "GPT-6 Astra", vec![])],
            ),
        ];
        let hits = search_models(&providers, "sonnet");
        assert_eq!(hits[0].1, "sonnet");
        assert_eq!(hits.len(), 1);
        let hits = search_models(&providers, "claude 5.5");
        assert_eq!(hits.len(), 2);
        let hits = search_models(&providers, "codex astra");
        assert_eq!(hits, vec![("codex".to_string(), "astra".to_string())]);
        assert!(search_models(&providers, "zzz").is_empty());
        // Fuzzy only from 3 chars.
        assert_eq!(search_models(&providers, "sn").len(), 0);
        assert_eq!(search_models(&providers, "snt")[0].1, "sonnet");
    }

    #[test]
    fn resolve_falls_back_to_first_default() {
        let mut m = model("opus", "Claude Opus 5.5", vec![]);
        m.is_default = true;
        let providers = vec![provider("claudeAgent", "Claude", vec![m])];
        let bogus = ModelSelection {
            instance_id: "nope".into(),
            model: "x".into(),
            options: vec![],
        };
        let sel = resolve_selection(&providers, Some(&bogus)).unwrap();
        assert_eq!(sel.model, "opus");
    }
}
