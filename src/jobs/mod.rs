use std::ffi::OsString;
use std::path::PathBuf;
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use once_cell::sync::OnceCell;
use sqlx::{Pool, Postgres};

pub mod fetch_reel;
pub mod llm_extract_details;
pub mod extract_transcript;

#[derive(Debug, Clone)]
pub struct JobContext {
    pub db: sqlx::PgPool,
    pub yt_dlp_command_string: OsString,
    pub reel_dir: PathBuf,
    pub openai_client: Client<OpenAIConfig>,
    pub model: String,
}

impl JobContext {
    pub fn new(p0: Pool<Postgres>, p1: &Option<PathBuf>, p2: PathBuf, p3: Client<OpenAIConfig>, model: String) -> JobContext {
        JobContext {
            db: p0,
            yt_dlp_command_string: p1.as_ref()
                .map_or_else(|| "yt-dlp".into(), |p| p.into()),
            reel_dir: p2,
            openai_client: p3,
            model
        }
    }

    pub fn video_path(&self, video_id: &str) -> PathBuf {
        self.reel_dir.join(format!("{}.mp4", video_id))
    }
}

pub(crate) static JOB_CONTEXT: OnceCell<JobContext> = OnceCell::new();
