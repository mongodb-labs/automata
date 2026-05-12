use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Automation {
    pub name: String,
    pub pipeline: Vec<PipelineEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineEntry {
    pub given: Given,
    pub when: Vec<WhenGroup>,
    pub then: Vec<serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Given {
    #[allow(dead_code)]
    pub trigger: String,
    pub repos: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WhenMatcher {
    #[serde(default)]
    pub event: StringFilter,
    #[serde(default)]
    pub action: StringFilter,
    pub actor: Option<String>,
    pub merged: Option<bool>,
    pub label: Option<StringFilter>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhenGroup {
    #[serde(flatten)]
    pub core: WhenMatcher,
    #[serde(default)]
    pub exclude: Vec<WhenMatcher>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(untagged)]
pub enum StringFilter {
    One(String),
    Many(Vec<String>),
    #[default]
    Any,
}

impl StringFilter {
    /// OR semantics: matches if value equals any of the specified strings.
    pub fn matches(&self, value: &str) -> bool {
        match self {
            StringFilter::One(s) => s == value,
            StringFilter::Many(v) => v.iter().any(|s| s == value),
            StringFilter::Any => true,
        }
    }

    /// Returns the list of required values (for AND label checks).
    pub fn values(&self) -> Vec<&str> {
        match self {
            StringFilter::One(s) => vec![s.as_str()],
            StringFilter::Many(v) => v.iter().map(|s| s.as_str()).collect(),
            StringFilter::Any => vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    pub func: String,
    pub id: Option<String>,
    pub inputs: HashMap<String, serde_yaml::Value>,
}

impl Step {
    pub fn from_yaml(val: &serde_yaml::Value) -> anyhow::Result<Self> {
        let map = val
            .as_mapping()
            .ok_or_else(|| anyhow::anyhow!("step must be a mapping"))?;

        let (func_name, inner) = map
            .iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("step mapping is empty"))?;

        let func = func_name
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("function name must be a string"))?
            .to_string();

        let mut inputs = HashMap::new();
        let mut id = None;

        if let Some(m) = inner.as_mapping() {
            for (k, v) in m {
                let key = k.as_str().unwrap_or_default();
                match key {
                    "id" => id = v.as_str().map(|s| s.to_string()),
                    _ => {
                        inputs.insert(key.to_string(), v.clone());
                    }
                }
            }
        }

        Ok(Step { func, id, inputs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(path: &str) -> Automation {
        let src = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("cannot read {path}"));
        serde_yaml::from_str(&src).unwrap_or_else(|e| panic!("parse error in {path}: {e}"))
    }

    #[test]
    fn parse_jira_lifecycle_atlascli() {
        let a = load("automations/jira-lifecycle-atlascli.yaml");
        assert_eq!(a.name, "jira-lifecycle-atlascli");
        assert_eq!(a.pipeline.len(), 4);
        // Entry 0: dependabot actor trigger
        let e0 = &a.pipeline[0];
        assert_eq!(e0.given.trigger, "github");
        assert!(matches!(&e0.when[0].core.event, StringFilter::One(ev) if ev == "pull_request"));
        assert_eq!(e0.when[0].core.actor.as_deref(), Some("dependabot[bot]"));
        assert_eq!(e0.then.len(), 3);
        // Entry 1: create_jira label trigger
        let e1 = &a.pipeline[1];
        assert!(matches!(&e1.when[0].core.label, Some(StringFilter::One(l)) if l == "create_jira"));
        assert_eq!(e1.then.len(), 6);
        // Entry 2: auto_close_jira merged close trigger
        let e2 = &a.pipeline[2];
        assert!(
            matches!(&e2.when[0].core.label, Some(StringFilter::One(l)) if l == "auto_close_jira")
        );
        assert_eq!(e2.when[0].core.merged, Some(true));
        assert_eq!(e2.then.len(), 3);
        // Entry 3: auto_close_jira closed-without-merge trigger
        let e3 = &a.pipeline[3];
        assert!(
            matches!(&e3.when[0].core.label, Some(StringFilter::One(l)) if l == "auto_close_jira")
        );
        assert_eq!(e3.when[0].core.merged, Some(false));
        assert_eq!(e3.then.len(), 3);
    }

    #[test]
    fn parse_jira_lifecycle_close() {
        let a = load("automations/jira-lifecycle-close.yaml");
        assert_eq!(a.name, "jira-lifecycle-close");
        let w = &a.pipeline[0].when[0].core;
        assert!(w.merged == Some(true));
        assert!(matches!(&w.label, Some(StringFilter::One(l)) if l == "auto_close_jira"));
    }

    #[test]
    fn parse_issue_sync() {
        let a = load("automations/issue-sync-atlascli.yaml");
        assert_eq!(a.pipeline.len(), 3);
        assert!(matches!(
            a.pipeline[0].when[0].core.action,
            StringFilter::One(_)
        ));
        assert!(matches!(
            a.pipeline[1].when[0].core.action,
            StringFilter::One(_)
        ));
        assert!(matches!(
            a.pipeline[2].when[0].core.action,
            StringFilter::One(_)
        ));
    }

    #[test]
    fn parse_dependabot_approve() {
        let a = load("automations/dependabot-approve.yaml");
        assert_eq!(a.name, "dependabot-approve");
        assert_eq!(
            a.pipeline[0].when[0].core.actor.as_deref(),
            Some("dependabot[bot]")
        );
        assert_eq!(a.pipeline[0].then.len(), 1);
    }

    #[test]
    fn parse_dependabot_automerge() {
        let a = load("automations/dependabot-automerge.yaml");
        assert_eq!(a.name, "dependabot-automerge");
        assert_eq!(
            a.pipeline[0].when[0].core.actor.as_deref(),
            Some("dependabot[bot]")
        );
        assert_eq!(a.pipeline[0].then.len(), 1);
    }

    #[test]
    fn step_from_yaml_builtin() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            "jira.create_issue:\n  id: ticket\n  issue_type: Story\n  project: CLOUDP\n  summary: test"
        ).unwrap();
        let step = Step::from_yaml(&yaml).unwrap();
        assert_eq!(step.func, "jira.create_issue");
        assert_eq!(step.id.as_deref(), Some("ticket"));
        assert_eq!(step.inputs["project"].as_str(), Some("CLOUDP"));
        assert_eq!(step.inputs["issue_type"].as_str(), Some("Story"));
    }
}
