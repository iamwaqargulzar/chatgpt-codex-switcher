use serde::Serialize;

/// Single error type surfaced to the frontend as `{ message: string }`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("url: {0}")]
    Url(#[from] url::ParseError),
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type R<T> = Result<T, AppError>;

pub fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(AppError::Msg(msg.into()))
}
