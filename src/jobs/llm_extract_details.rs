use async_openai::config::OpenAIConfig;
use async_openai::types::{ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent, CreateChatCompletionRequest};
use fang::{AsyncRunnable, FangError};
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::async_trait;
use crate::jobs::JOB_CONTEXT;

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct LLmExtractDetailsJob {
    pub instagram_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Response {
    title: String,
    ingredients: Vec<String>,
    instructions: Vec<String>,
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for LLmExtractDetailsJob {
    #[tracing::instrument(skip(_queueable))]
    async fn run(&self, _queueable: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        tracing::info!("Using LLM to extract details from recipe description");
        let context = JOB_CONTEXT.get().unwrap();

        let client = async_openai::Client::with_config(OpenAIConfig::new()
            .with_api_base("http://localhost:11434/v1")
            .with_api_key("gemma")
        );

        let (description, ) = sqlx::query_as::<_, (String, )>("select info_json ->> 'description' from unprocessed_recipes where instagram_id = $1")
            .bind(&self.instagram_id)
            .fetch_one(&context.db)
            .await.unwrap();

        let prompt = format!(
            "{prompt}:\n{description}",
            prompt = r#"/gptThis the instagram reel description of a recipie. Please extract the title of the recipie, an ingredients list, ordered instructions, and any useful notes from the description. Remove all extreneous information such as the author, biographical information, tags, someone's life story, requests for engagement, etc, only include the information I have requested, no yapping. Please provide your answer in a clear and consise manner but crucially do not skip details. There may be multiple recipies included in the description. If so please make sure to separate these out clearly with different titles and other information. Please provide this information as an array of JSON objects, one per recipie in the description. Each object you output will have three properties: "ingredients", "instructions", and "title". The "ingredients" key should contain arrays of strings, where each item in the list is an ingredient. The "instructions" key should contain arrays of strings, where each item in the list isa step in the instructions. The "title" key should be a string which is the title of the recipe. Do not wrap the JSON output in a code-block, or include any text before or after the JSON. Here is the description:"#,
            description = description
        );

        tracing::info!("Prompt prepared");

        let completion = CreateChatCompletionRequest {
            model: "gemma".to_string(),
            messages: vec![
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(prompt),
                    ..Default::default()
                })
            ],

            ..Default::default()
        };

        let completion = client.chat().create(completion).await.unwrap();
        tracing::info!("Response received: {:#?}", completion);

        let response = &completion.choices[0];
        let message = response.message.content.clone().unwrap();
        let message = message.replace("```json\n", "").replace("```", "");

        // TODO: Multiple recipes
        let recipes_in_description = &serde_json::from_str::<Vec<Response>>(&message).unwrap();
        tracing::info!("Found {} recipes in description", recipes_in_description.len());

        let mut txn = context.db.begin().await.unwrap();

        for recipe in recipes_in_description {
            tracing::info!("Add completed recipe to database", title=recipe.title);

            sqlx::query(r#"
                insert into recipes (instagram_id, title, raw_description, ingredients, instructions, info_json, instagram_url)
                select instagram_id, $2, info_json ->> 'description', $3, $4, info_json, instagram_url
                from unprocessed_recipes
                where instagram_id = $1
            "#)
                .bind(&self.instagram_id)
                .bind(&recipe.title)
                .bind(&recipe.ingredients)
                .bind(&recipe.instructions)
                .execute(&mut *txn)
                .await
                .unwrap();
        }

        sqlx::query("delete from unprocessed_recipes where instagram_id = $1")
            .bind(&self.instagram_id)
            .execute(&mut *txn)
            .await
            .unwrap();

        txn.commit().await.unwrap();
        tracing::info!("Added completed recipe to database");

        Ok(())
    }

    fn uniq(&self) -> bool {
        true
    }
}
