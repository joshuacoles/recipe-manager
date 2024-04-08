use std::fs::File;
use anyhow::{anyhow, bail};
use tokio::process::Command;
use fang::{AsyncRunnable, FangError};
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::async_trait;
use lazy_static::lazy_static;
use serde_json::Value;
use tempfile::TempDir;
use crate::jobs::{JOB_CONTEXT, JobContext};
use crate::jobs::llm_extract_details::LLmExtractDetailsJob;

lazy_static! {
    static ref REEL_REGEX: regex::Regex = regex::Regex::new(r"https://www.instagram.com/reel/([a-zA-Z0-9_-]+)/.+").expect("Failed to compile regex");
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct FetchReelJob {
    pub(crate) reel_url: String,
    pub(crate) reel_id: String,
}

impl FetchReelJob {
    pub fn new(reel_url: String) -> anyhow::Result<Self> {
        let captures = REEL_REGEX.captures(&reel_url).ok_or(anyhow!("Invalid URL"))?;
        let reel_id = captures.get(1).ok_or(anyhow!("Invalid URL"))?.as_str().to_string();

        Ok(Self {
            reel_url,
            reel_id,
        })
    }

    pub async fn exec(&self, context: &JobContext) -> anyhow::Result<()> {
        tracing::info!("Fetching reel");
        let temp_dir = TempDir::new()?;

        let existing = sqlx::query("select 1 from recipes where instagram_id = $1 union select 1 from unprocessed_recipes where instagram_id = $1")
            .bind(&self.reel_id)
            .fetch_optional(&context.raw_db)
            .await?;

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
            bail!(
                "yt-dlp failed {}: {}",
                yt_dlp_output.status,
                String::from_utf8_lossy(&yt_dlp_output.stderr)
            );
        }

        let info: Value = serde_json::from_reader(File::open(&temp_dir.path().join("reel.info.json"))?)?;
        let video_path = temp_dir.path().join("reel.mp4");

        std::fs::rename(
            video_path,
            &context.reel_dir.join(&self.reel_id).with_extension("mp4"),
        )?;

        tracing::info!("Adding to unprocessed_recipes");

        sqlx::query("insert into unprocessed_recipes (instagram_id, instagram_url, info_json) values ($1, $2, $3)")
            .bind(&self.reel_id)
            .bind(&self.reel_url)
            .bind(&info)
            .execute(&context.raw_db)
            .await?;

        Ok(())
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for FetchReelJob {
    #[tracing::instrument(skip(queue))]
    async fn run(&self, queue: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        let context = JOB_CONTEXT.get()
            .ok_or(FangError { description: "Failed to read context".to_string() })?;

        self.exec(context).await
            .map_err(|e| FangError { description: e.to_string() })?;

        tracing::info!("Triggering next job");

        queue.insert_task(&LLmExtractDetailsJob {
            instagram_id: self.reel_id.to_string(),
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