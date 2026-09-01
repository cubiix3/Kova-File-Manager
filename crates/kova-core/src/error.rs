/// Core error type for Kova filesystem and shell operations.
///
/// Preserves the originating OS or source error where applicable so the UI can
/// present human-readable text while logs retain technical details.
#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    #[error("path not found: {path}")]
    NotFound {
        path: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("permission denied: {path}")]
    PermissionDenied {
        path: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("already exists: {path}")]
    AlreadyExists {
        path: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("invalid path: {path}")]
    InvalidPath {
        path: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("unsupported operation: {operation}")]
    Unsupported {
        operation: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("device unavailable: {path}")]
    DeviceUnavailable {
        path: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("network unavailable: {path}")]
    NetworkUnavailable {
        path: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("operation cancelled")]
    OperationCancelled,

    #[error("shell error: {0}")]
    Shell(String),

    #[error("I/O error: {context}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}

impl OperationError {
    pub fn not_found(path: impl Into<String>, source: Option<std::io::Error>) -> Self {
        Self::NotFound {
            path: path.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    pub fn permission_denied(path: impl Into<String>, source: Option<std::io::Error>) -> Self {
        Self::PermissionDenied {
            path: path.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    pub fn already_exists(path: impl Into<String>, source: Option<std::io::Error>) -> Self {
        Self::AlreadyExists {
            path: path.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    pub fn invalid_path(path: impl Into<String>, source: Option<std::io::Error>) -> Self {
        Self::InvalidPath {
            path: path.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    pub fn device_unavailable(path: impl Into<String>, source: Option<std::io::Error>) -> Self {
        Self::DeviceUnavailable {
            path: path.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    pub fn network_unavailable(path: impl Into<String>, source: Option<std::io::Error>) -> Self {
        Self::NetworkUnavailable {
            path: path.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub fn path(&self) -> Option<&str> {
        match self {
            Self::NotFound { path, .. } => Some(path),
            Self::PermissionDenied { path, .. } => Some(path),
            Self::AlreadyExists { path, .. } => Some(path),
            Self::InvalidPath { path, .. } => Some(path),
            Self::DeviceUnavailable { path, .. } => Some(path),
            Self::NetworkUnavailable { path, .. } => Some(path),
            _ => None,
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::PermissionDenied { .. })
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::OperationCancelled)
    }
}

pub type Result<T> = std::result::Result<T, OperationError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoResultKind {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidInput,
    DeviceNotReady,
    NetworkPathNotFound,
    Other,
}

impl IoResultKind {
    pub fn from_io_error_kind(kind: std::io::ErrorKind) -> Self {
        match kind {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::AlreadyExists => Self::AlreadyExists,
            std::io::ErrorKind::InvalidInput => Self::InvalidInput,
            _ => Self::Other,
        }
    }

    pub fn classify_windows_error(raw: i32) -> Self {
        // Common Win32 error codes observed on Windows
        match raw {
            2 => Self::NotFound,         // ERROR_FILE_NOT_FOUND
            3 => Self::NotFound,         // ERROR_PATH_NOT_FOUND
            5 => Self::PermissionDenied, // ERROR_ACCESS_DENIED
            80 => Self::AlreadyExists,   // ERROR_FILE_EXISTS
            183 => Self::AlreadyExists,  // ERROR_ALREADY_EXISTS
            21 => Self::DeviceNotReady,  // ERROR_NOT_READY
            53 | 67 => Self::NetworkPathNotFound,
            87 => Self::InvalidInput, // ERROR_INVALID_PARAMETER
            _ => Self::Other,
        }
    }
}
