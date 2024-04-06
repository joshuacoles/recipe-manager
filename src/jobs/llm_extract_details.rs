use fang::{AsyncRunnable, FangError};
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::async_trait;

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct LLmExtractDetailsJob {
    pub instagram_id: String,
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for LLmExtractDetailsJob {
    async fn run(&self, _queueable: &mut dyn AsyncQueueable) -> Result<(), FangError> {

    }

    fn uniq(&self) -> bool {
        true
    }
}
