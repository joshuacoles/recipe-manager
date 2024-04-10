use std::collections::HashMap;
use crate::jobs::{JobContext, JOB_CONTEXT};
use anyhow::bail;
use async_trait::async_trait;
use fang::{AsyncQueueable, AsyncRunnable, Deserialize, FangError, Serialize};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, Set};
use std::path::{Path, PathBuf};
use serde_json::Value;
use tokio::process::Command;
use reqwest::{multipart, Body, Client};
use tokio::fs::File;
use tokio_util::codec::{BytesCodec, FramedRead};

#[derive(Debug, Serialize, Deserialize)]
pub struct Transcript {
    text: String,
    segments: Vec<Segment>,

    #[serde(flatten)]
    other: HashMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Segment {
    id: i32,
    start: f64,
    end: f64,
    text: String,

    #[serde(flatten)]
    other: HashMap<String, Value>,
}

pub struct ExtractTranscript;

impl ExtractTranscript {
    #[tracing::instrument(skip(context))]
    pub async fn extract_transcript(
        context: &JobContext,
        video_path: &Path,
    ) -> anyhow::Result<Transcript> {
        tracing::info!("Extracting transcript");
        let audio_path = Self::extract_audio(&video_path).await?;
        let audio_file = File::open(audio_path).await?;
        let audio = FramedRead::new(audio_file, BytesCodec::new());
        let audio = Body::wrap_stream(audio);
        let audio = multipart::Part::stream(audio)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let output = Client::new()
            .post(&format!("{}", context.whisper_url))
            .bearer_auth(&context.whisper_key)
            .multipart(
                multipart::Form::new()
                    .text("model", "whisper-1")
                    .text("response_format", "verbose_json")
                    .text("language", "en")
                    .part("file", audio)
            )
            .send()
            .await?
            .json()
            .await?;

        Ok(output)
    }

    async fn extract_audio(video_path: &Path) -> anyhow::Result<PathBuf> {
        let audio_path = video_path.with_extension("audio.wav");

        if audio_path.exists() {
            tracing::info!("Audio already extracted");
            return Ok(audio_path);
        }

        let output = Command::new("ffmpeg")
            .arg("-i")
            .arg(video_path)
            .args(&["-vn", "-ar", "16000", "-ac", "2", "-ab", "320k"])
            .arg(&audio_path)
            .output()
            .await?;

        if !output.status.success() {
            bail!(
                "Error occurred when extracting audio {:?}",
                String::from_utf8_lossy(&output.stderr)
            )
        }

        Ok(audio_path)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub struct ExtractTranscriptJob {
    pub video_id: i32,
    pub reel_id: String,
}

impl ExtractTranscriptJob {
    pub async fn new(video_id: i32, db: &DatabaseConnection) -> anyhow::Result<Self> {
        let (reel_id, ) = crate::entities::instagram_video::Entity::find()
            .select_only()
            .columns([crate::entities::instagram_video::Column::InstagramId])
            .filter(crate::entities::instagram_video::Column::Id.eq(video_id))
            .into_tuple::<(String, )>()
            .one(db)
            .await?
            .ok_or(anyhow::anyhow!("Video not found"))?;

        Ok(Self { video_id, reel_id })
    }

    async fn exec(&self, context: &JobContext) -> anyhow::Result<()> {
        let video_path = context.video_path(&self.reel_id);
        let transcript = ExtractTranscript::extract_transcript(context, &video_path).await?;

        crate::entities::instagram_video::Entity::update(
            crate::entities::instagram_video::ActiveModel {
                id: Set(self.video_id),
                transcript: Set(Some(serde_json::to_value(transcript).unwrap())),
                ..Default::default()
            },
        )
            .exec(&context.db)
            .await?;

        Ok(())
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for ExtractTranscriptJob {
    #[tracing::instrument(skip(_queue))]
    async fn run(&self, _queue: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        let context = JOB_CONTEXT.get().ok_or(FangError {
            description: "Failed to read context".to_string(),
        })?;

        self.exec(context).await.map_err(|e| FangError {
            description: e.to_string(),
        })?;

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
