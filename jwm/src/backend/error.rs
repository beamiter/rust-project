// src/backend/error.rs
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum BackendError {
    Unsupported(&'static str),
    NotFound(&'static str),
    Other(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Unsupported(s) => write!(f, "Unsupported: {s}"),
            BackendError::NotFound(s) => write!(f, "Not found: {s}"),
            BackendError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl Error for BackendError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            BackendError::Other(e) => Some(&**e),
            _ => None,
        }
    }
}

impl<E> From<E> for BackendError
where
    E: Error + Send + Sync + 'static,
{
    fn from(e: E) -> Self {
        BackendError::Other(Box::new(e))
    }
}
