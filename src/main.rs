mod cli;
mod entities;
mod jobs;
mod error;

use crate::jobs::{JobContext, JOB_CONTEXT};
use anyhow::anyhow;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::rejection::{FormRejection, JsonRejection};
use axum::extract::{FromRequest, Path, Request};
use axum::http::header::{ACCEPT, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{routing::{get, post}, Extension, Form, Json, Router};
use axum_extra::routing::Resource;
use clap::Parser;
use cli::Cli;
use fang::{AsyncQueue, AsyncQueueable, AsyncWorkerPool, NoTls, Serialize};
use jobs::fetch_reel::FetchReelJob;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

use axum_template::{engine::Engine, RenderHtml};
use jobs::extract_transcript::ExtractTranscriptJob;
use minijinja::{path_loader, Environment};
use minijinja_autoreload::AutoReloader;
use notify::Watcher;
use sea_orm::DatabaseBackend::Postgres;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, EntityTrait, FromQueryResult, QuerySelect, Statement,
};
use serde::de::DeserializeOwned;
use sqlx::PgPool;
use tower_livereload::LiveReloadLayer;
use crate::jobs::llm_extract_details::LLmExtractDetailsJob;

type FangQueue = AsyncQueue<NoTls>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    cli.validate_reel_dir()?;

    let db = PgPool::connect(&cli.database_url).await?;

    let seaorm = sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(db.clone());

    sqlx::migrate!().run(&db).await?;

    let mut queue = AsyncQueue::builder()
        .uri(&cli.database_url)
        .max_pool_size(2u32)
        .build();

    queue.connect(NoTls).await?;

    let job_context = JobContext::new(
        seaorm.clone(),
        db.clone(),
        &cli.yt_dlp_path,
        cli.reel_dir.clone(),
        cli.openai_client()?,
        cli.whisper_url.clone(),
        cli.whisper_key.clone(),
        cli.openai_model,
    );

    JOB_CONTEXT.set(job_context.clone()).unwrap();

    // Set up the `minijinja` engine with the same route paths as the Axum router
    let jinja = AutoReloader::new(move |notifier| {
        let template_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("./app/views");

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

    let videos = Resource::named("videos").show(get_video);

    let livereload = LiveReloadLayer::new();
    let reloader = livereload.reloader();

    let app = Router::new()
        .merge(recipes)
        .merge(videos)
        .route("/", get(|| async { Redirect::to("/recipes") } ))
        .route("/videos/:id/llm", post(llm))
        .route("/videos/:id/transcribe", post(transcribe_video))
        .nest_service("/public", ServeDir::new("./public"))
        .layer(Extension(seaorm.clone()))
        .layer(Extension(db.clone()))
        .layer(Extension(queue.clone()))
        .layer(Extension(template_engine))
        .layer(Extension(job_context))
        .layer(livereload);

    let mut watcher = notify::recommended_watcher(move |_| {
        tracing::info!("Reloading...");
        reloader.reload()
    })?;

    watcher.watch(
        &PathBuf::from("./app/views"),
        notify::RecursiveMode::Recursive,
    )?;

    watcher.watch(&PathBuf::from("./public"), notify::RecursiveMode::Recursive)?;

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

async fn transcribe_video(
    Path((id, )): Path<(u32, )>,
    Extension(mut jobs): Extension<FangQueue>,
    Extension(db): Extension<DatabaseConnection>,
) -> error::Result<impl IntoResponse> {
    return match ExtractTranscriptJob::new(id as i32, &db).await {
        Ok(job) => {
            jobs.insert_task(&job).await?;
            Ok(StatusCode::CREATED.into_response())
        }

        Err(_) => Ok((StatusCode::BAD_REQUEST, "Invalid video id").into_response()),
    };
}

async fn llm(
    Path((id, )): Path<(u32, )>,
    Extension(mut jobs): Extension<FangQueue>,
) -> error::Result<impl IntoResponse> {
    let job = LLmExtractDetailsJob {
        video_id: id as i32,
    };

    jobs.insert_task(&job).await?;
    Ok(StatusCode::CREATED.into_response())
}

#[derive(Serialize, FromQueryResult)]
struct RecipeIdTitle {
    id: i32,
    title: String,
}

async fn recipes_index(
    header_map: HeaderMap,
    Extension(template_engine): Extension<Engine<AutoReloader>>,
    Extension(db): Extension<DatabaseConnection>,
) -> error::Result<impl IntoResponse> {
    match header_map.get(ACCEPT) {
        Some(hv) if is_json(hv) => {
            let recipes = entities::recipes::Entity::find().all(&db).await?;

            Ok(Json(recipes).into_response())
        }

        _ => {
            let recipes = entities::recipes::Entity::find()
                .select_only()
                .columns([
                    entities::recipes::Column::Id,
                    entities::recipes::Column::Title,
                ])
                .into_model::<RecipeIdTitle>()
                .all(&db)
                .await?;

            Ok(RenderHtml("recipes/index.html", template_engine, json!({ "recipes": recipes })).into_response())
        }
    }
}

async fn load_nested_recipe(recipe_id: i32, db: &DatabaseConnection) -> anyhow::Result<Value> {
    let recipe = db
        .query_one(Statement::from_sql_and_values(
            Postgres,
            r#"
        select to_jsonb(r) || jsonb_build_object('instagram_video', to_jsonb(iv)) as json
from recipes r
         left join public.instagram_video iv on iv.id = r.instagram_video_id
where r.id = $1;
        "#,
            vec![recipe_id.into()],
        ))
        .await?
        .ok_or(anyhow!("Unknown recipe id"))?
        .try_get_by::<Value, _>("json")?;

    Ok(recipe)
}

async fn show_recipe(
    header_map: HeaderMap,
    Extension(template_engine): Extension<Engine<AutoReloader>>,
    Extension(db): Extension<DatabaseConnection>,
    Path((recipe_id, )): Path<(u32, )>,
) -> impl IntoResponse {
    let Ok(recipe) = load_nested_recipe(recipe_id as i32, &db).await else {
        return (StatusCode::NOT_FOUND, "Recipe not found").into_response();
    };

    tracing::info!("Displaying Recipe: {:#?}", recipe);

    match header_map.get(ACCEPT) {
        Some(hv) if is_json(hv) => Json(recipe).into_response(),
        _ => RenderHtml("recipes/show.html", template_engine, recipe).into_response(),
    }
}

fn is_json(hv: &HeaderValue) -> bool {
    hv
        .to_str()
        .map(|hv| hv == "application/json")
        .unwrap_or(false)
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.expect("Failed to install CTRL+C handler");
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
) -> error::Result<impl IntoResponse> {
    let request = request.into_inner();

    return match FetchReelJob::new(request.reel_url) {
        Ok(job) => {
            queue.insert_task(&job).await?;
            Ok(StatusCode::CREATED.into_response())
        }

        Err(_) => Ok((StatusCode::BAD_REQUEST, "Invalid reel URL").into_response()),
    };
}

async fn get_video(
    Extension(context): Extension<JobContext>,
    headers: HeaderMap,
    Path((instagram_id, )): Path<(String, )>,
) -> error::Result<impl IntoResponse> {
    let video_path = context.video_path(&instagram_id);

    let mut req = Request::new(Body::empty());
    *req.headers_mut() = headers;
    Ok(ServeFile::new(video_path).try_call(req)
        .await
        .map_err(|e| anyhow!(e))?
    )
}
