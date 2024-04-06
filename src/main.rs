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
}

type FangQueue = AsyncQueue<NoTls>;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let db = sqlx::PgPool::connect(&cli.database_url)
        .await?;

    sqlx::migrate!().run(&db)
        .await?;

    let mut queue = AsyncQueue::builder()
        .uri(&cli.database_url)
        .max_pool_size(2u32)
        .build();

    queue.connect(NoTls)
        .await?;

    let job_context = JobContext::new(db.clone(), &cli.yt_dlp_path);
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
