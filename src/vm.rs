use std::net::IpAddr;
use std::path::PathBuf;

/// Current lifecycle status of a VM as reported by `utmctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmStatus {
    /// VM is powered off.
    Stopped,
    /// VM is running.
    Started,
    /// VM is paused.
    Paused,
    /// VM is in the process of starting.
    Starting,
    /// VM is in the process of stopping.
    Stopping,
    /// Status string not recognised.
    Unknown(String),
}

impl VmStatus {
    pub(crate) fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "stopped" => VmStatus::Stopped,
            "started" => VmStatus::Started,
            "paused" => VmStatus::Paused,
            "starting" => VmStatus::Starting,
            "stopping" => VmStatus::Stopping,
            other => VmStatus::Unknown(other.to_string()),
        }
    }
}

/// Basic information about a VM as listed by `utmctl list`.
#[derive(Debug, Clone)]
pub struct VmInfo {
    /// Display name.
    pub name: String,
    /// UUID assigned by UTM.
    pub uuid: String,
    /// Current status.
    pub status: VmStatus,
}

/// A handle to a specific VM.
///
/// Returned by operations that act on an individual VM.  Cheap to clone.
#[derive(Debug, Clone)]
pub struct Vm {
    /// Display name.
    pub name: String,
    /// UTM-assigned UUID.
    pub uuid: String,
    /// Path to the `.utm` bundle directory.
    pub bundle_path: PathBuf,
    /// Current status (may be stale — call [`UtmManager::status`] to refresh).
    pub status: VmStatus,
    /// IP address, if the QEMU guest agent is running and an IP has been assigned.
    pub ip_address: Option<IpAddr>,
}