use std::fmt::Debug;
use std::fs::File;
use anyhow::{anyhow, bail};
use tokio::process::Command;
use fang::{AsyncRunnable, FangError};
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::async_trait;
use lazy_static::lazy_static;
use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, QuerySelect};
use sea_orm::ActiveValue::Set;
use serde_json::Value;
use tempfile::TempDir;
use crate::entities::instagram_video;
use crate::entities::instagram_video::Model;
use crate::jobs::{JOB_CONTEXT, JobContext};
use crate::jobs::extract_transcript::ExtractTranscript;
use sea_orm::ColumnTrait;

lazy_static! {
    static ref REEL_REGEX: regex::Regex = regex::Regex::new(r"https://www.instagram.com/reel/([a-zA-Z0-9_-]+)(?:/.+)?").expect("Failed to compile regex");
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

    pub async fn exec(&self, context: &JobContext) -> anyhow::Result<Option<Model>> {
        tracing::info!("Fetching reel");
        let temp_dir = TempDir::new()?;

        let existing = crate::entities::instagram_video::Entity::find()
            .filter(instagram_video::Column::InstagramId.eq(&self.reel_id))
            .select_only()
            .column(instagram_video::Column::Id)
            .into_tuple::<(i32,)>()
            .one(&context.db)
            .await?;

        if existing.is_some() {
            tracing::info!("Video already in the system... skipping");
            return Ok(None);
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

        tracing::info!("Adding to instagram_video");

        let video = instagram_video::ActiveModel {
            instagram_id: Set(self.reel_id.clone()),
            video_url: Set(self.reel_url.clone()),
            info: Set(info),

            ..Default::default()
        }.insert(&context.db).await?;

        tracing::info!("Added as video id: {}", video.id);

        Ok(Some(video))
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for FetchReelJob {
    #[tracing::instrument(skip(queue))]
    async fn run(&self, queue: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        let context = JOB_CONTEXT.get()
            .ok_or(FangError { description: "Failed to read context".to_string() })?;

        let new_video = self.exec(context).await
            .map_err(|e| {
                tracing::error!("{e:?}");
                FangError { description: e.to_string() }
            })?;

        if let Some(video) = new_video {
            tracing::info!("Running transcript extraction job");
            queue.insert_task(&ExtractTranscript {
                video_id: video.id,
            }).await.unwrap();
        }

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
