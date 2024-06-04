use crate::entities::instagram_video;
use crate::entities::instagram_video::Model;
use crate::jobs::extract_transcript::ExtractTranscript;
use crate::jobs::{JobContext, JOB_CONTEXT};
use anyhow::{anyhow, bail};
use fang::async_trait;
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::{AsyncRunnable, FangError};
use lazy_static::lazy_static;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, QuerySelect};
use sea_orm::{ColumnTrait, FromJsonQueryResult};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::process::Command;

lazy_static! {
    static ref REEL_REGEX: regex::Regex =
        regex::Regex::new(r"https://www.instagram.com/reel/([a-zA-Z0-9_-]+)(?:/.+)?")
            .expect("Failed to compile regex");
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct FetchReelJob {
    pub(crate) reel_url: String,
    pub(crate) reel_id: String,
    pub(crate) auto_llm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromJsonQueryResult, Eq, PartialEq)]
pub struct ReelInfo {
    pub description: String,

    #[serde(flatten)]
    rest: HashMap<String, Value>,
}

impl FetchReelJob {
    pub fn new(reel_url: String, auto_llm: bool) -> anyhow::Result<Self> {
        let captures = REEL_REGEX
            .captures(&reel_url)
            .ok_or(anyhow!("Invalid URL"))?;

        let reel_id = captures
            .get(1)
            .ok_or(anyhow!("Invalid URL"))?
            .as_str()
            .to_string();

        Ok(Self { reel_url, reel_id, auto_llm })
    }

    pub async fn exec(&self, context: &JobContext) -> anyhow::Result<Option<Model>> {
        tracing::info!("Fetching reel");

        let existing = crate::entities::instagram_video::Entity::find()
            .filter(instagram_video::Column::InstagramId.eq(&self.reel_id))
            .select_only()
            .column(instagram_video::Column::Id)
            .into_tuple::<(i32, )>()
            .one(&context.db)
            .await?;

        if existing.is_some() {
            tracing::info!("Video already in the system... skipping");
            return Ok(None);
        }

        let (info, video_path) = self.download_reel(&context).await?;

        let transcript = ExtractTranscript::extract_transcript(&context, &video_path).await?;

        tracing::info!("Adding to instagram_video");

        let video = instagram_video::ActiveModel {
            instagram_id: Set(self.reel_id.clone()),
            video_url: Set(self.reel_url.clone()),
            info: Set(info),
            transcript: Set(Some(transcript)),

            ..Default::default()
        }
            .insert(&context.db)
            .await?;

        tracing::info!("Added as video id: {}", video.id);

        Ok(Some(video))
    }

    async fn download_reel(&self, context: &&JobContext) -> anyhow::Result<(ReelInfo, PathBuf)> {
        tracing::info!("Downloading reel");

        let video_path = context.video_path(&self.reel_id);
        let json_path = video_path.with_extension("json");

        if video_path.exists() && json_path.exists() {
            tracing::info!("Reel already downloaded");
            let info: ReelInfo = serde_json::from_reader(File::open(&json_path)?)?;
            return Ok((info, video_path));
        }

        let temp_dir = TempDir::new()?;

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

        let temp_json = &temp_dir.path().join("reel.info.json");
        let temp_video_path = temp_dir.path().join("reel.mp4");

        std::fs::rename(temp_video_path, &video_path)?;
        std::fs::rename(temp_json, &json_path)?;

        let info: ReelInfo = serde_json::from_reader(File::open(&json_path)?)?;
        Ok((info, video_path))
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for FetchReelJob {
    #[tracing::instrument(skip(_queue))]
    async fn run(&self, _queue: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        let context = JOB_CONTEXT.get().ok_or(FangError {
            description: "Failed to read context".to_string(),
        })?;

        let video = self.exec(context).await.map_err(|e| {
            tracing::error!("{e:?}");
            FangError {
                description: e.to_string(),
            }
        })?;

        if let Some(video) = video {
            if self.auto_llm {
                let job = crate::jobs::llm_extract_details::LLmExtractDetailsJob {
                    video_id: video.id,
                };
                _queue.insert_task(&job).await?;
            }
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
