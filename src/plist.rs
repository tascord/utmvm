//! Typed wrappers around `/usr/libexec/PlistBuddy` for editing UTM `config.plist` files.

use crate::{Error, Result};
use std::path::Path;
use tokio::process::Command;

const PLIST_BUDDY: &str = "/usr/libexec/PlistBuddy";

// ---------------------------------------------------------------------------
// Raw PlistBuddy runner
// ---------------------------------------------------------------------------

/// Run a PlistBuddy command string against `plist_path`.
pub async fn run(plist_path: &Path, command: &str) -> Result<String> {
    let output = Command::new(PLIST_BUDDY)
        .arg("-c")
        .arg(command)
        .arg(plist_path)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::BinaryNotFound(PLIST_BUDDY.into())
            } else {
                Error::Io(e)
            }
        })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let msg = if stderr.is_empty() { stdout } else { stderr };
        Err(Error::PlistBuddy(format!("{command}: {msg}")))
    }
}

/// Run a PlistBuddy command, ignoring errors (useful for `Add` when key may already exist).
pub async fn run_ignore_err(plist_path: &Path, command: &str) -> Result<()> {
    let _ = run(plist_path, command).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Typed helpers
// ---------------------------------------------------------------------------

/// Read a string value at `key_path` (e.g. `":Network:0:MacAddress"`).
pub async fn get_string(plist_path: &Path, key_path: &str) -> Result<String> {
    run(plist_path, &format!("Print {key_path}")).await
}

/// Set a string value.
pub async fn set_string(plist_path: &Path, key_path: &str, value: &str) -> Result<()> {
    run(plist_path, &format!("Set {key_path} {value}"))
        .await
        .map(|_| ())
}

/// Set an integer value.
pub async fn set_integer(plist_path: &Path, key_path: &str, value: i64) -> Result<()> {
    run(plist_path, &format!("Set {key_path} {value}"))
        .await
        .map(|_| ())
}

/// Add a new string key.  Fails if it already exists — use [`set_string`] to update.
pub async fn add_string(plist_path: &Path, key_path: &str, value: &str) -> Result<()> {
    run(plist_path, &format!("Add {key_path} string {value}"))
        .await
        .map(|_| ())
}

/// Add a new integer key.
pub async fn add_integer(plist_path: &Path, key_path: &str, value: i64) -> Result<()> {
    run(plist_path, &format!("Add {key_path} integer {value}"))
        .await
        .map(|_| ())
}

/// Add an empty array at `key_path` (ignores error if already present).
pub async fn ensure_array(plist_path: &Path, key_path: &str) -> Result<()> {
    run_ignore_err(plist_path, &format!("Add {key_path} array")).await
}

/// Add an empty dict at `key_path`.
pub async fn add_dict(plist_path: &Path, key_path: &str) -> Result<()> {
    run(plist_path, &format!("Add {key_path} dict"))
        .await
        .map(|_| ())
}

/// Delete a key (and its subtree).
pub async fn delete(plist_path: &Path, key_path: &str) -> Result<()> {
    run(plist_path, &format!("Delete {key_path}"))
        .await
        .map(|_| ())
}

/// Delete a key, ignoring errors (key may not exist).
pub async fn delete_ignore_err(plist_path: &Path, key_path: &str) -> Result<()> {
    run_ignore_err(plist_path, &format!("Delete {key_path}")).await
}

// ---------------------------------------------------------------------------
// UTM-specific helpers
// ---------------------------------------------------------------------------

/// Read the MAC address of network interface at `index`.
pub async fn get_mac_address(plist_path: &Path, index: usize) -> Result<String> {
    get_string(plist_path, &format!(":Network:{index}:MacAddress")).await
}

/// Overwrite the MAC address of network interface at `index`.
pub async fn set_mac_address(plist_path: &Path, index: usize, mac: &str) -> Result<()> {
    set_string(plist_path, &format!(":Network:{index}:MacAddress"), mac).await
}

/// Configure a serial port in TCP server mode at `tcp_port`.
///
/// Idempotent — creates the `:Serial` array if it doesn't exist.
///
/// **Important:** UTM must be quit and restarted for this to take effect.
pub async fn configure_serial_tcp(plist_path: &Path, index: usize, tcp_port: u16) -> Result<()> {
    ensure_array(plist_path, ":Serial").await?;
    run_ignore_err(plist_path, &format!("Add :Serial:{index} dict")).await?;
    // Mode is case-sensitive — must be "TcpServer"
    let mode_cmd = format!("Set :Serial:{index}:Mode TcpServer");
    if run(plist_path, &mode_cmd).await.is_err() {
        run(
            plist_path,
            &format!("Add :Serial:{index}:Mode string TcpServer"),
        )
        .await?;
    }
    // Port key is "TcpPort" (not "TCPPort")
    let port_cmd = format!("Set :Serial:{index}:TcpPort {tcp_port}");
    if run(plist_path, &port_cmd).await.is_err() {
        run(
            plist_path,
            &format!("Add :Serial:{index}:TcpPort integer {tcp_port}"),
        )
        .await?;
    }
    // Target must be "Auto"
    let target_cmd = format!("Set :Serial:{index}:Target Auto");
    if run(plist_path, &target_cmd).await.is_err() {
        run(
            plist_path,
            &format!("Add :Serial:{index}:Target string Auto"),
        )
        .await?;
    }
    Ok(())
}

/// Remove the CD-ROM drive entry from the `Drive` array.
///
/// Iterates drive entries and deletes the first one where `ImageType` is `CD`.
/// Returns `Ok(true)` if a CD-ROM was found and removed, `Ok(false)` if none was found.
///
/// **Important:** UTM must be quit and restarted for this to take effect.
pub async fn remove_cdrom_drive(plist_path: &Path) -> Result<bool> {
    // Count drives
    for i in 0..16 {
        let key = format!(":Drive:{i}:ImageType");
        match get_string(plist_path, &key).await {
            Ok(v) if v.trim() == "CD" => {
                delete(plist_path, &format!(":Drive:{i}")).await?;
                return Ok(true);
            }
            Ok(_) => continue,
            Err(_) => break, // index out of range → no more drives
        }
    }
    Ok(false)
}

/// Add a port-forward rule to network interface `index` in `config.plist`.
///
/// `protocol` should be `"TCP"` or `"UDP"`.
///
/// **Important:** UTM must be quit and restarted for this to take effect.
pub async fn add_port_forward(
    plist_path: &Path,
    index: usize,
    protocol: &str,
    host_port: u16,
    guest_port: u16,
) -> Result<()> {
    // Ensure the PortForward array exists.
    run_ignore_err(
        plist_path,
        &format!("Add :Network:{index}:PortForward array"),
    )
    .await?;

    let pf_idx = 0_usize;
    run_ignore_err(
        plist_path,
        &format!("Add :Network:{index}:PortForward:{pf_idx} dict"),
    )
    .await?;

    let protocol_cmd = format!("Set :Network:{index}:PortForward:{pf_idx}:Protocol {protocol}");
    if run(plist_path, &protocol_cmd).await.is_err() {
        run(
            plist_path,
            &format!("Add :Network:{index}:PortForward:{pf_idx}:Protocol string {protocol}"),
        )
        .await?;
    }

    let host_port_cmd = format!("Set :Network:{index}:PortForward:{pf_idx}:HostPort {host_port}");
    if run(plist_path, &host_port_cmd).await.is_err() {
        run(
            plist_path,
            &format!("Add :Network:{index}:PortForward:{pf_idx}:HostPort integer {host_port}"),
        )
        .await?;
    }

    let guest_port_cmd = format!("Set :Network:{index}:PortForward:{pf_idx}:GuestPort {guest_port}");
    if run(plist_path, &guest_port_cmd).await.is_err() {
        run(
            plist_path,
            &format!("Add :Network:{index}:PortForward:{pf_idx}:GuestPort integer {guest_port}"),
        )
        .await?;
    }

    Ok(())
}

/// Append extra QEMU command-line arguments to `config.plist`.
///
/// **Important:** UTM must be quit and restarted for this to take effect.
pub async fn add_qemu_additional_args(plist_path: &Path, args: &[&str]) -> Result<()> {
    // Ensure the array exists.
    run_ignore_err(plist_path, "Add :QEMU:AdditionalArguments array").await?;

    for arg in args {
        // PlistBuddy doesn't have an "append" operation.  Find the next free
        // index by probing until we get an error (index out of range).
        let mut idx = 0;
        loop {
            let test_cmd = format!("Print :QEMU:AdditionalArguments:{idx}");
            if run(plist_path, &test_cmd).await.is_err() {
                // idx is free — add the argument here.
                run(
                    plist_path,
                    &format!(
                        "Add :QEMU:AdditionalArguments:{idx} string {arg}"
                    ),
                )
                .await?;
                break;
            }
            idx += 1;
        }
    }

    Ok(())
}

/// Validate the plist file using `plutil -lint`.
pub async fn validate(plist_path: &Path) -> Result<()> {
    let output = tokio::process::Command::new("plutil")
        .arg("-lint")
        .arg(plist_path)
        .output()
        .await
        .map_err(Error::Io)?;

    if output.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(Error::PlistBuddy(format!("plist validation failed: {msg}")))
    }
}