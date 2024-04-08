mod jobs;
mod cli;
mod entities;

use std::path::PathBuf;
use async_trait::async_trait;
use clap::Parser;
use fang::{AsyncQueue, AsyncQueueable, AsyncWorkerPool, NoTls};
use jobs::fetch_reel::FetchReelJob;
use axum::{Extension, Form, Json, Router, routing::get};
use axum::body::Body;
use axum::extract::{FromRequest, Path, Request};
use axum::extract::rejection::{FormRejection, JsonRejection};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::http::header::{ACCEPT, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum_extra::routing::Resource;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use tower_http::services::{ServeDir, ServeFile};
use cli::Cli;
use crate::jobs::{JOB_CONTEXT, JobContext};

use axum_template::{engine::Engine, RenderHtml};
use minijinja::{path_loader, Environment};
use minijinja_autoreload::AutoReloader;
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::de::DeserializeOwned;

type FangQueue = AsyncQueue<NoTls>;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    cli.validate_reel_dir()?;

    let db = sqlx::PgPool::connect(&cli.database_url)
        .await?;

    let seaorm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(db.clone());

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
        seaorm.clone(),
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
            .join("./app/views");

        let mut env = Environment::new();
        env.set_loader(path_loader(&template_path));
        notifier.set_fast_reload(true);
        notifier.watch_path(&template_path, true);
        Ok(env)
    });

    let template_engine = Engine::from(jinja);

    let recipes = Resource::named("recipes")
        // Define a route for `GET /recipes`
        .index(recipes_index)
        // `POST /recipes`
        .create(create_recipe_from_reel)
        // `GET /recipes/:id`
        .show(show_recipe);

    let app = Router::new()
        .route("/videos/:instagram_id", get(get_video))
        .merge(recipes)
        .nest_service("/public", ServeDir::new("./public"))
        .layer(Extension(seaorm.clone()))
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

async fn recipes_index(
    header_map: HeaderMap,
    Extension(template_engine): Extension<Engine<AutoReloader>>,
    Extension(db): Extension<DatabaseConnection>,
) -> impl IntoResponse {
    let recipes = entities::recipes::Entity::find()
        .all(&db)
        .await
        .unwrap();

    match header_map.get(ACCEPT) {
        Some(hv) if hv.to_str().map(|hv| hv == "application/json").unwrap_or(false) =>
            Json(recipes).into_response(),
        _ => RenderHtml("index.html", template_engine, json!({ "recipes": recipes })).into_response()
    }
}

async fn show_recipe(
    header_map: HeaderMap,
    Extension(template_engine): Extension<Engine<AutoReloader>>,
    Extension(db): Extension<DatabaseConnection>,
    Path((recipe_id, )): Path<(u32, )>,
) -> impl IntoResponse {
    let recipe = entities::recipes::Entity::find_by_id(recipe_id as i32)
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    match header_map.get(ACCEPT) {
        Some(hv) if hv.to_str().map(|hv| hv == "application/json").unwrap_or(false) =>
            Json(recipe).into_response(),
        _ => RenderHtml("recipe.html", template_engine, recipe).into_response()
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.unwrap();
}

#[derive(Deserialize, Debug)]
struct CreateRecipeFromReelRequest {
    reel_url: String,
}

enum FormOrJson<T> {
    Form(T),
    Json(T),
}

impl<T> FormOrJson<T> {
    fn into_inner(self) -> T {
        match self {
            FormOrJson::Form(t) => t,
            FormOrJson::Json(t) => t,
        }
    }
}

#[async_trait]
impl<T, S> FromRequest<S> for FormOrJson<T>
    where
        T: DeserializeOwned,
        S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        if req.headers().get(CONTENT_TYPE) == Some(&HeaderValue::from_static("application/json")) {
            Json::<T>::from_request(req, state)
                .await
                .map(|json| FormOrJson::Json(json.0))
                .map_err(JsonRejection::into_response)
        } else {
            Form::<T>::from_request(req, state)
                .await
                .map(|form| FormOrJson::Form(form.0))
                .map_err(FormRejection::into_response)
        }
    }
}

async fn create_recipe_from_reel(
    Extension(mut queue): Extension<FangQueue>,
    request: FormOrJson<CreateRecipeFromReelRequest>,
) -> impl IntoResponse {
    let request = request.into_inner();

    return match FetchReelJob::new(request.reel_url) {
        Ok(job) => {
            queue.insert_task(&job).await.unwrap();
            (StatusCode::OK, "Recipe creation task queued")
        }

        Err(_) => (StatusCode::BAD_REQUEST, "Invalid reel URL"),
    };
}

async fn get_video(Extension(context): Extension<JobContext>, headers: HeaderMap, Path((instagram_id, )): Path<(String, )>) -> impl IntoResponse {
    let video_path = context.video_path(&instagram_id);

    let mut req = Request::new(Body::empty());
    *req.headers_mut() = headers;
    ServeFile::new(video_path).try_call(req).await.unwrap()
}
