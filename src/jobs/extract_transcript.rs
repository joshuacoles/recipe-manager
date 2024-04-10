use crate::jobs::{JobContext, JOB_CONTEXT};
use anyhow::bail;
use async_openai::types::{
    AudioInput, AudioResponseFormat, CreateTranscriptionRequest,
    CreateTranscriptionResponseVerboseJson, InputSource, TimestampGranularity,
};
use async_trait::async_trait;
use fang::{AsyncQueueable, AsyncRunnable, Deserialize, FangError, Serialize};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, QueryFilter, QuerySelect, Set};
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub struct ExtractTranscript {
    pub(crate) video_id: i32,
}

impl ExtractTranscript {
    async fn extract_transcript(
        &self,
        video_path: &Path,
        context: &JobContext,
    ) -> anyhow::Result<CreateTranscriptionResponseVerboseJson> {
        let audio_path = self.extract_audio(&video_path).await?;

        // Does this work on ollama?
        // Answer: no
        let output = context
            .openai_direct_client
            .audio()
            .transcribe_verbose_json(CreateTranscriptionRequest {
                model: "whisper-1".to_string(),
                response_format: Some(AudioResponseFormat::VerboseJson),
                language: Some("en".to_string()),
                file: AudioInput {
                    source: InputSource::Path { path: audio_path },
                },
                timestamp_granularities: Some(vec![TimestampGranularity::Segment]),
                ..Default::default()
            })
            .await?;

        Ok(output)
    }

    async fn extract_audio(&self, video_path: &Path) -> anyhow::Result<PathBuf> {
        let audio_path = video_path.with_extension("audio.mp3");

        if audio_path.exists() {
            tracing::info!("Audio already extracted");
            return Ok(audio_path);
        }

        let output = Command::new("ffmpeg")
            .arg("-i")
            .arg(video_path)
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

    pub async fn exec(&self, context: &JobContext) -> anyhow::Result<()> {
        let (reel_id, existing_transcript) = crate::entities::instagram_video::Entity::find()
            .select_only()
            .columns([
                crate::entities::instagram_video::Column::InstagramId,
                crate::entities::instagram_video::Column::TranscriptId,
            ])
            .filter(crate::entities::instagram_video::Column::Id.eq(self.video_id))
            .into_tuple::<(String, Option<i32>)>()
            .one(&context.db)
            .await?
            .ok_or(anyhow::anyhow!("Video not found"))?;

        // Remove existing transcript if it exists
        if let Some(existing_transcript) = existing_transcript {
            tracing::info!("Removing existing transcript");

            crate::entities::instagram_video::Entity::update(
                crate::entities::instagram_video::ActiveModel {
                    id: Set(self.video_id),
                    transcript_id: Set(None),
                    ..Default::default()
                },
            )
            .exec(&context.db)
            .await?;

            crate::entities::transcript::Entity::delete_by_id(existing_transcript)
                .exec(&context.db)
                .await?;
        }

        let video_path = context.video_path(&reel_id);
        let transcript = self.extract_transcript(&video_path, context).await?;

        let v = crate::entities::transcript::ActiveModel {
            id: NotSet,
            content: Set(transcript.text.clone()),
            json: Set(Some(serde_json::to_value(transcript)?)),
        }
        .save(&context.db)
        .await?;

        crate::entities::instagram_video::Entity::update(
            crate::entities::instagram_video::ActiveModel {
                id: Set(self.video_id),
                transcript_id: Set(Some(v.id.unwrap())),
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
impl AsyncRunnable for ExtractTranscript {
    #[tracing::instrument(skip(queue))]
    async fn run(&self, queue: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        let context = JOB_CONTEXT.get().ok_or(FangError {
            description: "Failed to read context".to_string(),
        })?;

        self.exec(context).await.map_err(|e| FangError {
            description: e.to_string(),
        })?;

        queue
            .insert_task(&crate::jobs::llm_extract_details::LLmExtractDetailsJob {
                video_id: self.video_id,
            })
            .await?;

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
