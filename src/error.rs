use thiserror::Error;

#[derive(Error, Debug)]
pub enum GlobalError {
    #[error("invalid config {path:?} {value:?}")]
    InvalidConfig { path: String, value: String },
    #[error("unknown error")]
    Unknown,
}
