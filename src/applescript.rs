//! Low-level wrappers around `utmctl` and `osascript`.

use crate::{Error, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

// ---------------------------------------------------------------------------
// utmctl
// ---------------------------------------------------------------------------

/// Run `utmctl <args>` and return trimmed stdout.  Propagates stderr on failure.
pub async fn utmctl(args: &[&str]) -> Result<String> {
    let output = Command::new("utmctl")
        .args(args)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::BinaryNotFound("utmctl".into())
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
        Err(Error::Utmctl(format!(
            "utmctl {} failed: {}",
            args.join(" "),
            msg
        )))
    }
}

/// Run `utmctl <args>` and return (stdout, stderr) regardless of exit code.
/// Useful for commands where non-zero exit is expected in some cases.
pub async fn utmctl_raw(args: &[&str]) -> Result<(String, String, bool)> {
    let output = Command::new("utmctl")
        .args(args)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::BinaryNotFound("utmctl".into())
            } else {
                Error::Io(e)
            }
        })?;

    Ok((
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
        output.status.success(),
    ))
}

// ---------------------------------------------------------------------------
// osascript
// ---------------------------------------------------------------------------

/// Run an AppleScript program string and return trimmed stdout.
pub async fn osascript(script: &str) -> Result<String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::BinaryNotFound("osascript".into())
            } else {
                Error::Io(e)
            }
        })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(Error::AppleScript(stderr))
    }
}

/// Run a multi-line AppleScript program via stdin (`osascript -`).
///
/// This is the correct way to execute multi-line scripts that contain
/// `tell` blocks or multi-line record literals.  `osascript -e` flattens
/// newlines and can cause -1700 coercion errors with UTM.
pub async fn osascript_stdin(script: &str) -> Result<String> {
    let mut child = Command::new("osascript")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(Error::Io)?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(Error::Io)?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(Error::Io)?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(Error::AppleScript(stderr))
    }
}

/// Run a multi-line AppleScript program (joins lines with `\n` and passes via stdin-like arg).
pub async fn osascript_multiline(lines: &[&str]) -> Result<String> {
    // osascript supports multiple -e flags, one per line
    let mut cmd = Command::new("osascript");
    for line in lines {
        cmd.arg("-e").arg(line);
    }
    let output = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::BinaryNotFound("osascript".into())
        } else {
            Error::Io(e)
        }
    })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(Error::AppleScript(stderr))
    }
}

// ---------------------------------------------------------------------------
// qemu-img
// ---------------------------------------------------------------------------

/// Create a qcow2 disk image at `path` with `size_gb` gigabytes.
pub async fn qemu_img_create(path: &std::path::Path, size_gb: u32) -> Result<()> {
    let size_arg = format!("{size_gb}G");
    let output = Command::new("qemu-img")
        .args(["create", "-f", "qcow2"])
        .arg(path)
        .arg(&size_arg)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::BinaryNotFound("qemu-img".into())
            } else {
                Error::Io(e)
            }
        })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(Error::Other(format!("qemu-img create failed: {stderr}")))
    }
}