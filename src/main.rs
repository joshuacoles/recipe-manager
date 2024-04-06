mod jobs;
mod cli;

use std::path::PathBuf;
use clap::Parser;
use fang::{AsyncQueue, AsyncQueueable, AsyncWorkerPool, NoTls};
use jobs::fetch_reel::FetchReelJob;
use axum::{Extension, Json, Router, routing::get};
use axum::body::Body;
use axum::extract::Path;
use axum::http::{HeaderMap, Request};
use axum::response::IntoResponse;
use axum::routing::post;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use tower_http::services::ServeFile;
use cli::Cli;
use crate::jobs::{JOB_CONTEXT, JobContext};

use axum_template::{engine::Engine, RenderHtml};
use minijinja::{path_loader, Environment};
use minijinja_autoreload::AutoReloader;

type FangQueue = AsyncQueue<NoTls>;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    cli.validate_reel_dir()?;

    let db = sqlx::PgPool::connect(&cli.database_url)
        .await?;

    sqlx::migrate!()
        .run(&db)
        .await?;

    let mut queue = AsyncQueue::builder()
        .uri(&cli.database_url)
        .max_pool_size(2u32)
        .build();

    queue.connect(NoTls)
        .await?;

    let job_context = JobContext::new(
        db.clone(),
        &cli.yt_dlp_path,
        cli.reel_dir.clone(),
        cli.openai_client()?,
        cli.openai_model,
    );

    JOB_CONTEXT.set(job_context.clone()).unwrap();

    // Set up the `minijinja` engine with the same route paths as the Axum router
    let jinja = AutoReloader::new(move |notifier| {
        let template_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("views");

        let mut env = Environment::new();
        env.set_loader(path_loader(&template_path));
        notifier.set_fast_reload(true);
        notifier.watch_path(&template_path, true);
        Ok(env)
    });

    let template_engine = Engine::from(jinja);

    let app = Router::new()
        .route("/api/recipes/read/:id", get(get_recipe))
        .route("/api/recipes/from_reel", post(create_recipe_from_reel))
        .route("/videos/:instagram_id", get(get_video))
        .route("/recipes/:id", get(view_recipe))
        .layer(Extension(db.clone()))
        .layer(Extension(queue.clone()))
        .layer(Extension(template_engine))
        .layer(Extension(job_context));

    let mut pool: AsyncWorkerPool<AsyncQueue<NoTls>> = AsyncWorkerPool::builder()
        .number_of_workers(2u32)
        .queue(queue)
        .build();

    // This await does nothing, the method is entirely synchronous
    pool.start().await;

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind(&cli.address).await.unwrap();
    tracing::info!("Listening on {}", cli.address);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn view_recipe(
    Extension(template_engine): Extension<Engine<AutoReloader>>,
    Extension(db): Extension<PgPool>,
    Path((recipe_id, )): Path<(u32, )>
) -> impl IntoResponse {
    let recipe: Recipe = sqlx::query_as("select * from recipes where id = $1")
        .bind(recipe_id as i32)
        .fetch_one(&db)
        .await.unwrap();

    RenderHtml("recipe.html", template_engine, recipe)
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.unwrap();
}

#[derive(Deserialize, Debug)]
struct CreateRecipeFromReelRequest {
    reel_url: String,
}

#[derive(Serialize, Deserialize, FromRow, Debug)]
struct Recipe {
    id: i32,
    instagram_id: String,
    title: String,
    raw_description: String,
    ingredients: Vec<String>,
    instructions: Vec<String>,
    info_json: Value,
    instagram_url: String,
    updated_at: Option<DateTime<Utc>>,
}

async fn get_recipe(Extension(context): Extension<JobContext>, Path((recipe_id, )): Path<(u32, )>) -> impl IntoResponse {
    let recipe: Recipe = sqlx::query_as("select * from recipes where id = $1")
        .bind(recipe_id as i32)
        .fetch_one(&context.db)
        .await
        .unwrap();

    Json(recipe)
}

async fn create_recipe_from_reel(Extension(mut queue): Extension<FangQueue>, Json(request): Json<CreateRecipeFromReelRequest>) -> &'static str {
    queue.insert_task(&FetchReelJob::new(request.reel_url)).await.unwrap();

    "Recipe creation task queued"
}

async fn get_video(Extension(context): Extension<JobContext>, headers: HeaderMap, Path((instagram_id, )): Path<(String, )>) -> impl IntoResponse {
    let video_path = context.video_path(&instagram_id);

    let mut req = Request::new(Body::empty());
    *req.headers_mut() = headers;
    ServeFile::new(video_path).try_call(req).await.unwrap()
}
