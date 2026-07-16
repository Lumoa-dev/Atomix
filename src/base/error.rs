//! Unified error types for all Atomix crates.

use std::fmt;

// ─── Error Codes ──────────────────────────────────────────────────

/// Numeric error codes as defined in ATXP §7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ErrorCode {
    Ok = 0,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    NotSupported = 405,
    Timeout = 408,
    Conflict = 409,
    VersionMismatch = 412,
    PayloadTooLarge = 413,
    ChecksumMismatch = 415,
    TooManyRequests = 429,
    InternalError = 500,
    Busy = 503,
    NotConnected = 504,
}

impl ErrorCode {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Ok,
            400 => Self::BadRequest,
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            404 => Self::NotFound,
            405 => Self::NotSupported,
            408 => Self::Timeout,
            409 => Self::Conflict,
            412 => Self::VersionMismatch,
            413 => Self::PayloadTooLarge,
            415 => Self::ChecksumMismatch,
            429 => Self::TooManyRequests,
            500 => Self::InternalError,
            503 => Self::Busy,
            504 => Self::NotConnected,
            _ => Self::InternalError,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::BadRequest => "BAD_REQUEST",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::NotSupported => "NOT_SUPPORTED",
            Self::Timeout => "TIMEOUT",
            Self::Conflict => "CONFLICT",
            Self::VersionMismatch => "VERSION_MISMATCH",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::ChecksumMismatch => "CHECKSUM_MISMATCH",
            Self::TooManyRequests => "TOO_MANY_REQUESTS",
            Self::InternalError => "INTERNAL_ERROR",
            Self::Busy => "BUSY",
            Self::NotConnected => "NOT_CONNECTED",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ─── Atomix Error ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AtomixError {
    pub code: ErrorCode,
    pub message: String,
    pub endpoint: Option<String>,
}

impl AtomixError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            endpoint: None,
        }
    }

    pub fn with_endpoint(mut self, ep: impl Into<String>) -> Self {
        self.endpoint = Some(ep.into());
        self
    }

    // ── Common constructors ──

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }

    pub fn not_supported(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotSupported, msg)
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, msg)
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, msg)
    }

    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Timeout, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, msg)
    }

    pub fn checksum(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ChecksumMismatch, msg)
    }

    pub fn payload_too_large(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::PayloadTooLarge, msg)
    }
}

impl fmt::Display for AtomixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.endpoint {
            Some(ep) => write!(f, "[{}] {} (endpoint: {})", self.code, self.message, ep),
            None => write!(f, "[{}] {}", self.code, self.message),
        }
    }
}

impl std::error::Error for AtomixError {}

// ─── Type alias ───────────────────────────────────────────────────

pub type Result<T> = std::result::Result<T, AtomixError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = AtomixError::not_found("task abc123 not found").with_endpoint("tasks/abc123");
        assert!(e.to_string().contains("NOT_FOUND"));
        assert!(e.to_string().contains("abc123"));
    }

    #[test]
    fn code_roundtrip() {
        let defined: &[u32] = &[0, 400, 401, 403, 404, 405, 408, 409, 412, 413, 415, 429, 500, 503, 504];
        for &code in defined {
            let c = ErrorCode::from_u32(code);
            assert_eq!(c as u32, code);
        }
    }
}
