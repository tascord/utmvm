use thiserror::Error;

/// All errors produced by this library.
#[derive(Debug, Error)]
pub enum Error {
    /// `utmctl` exited with a non-zero status or produced unexpected output.
    #[error("utmctl error: {0}")]
    Utmctl(String),

    /// `osascript` failed.
    #[error("osascript error: {0}")]
    AppleScript(String),

    /// `PlistBuddy` failed.
    #[error("PlistBuddy error: {0}")]
    PlistBuddy(String),

    /// A required binary (`utmctl`, `osascript`, `PlistBuddy`, `qemu-img`) was not found.
    #[error("required binary not found: {0}")]
    BinaryNotFound(String),

    /// The requested VM does not exist.
    #[error("VM not found: {0}")]
    VmNotFound(String),

    /// A VM with that name already exists.
    #[error("VM already exists: {0}")]
    VmAlreadyExists(String),

    /// The VM's `config.plist` is missing or unreadable.
    #[error("config.plist not found for VM '{0}': {1}")]
    ConfigNotFound(String, String),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parse error (for utmctl structured output).
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// A timeout waiting for the VM or UTM to reach the expected state.
    #[error("timeout: {0}")]
    Timeout(String),

    /// The QEMU guest agent is not running or not installed in the VM.
    #[error("QEMU guest agent unavailable in VM '{0}'")]
    GuestAgentUnavailable(String),

    /// Generic error for anything else.
    #[error("{0}")]
    Other(String),
}