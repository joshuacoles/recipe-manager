use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, clap::Parser)]
pub struct Cli {
    /// Postgres connection url
    #[clap(
        short = 'd',
        long = "db",
        env = "RECIPE_DATABASE_URL",
        default_value = "postgres://postgres@localhost/recipes"
    )]
    pub database_url: String,

    /// Server address
    #[clap(
        short = 'a',
        long = "address",
        env = "RECIPE_ADDRESS",
        default_value = "0.0.0.0:5005"
    )]
    pub address: SocketAddr,

    /// Path to youtube-dl if not on PATH
    #[clap(long = "yt-dlp-path", env = "RECIPE_YT_DLP_PATH")]
    pub yt_dlp_path: Option<PathBuf>,

    /// Directory to save reels
    #[clap(
        short = 'r',
        long = "reel-dir",
        env = "RECIPE_REEL_DIR",
        default_value = "./reels"
    )]
    pub reel_dir: PathBuf,

    /// OpenAI API key
    #[clap(
        long = "openai-api-key",
        env = "RECIPE_OPENAI_API_KEY",
        default_value = "ollama"
    )]
    pub openai_api_key: String,

    /// Direct OpenAI API key
    #[clap(long = "direct-openai-api-key", env = "RECIPE_DIRECT_OPENAI_API_KEY")]
    pub direct_openai_api_key: String,

    /// OpenAI API model
    #[clap(long = "model", env = "RECIPE_OPENAI_MODEL", default_value = "llama2")]
    pub openai_model: String,

    /// OpenAI Base url
    #[clap(
        long = "openai-base-url",
        env = "RECIPE_OPENAI_BASE_URL",
        default_value = "http://localhost:11434/v1"
    )]
    pub openai_base_url: String,


    #[clap(
        long = "whisper-url",
        env = "RECIPE_WHISPER_URL",
        default_value = "http://127.0.0.1:8080/inference"
    )]
    pub whisper_url: String,

    #[clap(
        long = "whisper-key",
        env = "RECIPE_WHISPER_KEY",
        default_value = "local"
    )]
    pub whisper_key: String,
}

impl Cli {
    pub fn validate_reel_dir(&self) -> anyhow::Result<()> {
        if !self.reel_dir.exists() {
            std::fs::create_dir(&self.reel_dir)?;
        } else if !self.reel_dir.is_dir() {
            anyhow::bail!("reel-dir must be a directory");
        }

        Ok(())
    }

    pub fn openai_client(
        &self,
    ) -> anyhow::Result<async_openai::Client<async_openai::config::OpenAIConfig>> {
        let config = async_openai::config::OpenAIConfig::new()
            .with_api_base(&self.openai_base_url)
            .with_api_key(&self.openai_api_key);

        let client = async_openai::Client::with_config(config);

        Ok(client)
    }
}
