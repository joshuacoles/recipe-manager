use std::fs::File;
use std::path::PathBuf;
use tokio::process::Command;
use fang::{AsyncRunnable, FangError};
use fang::asynk::async_queue::AsyncQueueable;
use fang::serde::{Deserialize, Serialize};
use fang::async_trait;
use tempfile::TempDir;
use crate::jobs::JOB_CONTEXT;

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "fang::serde")]
pub(crate) struct FetchReelJob {
    pub(crate) reel_url: String,
}

impl FetchReelJob {
    pub fn new(reel_url: String) -> Self {
        Self {
            reel_url,
        }
    }
}

#[typetag::serde]
#[async_trait]
impl AsyncRunnable for FetchReelJob {
    async fn run(&self, _queueable: &mut dyn AsyncQueueable) -> Result<(), FangError> {
        let context = JOB_CONTEXT.get().unwrap();
        // let temp_dir = TempDir::new()?;
        let dir = PathBuf::from("./scratch");

        let yt_dlp_output = Command::new(&context.yt_dlp_command_string)
            // .current_dir(&temp_dir.path())
            .current_dir(&dir)
            .args(&["--write-info-json", "-o", "reel.%(ext)s", &self.reel_url])
            .output()
            .await?;

        if !yt_dlp_output.status.success() {
            let description = format!("yt-dlp failed {}: {}", yt_dlp_output.status, String::from_utf8_lossy(&yt_dlp_output.stderr));
            return Err(FangError { description });
        }

        let info = serde_json::from_reader(File::open(&dir.join("reel.info.json")).unwrap()).unwrap();
        let video_path = dir.join("reel.mp4");

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