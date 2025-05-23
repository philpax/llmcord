use crate::config::Configuration;

pub struct Ai {
    pub client: async_openai::Client<async_openai::config::OpenAIConfig>,
    pub models: Vec<String>,
}
impl Ai {
    pub async fn load(config: &Configuration) -> anyhow::Result<Self> {
        let client = async_openai::Client::with_config({
            let auth = &config.authentication;
            let mut config = async_openai::config::OpenAIConfig::default();
            if let Some(server) = auth.openai_api_server.as_deref() {
                config = config.with_api_base(server);
            }
            if let Some(key) = auth.openai_api_key.as_deref() {
                config = config.with_api_key(key);
            }
            config
        });

        let models: Vec<_> = client
            .models()
            .list()
            .await?
            .data
            .into_iter()
            .map(|m| m.id)
            .collect();

        Ok(Self { client, models })
    }
}
