#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("base de données : {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json : {0}")]
    Json(#[from] serde_json::Error),
    #[error("e/s : {0}")]
    Io(#[from] std::io::Error),
    #[error("introuvable : {0}")]
    NotFound(String),
    #[error("invalide : {0}")]
    Invalid(String),
}
