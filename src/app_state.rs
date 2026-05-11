use std::sync::Arc;

use crate::config::Config;
use crate::types::Automation;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub automations: Arc<Vec<Automation>>,
    pub http: reqwest::Client,
}
