use fang::{AsyncRunnable, FangError};
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::async_trait;

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct FetchReelJob {
    pub reel_url: String,
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for FetchReelJob {
    async fn run(&self, _queueable: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        dbg!(self);
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