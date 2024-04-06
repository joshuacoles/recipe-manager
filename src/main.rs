mod fetch_reel_job;

use std::net::SocketAddr;
use clap::Parser;
use fang::{AsyncQueue, AsyncQueueable, AsyncRunnable, AsyncWorkerPool, NoTls};
use fetch_reel_job::FetchReelJob;
use axum::{routing::{get}, Router, Extension};

#[derive(Debug, clap::Parser)]
struct Cli {
    /// Postgres connection url
    #[clap(short = 'd', long = "db", env = "RECIPE_DATABASE_URL", default_value = "postgres://postgres@localhost/recipes")]
    database_url: String,

    /// Server address
    #[clap(short = 'a', long = "address", env = "RECIPE_ADDRESS", default_value = "0.0.0.0:5005")]
    address: SocketAddr,
}

type FangQueue = AsyncQueue<NoTls>;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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

    // build our application with a route
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(root))
        .layer(Extension(queue.clone()));

    let mut pool: AsyncWorkerPool<AsyncQueue<NoTls>> = AsyncWorkerPool::builder()
        .number_of_workers(2u32)
        .queue(queue)
        .build();

    pool.start().await;

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind(&cli.address).await.unwrap();
    axum::serve(listener, app)
        .await?;

    Ok(())
}

// basic handler that responds with a static string
async fn root(Extension(mut queue): Extension<FangQueue>) -> &'static str {
    queue.insert_task(&FetchReelJob {
        reel_url: "https://www.instagram.com/reel/CJ9Qvz1g1Zb/".to_string()
    } as &dyn AsyncRunnable).await.unwrap();

    "Hello, World!"
}
