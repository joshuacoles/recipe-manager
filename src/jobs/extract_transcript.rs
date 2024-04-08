use std::path::Path;
use async_openai::types::{AudioInput, AudioResponseFormat, CreateTranscriptionRequest, InputSource};
use async_trait::async_trait;
use fang::{AsyncQueueable, AsyncRunnable, Deserialize, FangError, Serialize};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, Set};
use crate::jobs::{JOB_CONTEXT, JobContext};

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub struct ExtractTranscript {
    pub(crate) reel_id: String,
    pub(crate) regenerate_recipe_info: bool
}


impl ExtractTranscript {
    pub fn new(reel_id: String) -> Self {
        Self { reel_id, regenerate_recipe_info: false }
    }

    pub async fn extract_transcript(&self, video_path: &Path, context: &JobContext) -> anyhow::Result<String> {
        // Does this work on ollama?
        let output = context.openai_client.audio().transcribe(CreateTranscriptionRequest {
            model: "whisper-1".to_string(),
            response_format: Some(AudioResponseFormat::Text),
            language: Some("en".to_string()),
            file: AudioInput {
                source: InputSource::Path { path: video_path.to_path_buf() }
            },
            timestamp_granularities: None,
            ..Default::default()
        }).await?;

        let content = output.text;

        Ok(content)
    }

    pub async fn exec(&self, context: &JobContext) -> anyhow::Result<()> {
        let video_path = context.video_path(&self.reel_id);
        let transcript = self.extract_transcript(&video_path, context).await?;

        let v = crate::entities::transcript::ActiveModel {
            id: NotSet,
            content: Set(transcript)
        }.save(&context.db).await?;

        crate::entities::recipes::Entity::update(crate::entities::recipes::ActiveModel {
            instagram_id: Set(self.reel_id.to_string()),
            transcript_id: Set(Some(v.id.unwrap())),

            ..Default::default()
        }).exec(&context.db).await?;

        Ok(())
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for ExtractTranscript {
    #[tracing::instrument(skip(queue))]
    async fn run(&self, queue: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        let context = JOB_CONTEXT.get()
            .ok_or(FangError { description: "Failed to read context".to_string() })?;

        self.exec(context).await
            .map_err(|e| FangError { description: e.to_string() })?;

        if self.regenerate_recipe_info {
            let recipes = crate::entities::recipes::Entity::find()
                .filter(crate::entities::recipes::Column::InstagramId.eq(&self.reel_id))
                .all(&context.db).await?;

            for recipe in recipes {
                queue.insert_task(&crate::jobs::llm_extract_details::LLmExtractDetailsJob {
                    instagram_id: self.reel_id.to_string()
                }).await?;
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