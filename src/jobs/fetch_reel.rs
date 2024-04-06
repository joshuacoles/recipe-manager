use std::fs::File;
use tokio::process::Command;
use fang::{AsyncRunnable, FangError};
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::async_trait;
use lazy_static::lazy_static;
use serde_json::Value;
use tempfile::TempDir;
use crate::jobs::JOB_CONTEXT;
use crate::jobs::llm_extract_details::LLmExtractDetailsJob;

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct FetchReelJob {
    pub(crate) reel_url: String,
}

impl FetchReelJob {
    pub fn new(reel_url: String) -> Self {
        Self {
            reel_url,
        }
    }
}

lazy_static! {
    static ref REEL_REGEX: regex::Regex = regex::Regex::new(r"https://www.instagram.com/reel/([a-zA-Z0-9_-]+)/.+").unwrap();
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for FetchReelJob {
    #[tracing::instrument(skip(queue))]
    async fn run(&self, queue: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!("Fetching reel");
        let context = JOB_CONTEXT.get().unwrap();
        let captures = REEL_REGEX.captures(&self.reel_url).unwrap();
        let instagram_id = captures.get(1).unwrap().as_str();

        let temp_dir = TempDir::new()?;

        let existing = sqlx::query("select 1 from recipes where instagram_id = $1 union select 1 from unprocessed_recipes where instagram_id = $1")
            .bind(instagram_id)
            .fetch_optional(&context.db)
            .await
            .unwrap();

        if existing.is_some() {
            tracing::info!("Unprocessed recipe already exists skipping...");
            return Ok(());
        }

        tracing::info!("Downloading reel");
        let yt_dlp_output = Command::new(&context.yt_dlp_command_string)
            .current_dir(&temp_dir.path())
            .args(&["--write-info-json", "-o", "reel.%(ext)s", &self.reel_url])
            .output()
            .await?;

        tracing::info!("Reel downloaded, status_code = {:?}", yt_dlp_output.status);

        if !yt_dlp_output.status.success() {
            let description = format!("yt-dlp failed {}: {}", yt_dlp_output.status, String::from_utf8_lossy(&yt_dlp_output.stderr));
            return Err(FangError { description });
        }

        let info: Value = serde_json::from_reader(File::open(&temp_dir.path().join("reel.info.json")).unwrap()).unwrap();
        let video_path = temp_dir.path().join("reel.mp4");

        std::fs::rename(
            video_path,
            &context.reel_dir.join(&info["id"].as_str().unwrap()).with_extension("mp4"),
        )?;

        tracing::info!("Adding to unprocessed_recipes");
        sqlx::query("insert into unprocessed_recipes (instagram_id, instagram_url, info_json) values ($1, $2, $3)")
            .bind(instagram_id)
            .bind(&self.reel_url)
            .bind(&info)
            .execute(&context.db)
            .await
            .unwrap();

        tracing::info!("Triggering next job");
        queue.insert_task(&LLmExtractDetailsJob {
            instagram_id: instagram_id.to_string(),
        }).await?;

        Ok(())
    }

    fn uniq(&self) -> bool {
        true
    }

    fn max_retries(&self) -> i32 {
        3
    }

    fn backoff(&self, attempt: u32) -> u32 {
        60 * u32::pow(2, attempt)
    }
}