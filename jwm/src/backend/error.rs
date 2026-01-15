// src/backend/error.rs
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum BackendError {
    Unsupported(&'static str),
    NotFound(&'static str),
    Message(String),
    Other(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Unsupported(s) => write!(f, "Unsupported: {s}"),
            BackendError::NotFound(s) => write!(f, "Not found: {s}"),
            BackendError::Message(s) => write!(f, "Error: {s}"),
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

// === 标准库与基础类型 ===

impl From<std::io::Error> for BackendError {
    fn from(e: std::io::Error) -> Self {
        BackendError::Other(Box::new(e))
    }
}

impl From<String> for BackendError {
    fn from(s: String) -> Self {
        BackendError::Message(s)
    }
}

impl From<&'static str> for BackendError {
    fn from(s: &'static str) -> Self {
        BackendError::Message(s.to_string())
    }
}

impl From<Box<dyn Error + Send + Sync>> for BackendError {
    fn from(e: Box<dyn Error + Send + Sync>) -> Self {
        BackendError::Other(e)
    }
}

// === Calloop ===

impl From<calloop::Error> for BackendError {
    fn from(e: calloop::Error) -> Self {
        BackendError::Other(Box::new(e))
    }
}

// === X11RB ===

// 连接建立时的错误
impl From<x11rb::rust_connection::ConnectError> for BackendError {
    fn from(e: x11rb::rust_connection::ConnectError) -> Self {
        BackendError::Other(Box::new(e))
    }
}

// 连接运行时的错误 (只保留这一个 ConnectionError 实现以避免冲突)
impl From<x11rb::rust_connection::ConnectionError> for BackendError {
    fn from(e: x11rb::rust_connection::ConnectionError) -> Self {
        BackendError::Other(Box::new(e))
    }
}

// Reply 错误
impl From<x11rb::errors::ReplyError> for BackendError {
    fn from(e: x11rb::errors::ReplyError) -> Self {
        BackendError::Other(Box::new(e))
    }
}

// ReplyOrId 错误
impl From<x11rb::errors::ReplyOrIdError> for BackendError {
    fn from(e: x11rb::errors::ReplyOrIdError) -> Self {
        BackendError::Other(Box::new(e))
    }
}
