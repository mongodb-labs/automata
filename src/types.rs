use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Automation {
    pub name: String,
    pub description: Option<String>,
    pub given: Given,
    pub when: Vec<WhenGroup>,
    pub then: Vec<serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Given {
    pub trigger: String,
    pub repos: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhenGroup {
    pub event: Option<String>,
    #[serde(default)]
    pub action: ActionFilter,
    pub actor: Option<String>,
    pub actor_not: Option<String>,
    pub merged: Option<bool>,
    pub labels_include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(untagged)]
pub enum ActionFilter {
    One(String),
    Many(Vec<String>),
    #[default]
    Any,
}

impl ActionFilter {
    pub fn matches(&self, action: &str) -> bool {
        match self {
            ActionFilter::One(a) => a == action,
            ActionFilter::Many(v) => v.iter().any(|a| a == action),
            ActionFilter::Any => true,
        }
    }
}

/// A parsed step extracted from the raw YAML value.
#[derive(Debug, Clone)]
pub struct Step {
    /// The built-in function name (e.g. "jira.create_issue") or None if uses:.
    pub func: Option<String>,
    /// Named function to call via uses:.
    pub uses: Option<String>,
    /// Optional step ID for referencing outputs.
    pub id: Option<String>,
    /// Optional condition shorthand.
    pub if_cond: Option<String>,
    /// All remaining key-value inputs (after extracting id/if/uses).
    pub inputs: HashMap<String, serde_yaml::Value>,
}

impl Step {
    pub fn from_yaml(val: &serde_yaml::Value) -> anyhow::Result<Self> {
        let map = val
            .as_mapping()
            .ok_or_else(|| anyhow::anyhow!("step must be a mapping"))?;

        // uses: is a top-level key
        if let Some(uses_val) = map.get("uses") {
            let uses = uses_val
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("uses must be a string"))?
                .to_string();
            let mut inputs = HashMap::new();
            for (k, v) in map {
                let key = k.as_str().unwrap_or_default();
                if key != "uses" {
                    inputs.insert(key.to_string(), v.clone());
                }
            }
            return Ok(Step { func: None, uses: Some(uses), id: None, if_cond: None, inputs });
        }

        // Built-in: single key is the function name
        let (func_name, inner) = map
            .iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("step mapping is empty"))?;

        let func = func_name
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("function name must be a string"))?
            .to_string();

        let inner_map = inner.as_mapping();
        let mut inputs = HashMap::new();
        let mut id = None;
        let mut if_cond = None;

        if let Some(m) = inner_map {
            for (k, v) in m {
                let key = k.as_str().unwrap_or_default();
                match key {
                    "id" => id = v.as_str().map(|s| s.to_string()),
                    "if" => if_cond = v.as_str().map(|s| s.to_string()),
                    _ => { inputs.insert(key.to_string(), v.clone()); }
                }
            }
        }

        Ok(Step { func: Some(func), uses: None, id, if_cond, inputs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(path: &str) -> Automation {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("cannot read {path}"));
        serde_yaml::from_str(&src)
            .unwrap_or_else(|e| panic!("parse error in {path}: {e}"))
    }

    #[test]
    fn parse_jira_lifecycle_atlascli() {
        let a = load("automations/jira-lifecycle-atlascli.yaml");
        assert_eq!(a.name, "jira-lifecycle-atlascli");
        assert_eq!(a.given.trigger, "github");
        assert_eq!(a.given.repos.len(), 3);
        assert_eq!(a.when.len(), 1);
        assert_eq!(a.when[0].event.as_deref(), Some("pull_request"));
        assert_eq!(a.then.len(), 3);
    }

    #[test]
    fn parse_jira_lifecycle_close() {
        let a = load("automations/jira-lifecycle-close.yaml");
        assert_eq!(a.name, "jira-lifecycle-close");
        assert!(a.when[0].merged == Some(true));
        assert_eq!(
            a.when[0].labels_include.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
            Some(vec!["auto_close_jira"])
        );
    }

    #[test]
    fn parse_issue_sync() {
        let a = load("automations/issue-sync-atlascli.yaml");
        assert!(matches!(a.when[0].action, ActionFilter::Many(_)));
    }

    #[test]
    fn parse_dependabot_merge() {
        let a = load("automations/dependabot-merge.yaml");
        assert_eq!(a.when[0].actor.as_deref(), Some("dependabot[bot]"));
    }

    #[test]
    fn step_from_yaml_builtin() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            "jira.create_issue:\n  id: ticket\n  issue_type: Story\n  project: CLOUDP\n  summary: test"
        ).unwrap();
        let step = Step::from_yaml(&yaml).unwrap();
        assert_eq!(step.func.as_deref(), Some("jira.create_issue"));
        assert_eq!(step.id.as_deref(), Some("ticket"));
        assert_eq!(step.inputs["project"].as_str(), Some("CLOUDP"));
        assert_eq!(step.inputs["issue_type"].as_str(), Some("Story"));
    }

    #[test]
    fn step_from_yaml_uses() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            "uses: notify-slack\nchannel: C123\nmessage: hello"
        ).unwrap();
        let step = Step::from_yaml(&yaml).unwrap();
        assert_eq!(step.uses.as_deref(), Some("notify-slack"));
        assert_eq!(step.inputs["channel"].as_str(), Some("C123"));
    }
}
