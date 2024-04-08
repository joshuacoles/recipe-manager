use std::path::Path;
use async_trait::async_trait;
use fang::AsyncRunnable;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
use crate::jobs::JobContext;

pub struct ExtractTranscript {
    pub(crate) reel_id: String,
}


impl ExtractTranscript {
    pub fn new(reel_id: String) -> Self {
        Self { reel_id }
    }

    pub async fn extract_transcript(&self, video_path: &Path, context: &JobContext) -> anyhow::Result<String> {
        // load a context and model
        let ctx = WhisperContext::new_with_params(
            "/Users/joshuacoles/Library/Application Support/MacWhisper/models/ggml-model-whisper-small.en.bin",
            WhisperContextParameters::default(),
        ).expect("Failed to load model");

        // create a params object
        let params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        let audio_data = std::fs::read(video_path)?;

        // assume we have a buffer of audio data
        // here we'll make a fake one, floating point samples, 32 bit, 16KHz, mono
        let audio_data = vec![0_f32; 16000 * 2];

        // now we can run the model
        let mut state = ctx.create_state()?;
        state.full(params, &audio_data[..])?;

        // fetch the results
        let num_segments = state
            .full_n_segments()
            .expect("failed to get number of segments");

        let content = (0..num_segments)
            .map(|i| state.full_get_segment_text(i))
            .collect::<Result<Vec<_>, _>>()?.join("\n\n");

        Ok(content)
    }

    pub async fn exec(&self, context: &JobContext) -> anyhow::Result<()> {
        let video_path = context.video_path(&self.reel_id);
        let transcript = self.extract_transcript(&video_path, context).await?;

        sqlx::query("insert into transcripts (reel_id, transcript) values ($1, $2)")
            .bind(&self.reel_id)
            .bind(&transcript)
            .execute(&context.db)
            .await?;

        Ok(())
    }
}

// #[async_trait]
// impl AsyncRunnable for ExtractTranscript {
//     async fn run(&self) -> Result<()> {
//         let ctx = JOB_CONTEXT.get().expect("Job context not set");
//         let db = &ctx.db;
//
//         let video_path = ctx.video_path(&self.reel_id);
//         let video_path = video_path.to_str().expect("Invalid video path");
//
//         let transcript = extract_transcript(video_path).await?;
//
//         let mut tx = db.begin().await?;
//         sqlx::query("insert into transcripts (reel_id, transcript) values ($1, $2)")
//             .bind(&self.reel_id)
//             .bind(&transcript)
//             .execute(&mut tx)
//             .await?;
//
//         tx.commit().await?;
//
//         Ok(())
//     }
// }
