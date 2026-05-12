use anyhow::Context as _;

fn required(name: &'static str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("required env var {name} is not set"))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub github_app_id: u64,
    pub github_app_private_key: String,
    pub github_webhook_secret: String,
    pub sensor_token: String,
    pub jira_base_url: String,
    pub jira_api_token: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()
                .context("PORT must be a valid port number")?,
            github_app_id: required("GITHUB_APP_ID")?
                .parse()
                .context("GITHUB_APP_ID must be a valid u64")?,
            github_app_private_key: required("GITHUB_APP_PRIVATE_KEY")?.replace("\\n", "\n"),
            github_webhook_secret: required("GITHUB_WEBHOOK_SECRET")?,
            sensor_token: required("SENSOR_TOKEN")?,
            jira_base_url: required("JIRA_BASE_URL")?,
            jira_api_token: required("JIRA_API_TOKEN")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env var mutations are process-global; serialize all tests to prevent races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_all_vars() {
        std::env::set_var("GITHUB_APP_ID", "12345");
        std::env::set_var("GITHUB_APP_PRIVATE_KEY", "pem");
        std::env::set_var("GITHUB_WEBHOOK_SECRET", "webhook-secret-value");
        std::env::set_var("SENSOR_TOKEN", "sensor-token-value");
        std::env::set_var("JIRA_BASE_URL", "https://jira.mongodb.org");
        std::env::set_var("JIRA_API_TOKEN", "token");
    }

    #[test]
    fn all_vars_present_succeeds() {
        let _g = ENV_LOCK.lock().unwrap();
        set_all_vars();
        assert!(Config::from_env().is_ok());
    }

    #[test]
    fn missing_github_app_id_names_the_var() {
        let _g = ENV_LOCK.lock().unwrap();
        set_all_vars();
        std::env::remove_var("GITHUB_APP_ID");
        let err = Config::from_env().unwrap_err();
        assert!(
            err.to_string().contains("GITHUB_APP_ID"),
            "error should name the missing var: {err}"
        );
    }

    #[test]
    fn missing_github_webhook_secret_names_the_var() {
        let _g = ENV_LOCK.lock().unwrap();
        set_all_vars();
        std::env::remove_var("GITHUB_WEBHOOK_SECRET");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("GITHUB_WEBHOOK_SECRET"), "{err}");
    }

    #[test]
    fn missing_sensor_token_names_the_var() {
        let _g = ENV_LOCK.lock().unwrap();
        set_all_vars();
        std::env::remove_var("SENSOR_TOKEN");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("SENSOR_TOKEN"), "{err}");
    }

    #[test]
    fn missing_jira_base_url_names_the_var() {
        let _g = ENV_LOCK.lock().unwrap();
        set_all_vars();
        std::env::remove_var("JIRA_BASE_URL");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("JIRA_BASE_URL"), "{err}");
    }

    #[test]
    fn invalid_github_app_id_format_is_reported() {
        let _g = ENV_LOCK.lock().unwrap();
        set_all_vars();
        std::env::set_var("GITHUB_APP_ID", "not-a-number");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("GITHUB_APP_ID"), "{err}");
    }

    #[test]
    fn port_defaults_to_8080_when_unset() {
        let _g = ENV_LOCK.lock().unwrap();
        set_all_vars();
        std::env::remove_var("PORT");
        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.port, 8080);
    }
}
