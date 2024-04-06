use std::ffi::OsString;
use std::path::PathBuf;
use once_cell::sync::OnceCell;
use sqlx::{Pool, Postgres};

pub mod fetch_reel;
pub mod llm_extract_details;

#[derive(Debug, Clone)]
pub struct JobContext {
    pub db: sqlx::PgPool,
    pub yt_dlp_command_string: OsString,
}

impl JobContext {
    pub fn new(p0: Pool<Postgres>, p1: &Option<PathBuf>) -> JobContext {
        JobContext {
            db: p0,
            yt_dlp_command_string: p1.as_ref()
                .map_or_else(|| "yt-dlp".into(), |p| p.into()),
        }
    }
}

pub(crate) static JOB_CONTEXT: OnceCell<JobContext> = OnceCell::new();
