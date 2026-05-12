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
    pub trigger: String,
    pub repos: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Exclude {
    pub event: Option<String>,
    #[serde(default)]
    pub action: ActionFilter,
    pub actor: Option<String>,
    pub merged: Option<bool>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhenGroup {
    pub event: Option<String>,
    #[serde(default)]
    pub action: ActionFilter,
    pub actor: Option<String>,
    pub merged: Option<bool>,
    pub labels: Option<Vec<String>>,
    pub exclude: Option<Exclude>,
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

#[derive(Debug, Clone)]
pub struct Step {
    pub func: String,
    pub id: Option<String>,
    pub if_cond: Option<String>,
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
        let mut if_cond = None;

        if let Some(m) = inner.as_mapping() {
            for (k, v) in m {
                let key = k.as_str().unwrap_or_default();
                match key {
                    "id" => id = v.as_str().map(|s| s.to_string()),
                    "if" => if_cond = v.as_str().map(|s| s.to_string()),
                    _ => { inputs.insert(key.to_string(), v.clone()); }
                }
            }
        }

        Ok(Step { func, id, if_cond, inputs })
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
        assert_eq!(a.pipeline.len(), 1);
        let e = &a.pipeline[0];
        assert_eq!(e.given.trigger, "github");
        assert_eq!(e.given.repos.len(), 1);
        assert_eq!(e.when.len(), 1);
        assert_eq!(e.when[0].event.as_deref(), Some("pull_request"));
        assert_eq!(e.then.len(), 3);
    }

    #[test]
    fn parse_jira_lifecycle_close() {
        let a = load("automations/jira-lifecycle-close.yaml");
        assert_eq!(a.name, "jira-lifecycle-close");
        let e = &a.pipeline[0];
        assert!(e.when[0].merged == Some(true));
        assert_eq!(
            e.when[0].labels.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
            Some(vec!["auto_close_jira"])
        );
    }

    #[test]
    fn parse_issue_sync() {
        let a = load("automations/issue-sync-atlascli.yaml");
        assert_eq!(a.pipeline.len(), 3);
        assert!(matches!(a.pipeline[0].when[0].action, ActionFilter::One(_)));
        assert!(matches!(a.pipeline[1].when[0].action, ActionFilter::One(_)));
        assert!(matches!(a.pipeline[2].when[0].action, ActionFilter::One(_)));
    }

    #[test]
    fn parse_dependabot_merge() {
        let a = load("automations/dependabot-merge.yaml");
        assert_eq!(a.pipeline[0].when[0].actor.as_deref(), Some("dependabot[bot]"));
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
