use crate::context::ExecutionContext;
use crate::functions::Clients;
use crate::types::{Automation, PipelineEntry, StringFilter, WhenCore, WhenGroup};
use anyhow::Context as _;
use glob::glob;
use serde_json::Value;
use tracing::warn;

pub fn load_automations(dir: &str) -> anyhow::Result<Vec<Automation>> {
    anyhow::ensure!(
        std::path::Path::new(dir).is_dir(),
        "automations directory not found: {dir}"
    );
    let pattern = format!("{dir}/*.yaml");
    let mut automations = Vec::new();
    for entry in glob(&pattern).context("invalid glob pattern")? {
        let path = entry?;
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let auto: Automation = serde_yaml::from_str(&src)
            .with_context(|| format!("parsing {}", path.display()))?;
        automations.push(auto);
    }
    Ok(automations)
}

/// Returns true if the pipeline entry matches this event.
pub fn matches_when(entry: &PipelineEntry, event_type: &str, repo: &str, payload: &Value) -> bool {
    if !entry.given.repos.iter().any(|r| r == repo) {
        return false;
    }
    entry.when.iter().any(|group| matches_group(group, event_type, payload))
}

fn matches_group(group: &WhenGroup, event_type: &str, payload: &Value) -> bool {
    if !matches_core(&group.core, event_type, payload) {
        return false;
    }
    if let Some(excl) = &group.exclude {
        if matches_core(excl, event_type, payload) {
            return false;
        }
    }
    true
}

fn matches_core(core: &WhenCore, event_type: &str, payload: &Value) -> bool {
    if !core.event.matches(event_type) {
        return false;
    }

    let payload_action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    if !core.action.matches(payload_action) {
        return false;
    }

    let payload_actor = payload
        .pointer("/sender/login")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some(required) = &core.actor {
        if payload_actor != required.as_str() {
            return false;
        }
    }

    if let Some(required_merged) = core.merged {
        let is_merged = payload
            .pointer("/pull_request/merged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_merged != required_merged {
            return false;
        }
    }

    if let Some(label_filter) = &core.label {
        let present: Vec<&str> = payload
            .pointer("/pull_request/labels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l.get("name").and_then(|n| n.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        if !label_filter.values().iter().all(|req| present.contains(req)) {
            return false;
        }
    }

    true
}

/// Evaluate a step `if:` condition against the payload.
pub fn eval_if(cond: &str, payload: &Value) -> bool {
    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    match cond {
        "action_is_opened" => action == "opened",
        "action_is_closed" => action == "closed",
        "action_is_reopened" => action == "reopened",
        "action_not_opened" => action != "opened",
        _ => {
            warn!(cond, "unknown if condition, skipping step");
            false
        }
    }
}

pub async fn run_automation(
    entry: &PipelineEntry,
    payload: &Value,
    clients: &Clients,
) -> anyhow::Result<()> {
    let mut ctx = ExecutionContext::new(payload.clone());
    for raw_step in &entry.then {
        let step = crate::types::Step::from_yaml(raw_step)?;
        crate::functions::execute_step(&step, &mut ctx, clients).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PipelineEntry, StringFilter};
    use serde_json::json;

    fn make_entry(event: &str, action: &str, repo: &str) -> PipelineEntry {
        serde_yaml::from_str(&format!(
            "given:\n  trigger: github\n  repos:\n    - {repo}\nwhen:\n  - event: {event}\n    action: {action}\nthen: []\n"
        )).unwrap()
    }

    #[test]
    fn matches_correct_event_and_repo() {
        let e = make_entry("pull_request", "opened", "mongodb/atlas-cli");
        let payload = json!({"action": "opened", "sender": {"login": "alice"}});
        assert!(matches_when(&e, "pull_request", "mongodb/atlas-cli", &payload));
    }

    #[test]
    fn rejects_wrong_repo() {
        let e = make_entry("pull_request", "opened", "mongodb/atlas-cli");
        let payload = json!({"action": "opened", "sender": {"login": "alice"}});
        assert!(!matches_when(&e, "pull_request", "mongodb/other-repo", &payload));
    }

    #[test]
    fn rejects_wrong_action() {
        let e = make_entry("pull_request", "opened", "mongodb/atlas-cli");
        let payload = json!({"action": "closed", "sender": {"login": "alice"}});
        assert!(!matches_when(&e, "pull_request", "mongodb/atlas-cli", &payload));
    }

    #[test]
    fn actor_not_excludes_dependabot() {
        let e: PipelineEntry = serde_yaml::from_str(
            "given:\n  trigger: github\n  repos:\n    - mongodb/atlas-cli\nwhen:\n  - event: pull_request\n    action: opened\n    exclude:\n      actor: dependabot[bot]\nthen: []\n",
        ).unwrap();
        let bot = json!({"action": "opened", "sender": {"login": "dependabot[bot]"}});
        let human = json!({"action": "opened", "sender": {"login": "alice"}});
        assert!(!matches_when(&e, "pull_request", "mongodb/atlas-cli", &bot));
        assert!(matches_when(&e, "pull_request", "mongodb/atlas-cli", &human));
    }

    #[test]
    fn label_filter() {
        let e: PipelineEntry = serde_yaml::from_str(
            "given:\n  trigger: github\n  repos:\n    - mongodb/atlas-cli\nwhen:\n  - event: pull_request\n    action: closed\n    merged: true\n    label: auto_close_jira\nthen: []\n"
        ).unwrap();
        let with_label = json!({
            "action": "closed",
            "sender": {"login": "alice"},
            "pull_request": {
                "merged": true,
                "labels": [{"name": "auto_close_jira"}]
            }
        });
        let without_label = json!({
            "action": "closed",
            "sender": {"login": "alice"},
            "pull_request": {"merged": true, "labels": []}
        });
        assert!(matches_when(&e, "pull_request", "mongodb/atlas-cli", &with_label));
        assert!(!matches_when(&e, "pull_request", "mongodb/atlas-cli", &without_label));
    }

    #[test]
    fn eval_if_conditions() {
        let opened = json!({"action": "opened"});
        let closed = json!({"action": "closed"});
        assert!(eval_if("action_is_opened", &opened));
        assert!(!eval_if("action_is_opened", &closed));
        assert!(eval_if("action_not_opened", &closed));
        assert!(eval_if("action_is_closed", &closed));
    }

    #[test]
    fn load_automations_from_dir() {
        let autos = load_automations("automations/").unwrap();
        assert_eq!(autos.len(), 4);
    }

    #[test]
    fn load_automations_nonexistent_dir_returns_error() {
        assert!(load_automations("automations_nonexistent/").is_err());
    }

    #[test]
    fn string_filter_many_matches_any_listed() {
        let f = StringFilter::Many(vec!["opened".into(), "closed".into(), "reopened".into()]);
        assert!(f.matches("opened"));
        assert!(f.matches("reopened"));
        assert!(!f.matches("labeled"));
    }

    #[test]
    fn string_filter_any_matches_everything() {
        assert!(StringFilter::Any.matches("anything"));
        assert!(StringFilter::Any.matches(""));
    }
}
