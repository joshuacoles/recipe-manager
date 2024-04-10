use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use fang::{AsyncQueueError, FangError};
use sea_orm::DbErr;
use tracing::log::error;

pub type Result<T, E = AppError> = std::result::Result<T, E>;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    /// SeaORM error, separated for ease of use allowing us to `?` db operations.
    #[error("Internal error")]
    DbError(#[from] DbErr),

    /// Fang error
    #[error("Job scheduler error {0}")]
    JobSchedulerError(#[from] AsyncQueueError),

    #[error("Job error {0}")]
    FangError(String),

    #[error("Invalid request {0}")]
    BadRequest(anyhow::Error),

    /// Catch all for error we don't care to expose publicly.
    #[error("Internal error")]
    Anyhow(#[from] anyhow::Error),
}

impl From<FangError> for AppError {
    fn from(err: FangError) -> Self {
        AppError::FangError(err.description)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("Internal server error: {self:?}");

        let status_code = match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status_code, self.to_string()).into_response()
    }
}
