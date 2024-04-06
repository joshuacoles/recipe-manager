mod jobs;

use std::net::SocketAddr;
use std::path::PathBuf;
use clap::Parser;
use fang::{AsyncQueue, AsyncQueueable, AsyncWorkerPool, NoTls};
use jobs::fetch_reel::FetchReelJob;
use axum::{routing::{get}, Router, Extension};
use crate::jobs::{JobContext, JOB_CONTEXT};

#[derive(Debug, clap::Parser)]
struct Cli {
    /// Postgres connection url
    #[clap(short = 'd', long = "db", env = "RECIPE_DATABASE_URL", default_value = "postgres://postgres@localhost/recipes")]
    database_url: String,

    /// Server address
    #[clap(short = 'a', long = "address", env = "RECIPE_ADDRESS", default_value = "0.0.0.0:5005")]
    address: SocketAddr,

    /// Path to youtube-dl if not on PATH
    #[clap(long = "yt-dlp-path", env = "RECIPE_YT_DLP_PATH")]
    yt_dlp_path: Option<PathBuf>,

    /// Directory to save reels
    #[clap(short = 'r', long = "reel-dir", env = "RECIPE_REEL_DIR", default_value = "./reels")]
    reel_dir: PathBuf,

    /// OpenAI API key
    #[clap(long = "openai-api-key", env = "RECIPE_OPENAI_API_KEY", default_value="ollama")]
    openai_api_key: String,

    /// OpenAI API model
    #[clap(long = "model", env = "RECIPE_OPENAI_MODEL", default_value = "gemma")]
    openai_model: String,

    /// OpenAI Base url
    #[clap(long = "openai-base-url", env = "RECIPE_OPENAI_BASE_URL", default_value = "http://localhost:11434/v1")]
    openai_base_url: String,
}

impl Cli {
    fn validate_reel_dir(&self) -> anyhow::Result<()> {
        if !self.reel_dir.exists() {
            std::fs::create_dir(&self.reel_dir)?;
        } else if !self.reel_dir.is_dir() {
            anyhow::bail!("reel-dir must be a directory");
        }

        Ok(())
    }

    fn openai_client(&self) -> anyhow::Result<async_openai::Client<async_openai::config::OpenAIConfig>> {
        let config = async_openai::config::OpenAIConfig::new()
            .with_api_base(&self.openai_base_url)
            .with_api_key(&self.openai_api_key);

        let client = async_openai::Client::with_config(config);

        Ok(client)
    }
}

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
        cli.openai_model
    );
    JOB_CONTEXT.set(job_context.clone()).unwrap();

    let app = Router::new()
        .route("/", get(root))
        .layer(Extension(queue.clone()))
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
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.unwrap();
        })
        .await?;

    Ok(())
}

// basic handler that responds with a static string
async fn root(Extension(mut queue): Extension<FangQueue>) -> &'static str {
    queue.insert_task(&FetchReelJob::new(
        "https://www.instagram.com/reel/C4Y3U7-vKxF/?igsh=MTk4ZnRpdDlxa21qag==".to_string()
    )).await.unwrap();

    "Hello, World!"
}
