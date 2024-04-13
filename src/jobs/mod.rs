use once_cell::sync::OnceCell;
use sea_orm::DatabaseConnection;
use sqlx::PgPool;
use std::ffi::OsString;
use std::path::PathBuf;
use crate::cli::Cli;
use crate::jobs::llm_extract_details::LlmMethod;

pub mod extract_transcript;
pub mod fetch_reel;
pub mod llm_extract_details;

#[derive(Debug, Clone)]
pub struct JobContext {
    pub db: DatabaseConnection,
    pub raw_db: PgPool,
    pub yt_dlp_command_string: OsString,
    pub reel_dir: PathBuf,

    pub whisper_url: String,
    pub whisper_key: String,

    pub completion_url: String,
    pub completion_key: String,
    pub completion_model: String,
    pub completion_mode: LlmMethod
}

impl JobContext {
    pub fn new(
        db: DatabaseConnection,
        raw_db: PgPool,
        cli: &Cli,
    ) -> JobContext {
        JobContext {
            db,
            raw_db,

            yt_dlp_command_string: cli.yt_dlp_path
                .as_ref()
                .map_or_else(|| "yt-dlp".into(), |p| p.into()),

            reel_dir: cli.reel_dir.clone(),

            whisper_url: cli.whisper_url.clone(),
            whisper_key: cli.whisper_key.clone(),

            completion_url: cli.completion_url.clone(),
            completion_key: cli.completion_key.clone(),
            completion_model: cli.completion_model.clone(),
            completion_mode: cli.completion_mode,
        }
    }

    pub fn video_path(&self, reel_id: &str) -> PathBuf {
        self.reel_dir.join(&reel_id).with_extension("mp4")
    }
}

pub(crate) static JOB_CONTEXT: OnceCell<JobContext> = OnceCell::new();
