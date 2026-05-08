use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sidecar error: {0}")]
    Sidecar(String),

    #[error("sing-box invocation failed: {0}")]
    SingBoxFailed(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("clash api error: {0}")]
    ClashApi(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    /// User toggled TUN ON (or some flow tried to start sing-box with TUN)
    /// but the underlying OS doesn't grant the necessary capability. The
    /// frontend recognises this variant by its serialised shape and routes
    /// it to the privilege dialog instead of a generic toast.
    #[error("TUN requires elevation on {platform}: {hint}")]
    PrivilegeRequired { platform: String, hint: String },

    #[error("{0}")]
    Other(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            AppError::PrivilegeRequired { platform, hint } => {
                // Structured shape so the frontend can dispatch on `kind`.
                // Other variants stay as plain strings to preserve existing
                // toast handling.
                let mut s = ser.serialize_struct("AppError", 3)?;
                s.serialize_field("kind", "privilege_required")?;
                s.serialize_field("platform", platform)?;
                s.serialize_field("hint", hint)?;
                s.end()
            }
            _ => ser.serialize_str(&self.to_string()),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
