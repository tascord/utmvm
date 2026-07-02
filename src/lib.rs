//! # utmvm
//!
//! A Rust library for creating and managing UTM VMs on macOS via `utmctl` and `osascript`.
//!
//! ## Overview
//!
//! UTM is a macOS GUI wrapper around QEMU. This library exposes a typed async API over:
//! - `utmctl` — the UTM CLI for VM lifecycle operations
//! - `osascript` — AppleScript for VM creation and app-level management
//! - `PlistBuddy` — plist editing for configuration changes
//!
//! ## Critical UTM behaviours
//!
//! - UTM **caches** `config.plist` in memory at startup. Config edits have no effect
//!   until UTM is quit and restarted — this library handles that automatically where needed.
//! - UEFI NVRAM (`efi_vars.fd`) does **not** persist across UTM restarts. Always use the
//!   removable-media fallback path (`\EFI\BOOT\BOOTAA64.EFI` / `BOOTX64.EFI`).
//! - `utmctl clone` copies the MAC address — always regenerate after cloning.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use utmvm::{UtmManager, VmConfig, Architecture, NetworkMode, DriveConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), utmvm::Error> {
//!     let mgr = UtmManager::new();
//!
//!     // List existing VMs
//!     let vms = mgr.list().await?;
//!     for vm in &vms {
//!         println!("{}: {:?}", vm.name, vm.status);
//!     }
//!
//!     // Clone a template and give the clone a fresh MAC
//!     let clone = mgr.clone_vm("my-template", "my-clone").await?;
//!     println!("Clone ready at {}", clone.name);
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]

pub mod error;
pub mod manager;
pub mod vm;
pub mod config;
pub mod applescript;
pub mod plist;
pub mod network;

pub use error::Error;
pub use manager::UtmManager;
pub use vm::{Vm, VmStatus, VmInfo};
pub use config::{VmConfig, Architecture, CpuCount, MemoryMb, DiskSizeGb, NetworkMode, DriveConfig, DriveInterface, DisplayHardware, PortForward};
pub use network::MacAddress;

/// Convenience `Result` alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;