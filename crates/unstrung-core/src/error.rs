use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("database migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
    #[error("engine request: {0}")]
    Http(#[from] reqwest::Error),
    #[error("data format: {0}")]
    Json(#[from] serde_json::Error),
    #[error("chat engine: {0}")]
    Engine(String),
    #[error("the chat engine is not ready")]
    NotReady,
    #[error("a reply is already in progress")]
    Busy,
    #[error("no reply after {0} seconds")]
    Timeout(u64),
    #[error("stopped")]
    Cancelled,
    #[error("not enough space: {need} more bytes needed")]
    NoSpace { need: u64 },
    #[error("no internet connection: {0}")]
    Offline(String),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("invalid input: {0}")]
    Invalid(String),
    /// A file that the user picked and that the app cannot use. `message` is
    /// for the user, with what to do. `why` is the technical reason.
    #[error("{message} ({why})")]
    Rejected { message: String, why: String },
    /// The database failed its damage check.
    #[error("damaged data: {0}")]
    Damaged(String),
}
