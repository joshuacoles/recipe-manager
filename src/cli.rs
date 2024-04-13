use std::net::SocketAddr;
use std::path::PathBuf;
use crate::jobs::llm_extract_details::LlmMethod;

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

    #[clap(
        long = "completion-url",
        env = "RECIPE_COMPLETION_URL",
        default_value = "http://localhost:11434/api/generate"
    )]
    pub completion_url: String,

    #[clap(
        long = "completion-key",
        env = "RECIPE_COMPLETION_KEY",
        default_value = "ollama"
    )]
    pub completion_key: String,

    #[clap(
        long = "model",
        env = "RECIPE_COMPLETION_MODEL",
        default_value = "gemma"
    )]
    pub completion_model: String,

    #[clap(
        long = "completion-mode",
        env = "RECIPE_COMPLETION_MODE",
        default_value = "ollama-json"
    )]
    pub completion_mode: LlmMethod,
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
}
