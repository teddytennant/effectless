//! `model/list` and `account/read` results -> picker models and auth state (CP:171-357, CP:975-1010).

use serde_json::Value;
use tern_core::*;

use crate::util::str_of;

/// Codex models from the T3 manifest (`model-manifest.json`, 2026-11-20): display names
/// for known slugs and which ones sit behind "Show more".
const MANIFEST: &[(&str, &str, bool)] = &[
    ("gpt-6.1-sol", "GPT-6.1 Sol", false),
    ("gpt-6-astra", "GPT-6 Astra", false),
    ("gpt-6-luna", "GPT-6 Luna", false),
    ("gpt-6-sol", "GPT-6 Sol", true),
    ("gpt-5.6-sol", "GPT-5.6 Sol", true),
    ("gpt-5.6-terra", "GPT-5.6 Terra", true),
    ("gpt-5.6-luna", "GPT-5.6 Luna", true),
    ("gpt-5.5", "GPT-5.5", true),
    ("gpt-5.4-mini", "GPT-5.4 Mini", true),
    ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark", true),
];

pub const DEFAULT_MODEL: &str = "gpt-6-astra";
/// Text generation default (`model.ts:161-169`).
pub const TEXT_GENERATION_MODEL: &str = "gpt-6-luna";

fn effort_label(e: &str) -> String {
    match e {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultra" => "Ultra",
        other => other,
    }
    .to_string()
}

/// `formatCodexModelName`: `gpt-6-astra` -> `GPT-6-Astra`.
pub fn format_model_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case("gpt") {
        out.push_str("GPT");
        rest = &rest[3..];
    }
    let mut upper_next = false;
    for c in rest.chars() {
        if upper_next && c.is_ascii_lowercase() {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c);
        }
        upper_next = c == '-';
    }
    out
}

/// Strips `GPT-`/`GPT ` when words other than version numbers remain (spec/04 §2).
pub fn short_name(name: &str) -> String {
    for prefix in ["GPT-", "GPT "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.split([' ', '-']).any(|w| w.chars().any(|c| c.is_alphabetic())) {
                return rest.to_string();
            }
        }
    }
    name.to_string()
}

pub fn map_model(m: &Value) -> Option<ProviderModel> {
    let slug = str_of(m, "model").or_else(|| str_of(m, "id"))?.to_string();
    let manifest = MANIFEST.iter().find(|(s, _, _)| *s == slug);
    let name = manifest
        .map(|(_, n, _)| n.to_string())
        .unwrap_or_else(|| format_model_name(str_of(m, "displayName").unwrap_or(&slug)));
    let default_effort = if slug == "gpt-6-astra" || slug.starts_with("gpt-6-astra-") {
        Some("medium".to_string())
    } else {
        str_of(m, "defaultReasoningEffort").map(str::to_string)
    };
    let mut options = Vec::new();
    let efforts: Vec<ModelOptionChoice> = m
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let id = str_of(e, "reasoningEffort")?;
                    Some(ModelOptionChoice {
                        value: id.to_string(),
                        label: effort_label(id),
                        is_default: default_effort.as_deref() == Some(id),
                        description: str_of(e, "description").filter(|d| !d.is_empty()).map(str::to_string),
                        prompt_injected: false,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if !efforts.is_empty() {
        options.push(ModelOptionDescriptor { id: "reasoningEffort".into(), label: "Reasoning".into(), choices: efforts, boolean: false });
    }
    let mut tiers: Vec<(String, String, String)> = m
        .get("serviceTiers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some((
                        str_of(t, "id")?.to_string(),
                        str_of(t, "name").unwrap_or_default().to_string(),
                        str_of(t, "description").unwrap_or_default().to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if tiers.is_empty() {
        if let Some(extra) = m.get("additionalSpeedTiers").and_then(Value::as_array) {
            tiers = extra
                .iter()
                .filter_map(Value::as_str)
                .map(|id| (id.to_string(), if id == "fast" { "Fast".into() } else { id.to_string() }, String::new()))
                .collect();
        }
    }
    if !tiers.is_empty() {
        let default_tier = str_of(m, "defaultServiceTier")
            .filter(|d| tiers.iter().any(|t| t.0 == *d))
            .unwrap_or("default")
            .to_string();
        let mut choices = vec![ModelOptionChoice {
            value: "default".into(),
            label: "Standard".into(),
            is_default: default_tier == "default",
            description: None,
            prompt_injected: false,
        }];
        for (id, name, desc) in tiers {
            let desc = if id == "ultrafast" { "Even faster, more expensive".to_string() } else { desc };
            choices.push(ModelOptionChoice {
                is_default: default_tier == id,
                value: id,
                label: name,
                description: (!desc.is_empty()).then_some(desc),
                prompt_injected: false,
            });
        }
        options.push(ModelOptionDescriptor { id: "serviceTier".into(), label: "Service Tier".into(), choices, boolean: false });
    }
    let cyber: Vec<String> = m
        .get("availableAccessPrograms")
        .and_then(|p| p.get("cyber"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    if !cyber.is_empty() {
        let mut programs = cyber.clone();
        if !programs.iter().any(|p| p == "standard") {
            programs.insert(0, "standard".into());
        }
        let default = if cyber.iter().any(|p| p == "standard") { "standard".to_string() } else { cyber[0].clone() };
        let choices = programs
            .into_iter()
            .map(|p| ModelOptionChoice {
                label: match p.as_str() {
                    "standard" => "Standard".into(),
                    "daybreakBlue" => "Daybreak Blue".into(),
                    "daybreakRed" => "Daybreak Red".into(),
                    other => other.into(),
                },
                is_default: p == default,
                value: p,
                description: None,
                prompt_injected: false,
            })
            .collect();
        options.push(ModelOptionDescriptor { id: "cyberAccessProgram".into(), label: "Cyber Access".into(), choices, boolean: false });
    }
    Some(ProviderModel {
        short_name: Some(short_name(&name)),
        name,
        description: str_of(m, "description").map(str::to_string),
        is_default: m.get("isDefault").and_then(Value::as_bool).unwrap_or(false),
        is_custom: false,
        is_legacy: manifest.is_some_and(|(_, _, legacy)| *legacy),
        options,
        slug,
    })
}

/// `PREFERRED_DEFAULT_CODEX_MODELS` (contracts/src/model.ts:131): the first one listed
/// wins over Codex's own `isDefault`.
const PREFERRED_DEFAULTS: &[&str] = &[DEFAULT_MODEL, "gpt-5.6-sol", "gpt-5.6-terra"];

/// Maps every page of `model/list`, skipping hidden models, and applies T3's preferred
/// default (`applyPreferredCodexDefaultModel`).
pub fn map_models(pages: &[Value]) -> Vec<ProviderModel> {
    let mut out: Vec<ProviderModel> = pages
        .iter()
        .flat_map(|p| p.get("data").and_then(Value::as_array).cloned().unwrap_or_default())
        .filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|m| map_model(&m))
        .collect();
    let family = |slug: &str| slug.strip_prefix("openai.").unwrap_or(slug).to_string();
    let preferred = PREFERRED_DEFAULTS
        .iter()
        .find_map(|p| out.iter().find(|m| family(&m.slug) == *p).map(|m| m.slug.clone()));
    if let Some(slug) = preferred {
        for m in &mut out {
            m.is_default = m.slug == slug;
        }
    }
    out
}

/// `codexPlanLabel` (CodexProvider.ts:212-245).
fn plan_label(plan: &str) -> Option<&'static str> {
    Some(match plan {
        "free" => "ChatGPT Free Subscription",
        "go" => "ChatGPT Go Subscription",
        "plus" => "ChatGPT Plus Subscription",
        "pro" => "ChatGPT Pro 200 Subscription",
        "prolite" => "ChatGPT Pro 100 Subscription",
        "promax" => "ChatGPT Pro 500 Subscription",
        "self_serve_business_prolite" => "ChatGPT Business Premium Subscription",
        "team" | "self_serve_business_usage_based" | "business" => "ChatGPT Business Subscription",
        "ent26" | "enterprise_cbp_automation" | "enterprise_cbp_usage_based" | "enterprise" => {
            "ChatGPT Enterprise Subscription"
        }
        "edu" | "edu_plus" | "edu_pro" => "ChatGPT Edu Subscription",
        "unknown" => "ChatGPT Subscription",
        _ => return None,
    })
}

pub struct AccountStatus {
    pub auth: ProviderAuthState,
    pub label: Option<String>,
    pub message: Option<String>,
}

/// `account/read` result -> auth state (CP:975-1010).
pub fn map_account(result: &Value) -> AccountStatus {
    let account = result.get("account").filter(|a| !a.is_null());
    match account {
        Some(a) => {
            let label = match str_of(a, "type") {
                Some("apiKey") => Some("OpenAI API Key".to_string()),
                Some("amazonBedrock") => Some("Amazon Bedrock".to_string()),
                Some("chatgpt") => {
                    let plan = str_of(a, "planType").and_then(plan_label).map(str::to_string);
                    match (plan, str_of(a, "email")) {
                        (Some(p), Some(e)) => Some(format!("{p} ({e})")),
                        (p, e) => p.or(e.map(str::to_string)),
                    }
                }
                _ => None,
            };
            AccountStatus { auth: ProviderAuthState::SignedIn, label, message: None }
        }
        None if result.get("requiresOpenaiAuth").and_then(Value::as_bool).unwrap_or(false) => AccountStatus {
            auth: ProviderAuthState::SignedOut,
            label: None,
            message: Some(
                match std::env::var("CODEX_HOME") {
                    Ok(home) if !home.is_empty() => format!(
                        "Codex CLI is not authenticated. Run `CODEX_HOME={home} codex login` and try again."
                    ),
                    _ => "Codex CLI is not authenticated. Run `codex login` and try again.".to_string(),
                },
            ),
        },
        // A custom model provider that needs no OpenAI login.
        None => AccountStatus { auth: ProviderAuthState::Unknown, label: None, message: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names() {
        assert_eq!(format_model_name("gpt-6-astra"), "GPT-6-Astra");
        assert_eq!(short_name("GPT-6 Astra"), "6 Astra");
        assert_eq!(short_name("GPT-5.5"), "GPT-5.5");
    }

    #[test]
    fn account_states() {
        assert_eq!(map_account(&json!({"account": null, "requiresOpenaiAuth": false})).auth, ProviderAuthState::Unknown);
        assert_eq!(map_account(&json!({"account": null, "requiresOpenaiAuth": true})).auth, ProviderAuthState::SignedOut);
        let s = map_account(&json!({"account": {"type": "chatgpt", "email": "a@b.c", "planType": "plus"}}));
        assert_eq!(s.auth, ProviderAuthState::SignedIn);
        assert_eq!(s.label.as_deref(), Some("ChatGPT Plus Subscription (a@b.c)"));
    }
}
