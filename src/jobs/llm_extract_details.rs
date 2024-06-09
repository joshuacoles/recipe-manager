use crate::entities::instagram_video::Model;
use crate::entities::{instagram_video, recipes};
use crate::jobs::{JobContext, JOB_CONTEXT};
use anyhow::anyhow;
use async_openai::types::CreateChatCompletionResponse;
use clap::ValueEnum;
use fang::async_trait;
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::{AsyncRunnable, FangError};
use reqwest::Response;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{DatabaseConnection, EntityTrait, Set};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::fmt::Debug;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, ValueEnum)]
pub enum LlmMethod {
    GenericOpenAI,
    OllamaJson,
    OpenAITools,
    AnthropicTools,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaGenerateResponse {
    response: String,

    #[serde(flatten)]
    rest: Value,
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

    async fn handle_response<T: DeserializeOwned + Debug>(response: Response) -> anyhow::Result<T> {
        if !response.status().is_success() {
            tracing::error!("Failed to send request, response metadata: {:#?}", response);
            let response_status_code = response.status().as_u16();
            let response_body = response.text().await?;
            tracing::error!(
                "Failed to send request, response content: {:#?}",
                response_body
            );
            return Err(anyhow!("Failed to send request: {}", response_status_code));
        }

        let response = response.text().await?;
        tracing::info!("Response raw received: {}", response);

        let response = serde_json::from_str::<T>(&response)?;
        tracing::info!("Response received: {:#?}", response);

        Ok(response)
    }

    fn fetch_prompt(&self, llm_method: LlmMethod) -> String {
        let dynamic = true;

        let prompt_template = if dynamic {
            std::fs::read_to_string("app/prompts/extract_recipe_details.txt").unwrap()
        } else {
            include_str!("../../app/prompts/extract_recipe_details.txt").to_string()
        };

        prompt_template
    }

    async fn extract_recipes(
        &self,
        context: &JobContext,
        instagram_video: &Model,
    ) -> anyhow::Result<Vec<ExtractedRecipe>> {
        let completion_url = &context.completion_url;
        let api_key = &context.completion_key;
        let llm_model = &context.completion_model;

        let transcript = match &instagram_video.transcript {
            Some(transcript) => &transcript.text,
            None => "",
        };

        let description = instagram_video.info.description.as_str();

        let prompt_template = self.fetch_prompt(context.completion_mode);
        let prompt = Self::assemble_prompt(&prompt_template, description, transcript);
        tracing::info!("Prompt prepared");

        match context.completion_mode {
            LlmMethod::GenericOpenAI => {
                let request = json!({
                    "model": llm_model,
                    "messages": [
                        {
                            "role": "user",
                            "content": prompt,
                        }
                    ]
                });

                tracing::info!("Request prepared: {:#?}", request);

                let response = reqwest::Client::new()
                    .post(completion_url)
                    .bearer_auth(api_key)
                    .json(&request)
                    .send()
                    .await?;

                let response: CreateChatCompletionResponse =
                    Self::handle_response(response).await?;

                let response = response.choices[0]
                    .message
                    .content
                    .clone()
                    .ok_or(anyhow!("No content in response"))?;

                // If we are not in Llama json mode, we may need to strip a code block
                let message = response.replace("```json\n", "").replace("```", "");

                return Ok(serde_json::from_str::<AcceptableResponses>(&message)?.retrieve());
            }

            LlmMethod::OllamaJson => {
                let request = json!({
                    "model": llm_model,
                    "prompt": prompt,
                    "format": "json",
                    "stream": false,
                });

                tracing::info!("Request prepared: {:#?}", request);

                let response = reqwest::Client::new()
                    .post(completion_url)
                    .bearer_auth(api_key)
                    .json(&request)
                    .send()
                    .await?;

                let response: OllamaGenerateResponse = Self::handle_response(response).await?;

                // If we are not in Llama json mode, we may need to strip a code block
                let message = response
                    .response
                    .replace("```json\n", "")
                    .replace("```", "");

                return Ok(serde_json::from_str::<AcceptableResponses>(&message)?.retrieve());
            }

            LlmMethod::OpenAITools => {
                unimplemented!()
            }

            LlmMethod::AnthropicTools => {
                unimplemented!()
            }
        }
    }

    fn assemble_prompt(prompt_template: &String, description: &str, transcript: &str) -> String {
        let env = {
            let mut env = minijinja::Environment::new();
            env.add_template("prompt", &prompt_template).unwrap();
            env
        };

        let template = env.get_template("prompt").unwrap();

        let prompt = template
            .render(json!({
                "description": description,
                "transcript": transcript,
            }))
            .unwrap();
        prompt
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
