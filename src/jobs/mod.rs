use async_openai::config::OpenAIConfig;
use async_openai::Client;
use once_cell::sync::OnceCell;
use sea_orm::DatabaseConnection;
use sqlx::PgPool;
use std::ffi::OsString;
use std::path::PathBuf;

pub mod extract_transcript;
pub mod fetch_reel;
pub mod llm_extract_details;

#[derive(Debug, Clone)]
pub struct JobContext {
    pub db: DatabaseConnection,
    pub raw_db: PgPool,
    pub yt_dlp_command_string: OsString,
    pub reel_dir: PathBuf,
    pub openai_client: Client<OpenAIConfig>,
    pub model: String,

    pub whisper_url: String,
    pub whisper_key: String,
}

impl JobContext {
    pub fn new(
        p0: DatabaseConnection,
        raw_db: PgPool,
        p1: &Option<PathBuf>,
        p2: PathBuf,
        p3: Client<OpenAIConfig>,
        p4: String,
        p5: String,
        model: String,
    ) -> JobContext {
        JobContext {
            db: p0,
            raw_db,
            yt_dlp_command_string: p1.as_ref().map_or_else(|| "yt-dlp".into(), |p| p.into()),
            reel_dir: p2,
            openai_client: p3,

            whisper_url: p4,
            whisper_key: p5,
            model,
        }
    }

    pub fn video_path(&self, reel_id: &str) -> PathBuf {
        self.reel_dir.join(&reel_id).with_extension("mp4")
    }
}

pub(crate) static JOB_CONTEXT: OnceCell<JobContext> = OnceCell::new();
