use crate::entities::instagram_video::Model;
use crate::entities::{instagram_video, recipes};
use crate::jobs::{JobContext, JOB_CONTEXT};
use anyhow::anyhow;
use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, CreateChatCompletionRequest,
};
use async_openai::Client;
use fang::async_trait;
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::{AsyncRunnable, FangError};
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{DatabaseConnection, EntityTrait, Set};

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct LLmExtractDetailsJob {
    pub video_id: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExtractedRecipe {
    title: String,
    ingredients: Vec<String>,
    instructions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AcceptableResponses {
    Array(Vec<ExtractedRecipe>),
    Object { recipes: Vec<ExtractedRecipe> },
}

impl AcceptableResponses {
    fn retrieve(self) -> Vec<ExtractedRecipe> {
        match self {
            AcceptableResponses::Array(recipes) => recipes,
            AcceptableResponses::Object { recipes } => recipes,
        }
    }
}

impl LLmExtractDetailsJob {
    async fn exec(&self, context: &JobContext) -> anyhow::Result<()> {
        tracing::info!("Using LLM to extract details from recipe description");

        let video: Model = instagram_video::Entity::find()
            .filter(instagram_video::Column::Id.eq(self.video_id))
            .one(&context.db)
            .await?
            .ok_or(anyhow!("Video not found"))?;

        let recipes_in_description = self.extract_recipes(context, &video).await?;
        tracing::info!(
            "Found {} recipes in description",
            recipes_in_description.len()
        );

        self.save_newly_recipes(&context.db, &recipes_in_description, &video)
            .await?;
        tracing::info!("Added completed recipe to database");

        Ok(())
    }

    async fn extract_recipes(
        &self,
        context: &JobContext,
        instagram_video: &Model,
    ) -> anyhow::Result<Vec<ExtractedRecipe>> {
        let client = &context.openai_client;
        let llm_model = &context.model;

        let transcript = match &instagram_video.transcript {
            Some(transcript) => transcript
                .get("text")
                .ok_or(anyhow!("No text in transcript"))?
                .as_str()
                .expect("Failed to convert transcript to string")
                .to_string(),

            None => String::new(),
        };

        let description = instagram_video
            .info
            .get("description")
            .ok_or(anyhow!("No description in video"))?
            .as_str()
            .ok_or(anyhow!("Description is not a string"))?;

        let dynamic = true;

        let prompt_template = if dynamic {
            std::fs::read_to_string("app/prompts/extract_recipe_details.txt").unwrap()
        } else {
            include_str!("../../app/prompts/extract_recipe_details.txt").to_string()
        };

        let env = {
            let mut env = minijinja::Environment::new();
            env.add_template("prompt", &prompt_template).unwrap();
            env
        };

        let template = env.get_template("prompt").unwrap();

        let prompt = template.render(serde_json::json!({
            "description": description,
            "transcript": transcript,
        })).unwrap();

        tracing::info!("Prompt prepared");

        let completion = CreateChatCompletionRequest {
            model: llm_model.clone(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(prompt),
                    ..Default::default()
                },
            )],

            ..Default::default()
        };

        let recipes_in_description = Self::parse_response(client, completion).await?;
        Ok(recipes_in_description)
    }

    async fn parse_response(
        client: &Client<OpenAIConfig>,
        response: CreateChatCompletionRequest,
    ) -> anyhow::Result<Vec<ExtractedRecipe>> {
        let completion = client.chat().create(response).await?;
        tracing::info!("Response received: {:#?}", completion);

        let response = &completion.choices[0];
        let message = response
            .message
            .content
            .clone()
            .ok_or(anyhow!("No content in response"))?;
        let message = message.replace("```json\n", "").replace("```", "");

        let recipes_in_description =
            serde_json::from_str::<AcceptableResponses>(&message)?.retrieve();

        Ok(recipes_in_description)
    }

    async fn save_newly_recipes(
        &self,
        db: &DatabaseConnection,
        recipes_in_description: &[ExtractedRecipe],
        instagram_video: &Model,
    ) -> anyhow::Result<()> {
        recipes::Entity::insert_many(recipes_in_description.iter().map(|recipe| {
            recipes::ActiveModel {
                instagram_video_id: Set(Some(instagram_video.id.clone())),
                title: Set(Some(recipe.title.clone())),
                ingredients: Set(Some(recipe.ingredients.clone())),
                instructions: Set(Some(recipe.instructions.clone())),
                generated_at: Set(Some(chrono::Utc::now().fixed_offset())),

                ..Default::default()
            }
        }))
            .exec(db)
            .await?;

        Ok(())
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for LLmExtractDetailsJob {
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
}
