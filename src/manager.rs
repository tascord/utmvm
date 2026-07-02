//! [`UtmManager`] — the main entry point for all UTM VM operations.

use crate::{
    applescript::{osascript, osascript_stdin, utmctl, utmctl_raw},
    config::VmConfig,
    network::MacAddress,
    plist,
    vm::{Vm, VmInfo, VmStatus},
    Error, Result,
};
use std::{
    net::IpAddr,
    path::PathBuf,
    time::Duration,
};
use tokio::fs;
use tokio::time::sleep;
use uuid::Uuid;

/// Default location for UTM VM bundles.
fn utm_docs_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/unknown".into());
    PathBuf::from(home)
        .join("Library/Containers/com.utmapp.UTM/Data/Documents")
}

/// The main interface to UTM.
///
/// All methods are `async` and require a Tokio runtime.
///
/// # Notes on UTM configuration caching
///
/// UTM loads `config.plist` **once** at startup.  Any method that modifies plist files
/// will automatically quit and restart UTM unless you opt out via the lower-level helpers.
/// Operations that only call `utmctl` (start, stop, list, ip-address) do **not** restart UTM.
pub struct UtmManager {
    /// Override for the documents directory (useful in tests).
    docs_dir: PathBuf,
}

impl UtmManager {
    /// Create a manager using the default UTM documents directory.
    pub fn new() -> Self {
        Self {
            docs_dir: utm_docs_dir(),
        }
    }

    /// Create a manager pointing at a custom documents directory (e.g. for testing).
    pub fn with_docs_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            docs_dir: dir.into(),
        }
    }

    // -----------------------------------------------------------------------
    // Paths
    // -----------------------------------------------------------------------

    /// Return the path to a VM's `.utm` bundle.
    pub fn bundle_path(&self, name: &str) -> PathBuf {
        self.docs_dir.join(format!("{name}.utm"))
    }

    /// Return the path to a VM's `config.plist`.
    pub fn config_path(&self, name: &str) -> PathBuf {
        self.bundle_path(name).join("config.plist")
    }

    fn config_path_checked(&self, name: &str) -> Result<PathBuf> {
        let p = self.config_path(name);
        if p.exists() {
            Ok(p)
        } else {
            Err(Error::ConfigNotFound(
                name.to_string(),
                p.display().to_string(),
            ))
        }
    }

    // -----------------------------------------------------------------------
    // UTM app lifecycle
    // -----------------------------------------------------------------------

    /// Quit the UTM app gracefully.
    ///
    /// Required before any `config.plist` modifications take effect.
    pub async fn quit_utm(&self) -> Result<()> {
        osascript("quit app \"UTM\"").await?;
        // UTM needs generous time to flush in-memory state (VM plists,
        // drive caches) before we start editing files externally.
        sleep(Duration::from_secs(8)).await;
        Ok(())
    }

    /// Launch the UTM app and wait for it to be ready.
    pub async fn launch_utm(&self) -> Result<()> {
        let output = tokio::process::Command::new("open")
            .args(["-a", "UTM"])
            .output()
            .await
            .map_err(Error::Io)?;
        if !output.status.success() {
            let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(Error::Other(format!("failed to launch UTM: {msg}")));
        }
        // Delay is critical — UTM must finish loading all VM bundles and
        // setting up QEMU configurations before we issue further commands.
        sleep(Duration::from_secs(8)).await;
        Ok(())
    }

    /// Quit UTM, run `f`, then relaunch UTM.
    ///
    /// Use this wrapper for any sequence of `config.plist` edits.
    ///
    /// ```rust,no_run
    /// # use utmvm::UtmManager;
    /// # async fn example(mgr: &UtmManager, name: &str) -> utmvm::Result<()> {
    /// mgr.with_utm_restart(|mgr| {
    ///     Box::pin(async move {
    ///         // plist edits here …
    ///         Ok(())
    ///     })
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn with_utm_restart<F, Fut>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&UtmManager) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        self.quit_utm().await?;
        let result = f(self).await;
        self.launch_utm().await?;
        result
    }

    // -----------------------------------------------------------------------
    // VM listing and inspection
    // -----------------------------------------------------------------------

    /// List all VMs known to UTM.
    pub async fn list(&self) -> Result<Vec<VmInfo>> {
        let raw = utmctl(&["list"]).await?;
        Ok(parse_list_output(&raw))
    }

    /// Return `true` if a VM with `name` exists.
    pub async fn exists(&self, name: &str) -> Result<bool> {
        let vms = self.list().await?;
        Ok(vms.iter().any(|v| v.name == name))
    }

    /// Return the current [`VmStatus`] of `name`.
    pub async fn status(&self, name: &str) -> Result<VmStatus> {
        let raw = utmctl(&["status", name]).await?;
        Ok(VmStatus::from_str(&raw))
    }

    /// Return a [`Vm`] handle for `name`, populating the IP address if the guest agent is running.
    pub async fn get(&self, name: &str) -> Result<Vm> {
        let info = self
            .list()
            .await?
            .into_iter()
            .find(|v| v.name == name)
            .ok_or_else(|| Error::VmNotFound(name.to_string()))?;

        let ip = self.ip_address(name).await.ok();
        Ok(Vm {
            name: info.name,
            uuid: info.uuid,
            bundle_path: self.bundle_path(name),
            status: info.status,
            ip_address: ip,
        })
    }

    // -----------------------------------------------------------------------
    // VM lifecycle
    // -----------------------------------------------------------------------

    /// Start a stopped VM.
    pub async fn start(&self, name: &str) -> Result<()> {
        utmctl(&["start", name]).await.map(|_| ())
    }

    /// Stop a running VM.
    pub async fn stop(&self, name: &str) -> Result<()> {
        utmctl(&["stop", name]).await.map(|_| ())
    }

    /// Suspend (pause) a running VM.
    pub async fn suspend(&self, name: &str) -> Result<()> {
        utmctl(&["suspend", name]).await.map(|_| ())
    }

    /// Delete a VM (stops it first if running).  Permanently removes the `.utm` bundle.
    pub async fn delete(&self, name: &str) -> Result<()> {
        // Stop it first; ignore errors if already stopped.
        let _ = self.stop(name).await;
        sleep(Duration::from_secs(2)).await;
        utmctl(&["delete", name]).await.map(|_| ())
    }

    // -----------------------------------------------------------------------
    // VM creation
    // -----------------------------------------------------------------------

    /// After AppleScript creates a VM, CD-ROM drives lack `ImageName` in the
    /// plist.  Copy the ISO into the VM bundle's `Data/` directory and set the
    /// `ImageName` key so UTM can find the media on restart.
    async fn fixup_cdrom_images(&self, config: &VmConfig) -> Result<()> {
        let bundle = self.bundle_path(&config.name);
        if !bundle.exists() {
            return Err(Error::ConfigNotFound(
                config.name.clone(),
                bundle.display().to_string(),
            ));
        }

        let data_dir = bundle.join("Data");
        tokio::fs::create_dir_all(&data_dir).await.map_err(Error::Io)?;
        let plist = self.config_path(&config.name);

        for drive in &config.drives {
            let Some(iso) = &drive.iso_path else { continue };
            let Some(filename) = iso.file_name() else {
                return Err(Error::Other(format!(
                    "invalid ISO path for VM '{}': {}",
                    config.name,
                    iso.display()
                )));
            };

            // Copy ISO into the bundle so the sandboxed UTM can always find it.
            let dest = data_dir.join(filename);
            fs::copy(iso, &dest).await.map_err(Error::Io)?;

            // Find the drive entry and set ImageName.
            for i in 0..16 {
                let key = format!(":Drive:{i}:ImageType");
                match plist::get_string(&plist, &key).await {
                    Ok(v) if v.trim() == "CD" => {
                        let img_name = filename.to_string_lossy();
                        let set_cmd = format!(
                            "Set :Drive:{i}:ImageName {img_name}"
                        );
                        let _ = plist::run(&plist, &set_cmd).await;
                        // If Set fails (key doesn't exist), try Add.
                        let add_cmd = format!(
                            "Add :Drive:{i}:ImageName string {img_name}"
                        );
                        let _ = plist::run(&plist, &add_cmd).await;
                        break;
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }

        Ok(())
    }

    /// Create a new VM using UTM's AppleScript API.
    ///
    /// This is the recommended approach for programmatic VM creation; plist-only creation
    /// is unreliable for boot order.  UTM must be running when this is called.
    ///
    /// After creation the VM is **not** started automatically.

    /// Create a new VM using UTM's AppleScript API.
    ///
    /// This is the recommended approach for programmatic VM creation; plist-only creation
    /// is unreliable for boot order.  UTM must be running when this is called.
    ///
    /// After creation the VM is **not** started automatically.
    pub async fn create(&self, config: &VmConfig) -> Result<()> {
        if self.exists(&config.name).await? {
            return Err(Error::VmAlreadyExists(config.name.clone()));
        }

        // AppleScript creation with non-empty `port forwards:` in emulated mode
        // triggers a -1700 coercion error, so we create the VM without port
        // forwards and add them via plist in a UTM restart afterwards.
        let mut script_config = config.clone();
        script_config.port_forwards.clear();
        let script = build_create_script(&script_config);
        osascript_stdin(&script).await?;

        // Give UTM time to finish writing the new VM's plist before we
        // start editing files externally.
        sleep(Duration::from_secs(5)).await;

        // AppleScript one-shot creation does not set ImageName for CD-ROM
        // drives.  Copy the ISO into the bundle and patch the plist so UTM
        // can find the media.
        self.fixup_cdrom_images(config).await?;

        // Add port-forwards through plist if they were requested.
        if !config.port_forwards.is_empty() {
            let config_path = self.config_path(&config.name);
            let pf = config.port_forwards[0].clone();
            self.with_utm_restart(|_| {
                let path = config_path.clone();
                Box::pin(async move {
                    plist::add_port_forward(
                        &path,
                        0,
                        &pf.protocol,
                        pf.host_port,
                        pf.guest_port,
                    )
                    .await
                })
            })
            .await?;
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Cloning
    // -----------------------------------------------------------------------

    /// Clone `source` to `new_name`, generating a fresh MAC address for the clone.
    ///
    /// This method:
    /// 1. Calls `utmctl clone`
    /// 2. Quits UTM
    /// 3. Updates the MAC address in `config.plist`
    /// 4. Relaunches UTM
    ///
    /// The clone is **not** started automatically.
    pub async fn clone_vm(&self, source: &str, new_name: &str) -> Result<Vm> {
        if !self.exists(source).await? {
            return Err(Error::VmNotFound(source.to_string()));
        }
        if self.exists(new_name).await? {
            return Err(Error::VmAlreadyExists(new_name.to_string()));
        }

        utmctl(&["clone", source, new_name]).await?;

        let new_mac = MacAddress::random_qemu();
        let new_mac_str = new_mac.as_str();
        let config_path = self.config_path(new_name);

        self.with_utm_restart(|_mgr| {
            let path = config_path.clone();
            let mac = new_mac_str.clone();
            Box::pin(async move {
                plist::set_mac_address(&path, 0, &mac).await
            })
        })
        .await?;

        self.get(new_name).await
    }

    // -----------------------------------------------------------------------
    // Configuration
    // -----------------------------------------------------------------------

    /// Configure a serial console in TCP server mode on `tcp_port`.
    ///
    /// Quits and relaunches UTM so the change takes effect.
    pub async fn configure_serial_tcp(&self, name: &str, tcp_port: u16) -> Result<()> {
        let config_path = self.config_path_checked(name)?;
        self.with_utm_restart(|_| {
            let path = config_path.clone();
            Box::pin(async move { plist::configure_serial_tcp(&path, 0, tcp_port).await })
        })
        .await
    }

    /// Remove the CD-ROM drive from a VM's configuration (post-installation).
    ///
    /// Quits and relaunches UTM so the change takes effect.
    /// Returns `Ok(true)` if a CD-ROM was found and removed.
    pub async fn remove_cdrom(&self, name: &str) -> Result<bool> {
        let config_path = self.config_path_checked(name)?;
        let mut removed = false;
        self.with_utm_restart(|_| {
            let path = config_path.clone();
            Box::pin(async move {
                let r = plist::remove_cdrom_drive(&path).await?;
                removed = r;
                Ok(())
            })
        })
        .await?;
        Ok(removed)
    }

    /// Update the MAC address of network interface `index` on `name`.
    ///
    /// Quits and relaunches UTM.
    pub async fn set_mac_address(&self, name: &str, index: usize, mac: &MacAddress) -> Result<()> {
        let config_path = self.config_path_checked(name)?;
        let mac_str = mac.as_str();
        self.with_utm_restart(|_| {
            let path = config_path.clone();
            let m = mac_str.clone();
            Box::pin(async move { plist::set_mac_address(&path, index, &m).await })
        })
        .await
    }

    // -----------------------------------------------------------------------
    // Networking
    // -----------------------------------------------------------------------

    /// Return the IP address of a running VM (requires QEMU guest agent).
    ///
    /// Returns [`Error::GuestAgentUnavailable`] if the agent is not running.
    pub async fn ip_address(&self, name: &str) -> Result<IpAddr> {
        let (stdout, stderr, ok) = utmctl_raw(&["ip-address", name]).await?;
        if !ok || stdout.is_empty() {
            if stderr.to_lowercase().contains("guest agent") || stdout.is_empty() {
                return Err(Error::GuestAgentUnavailable(name.to_string()));
            }
            return Err(Error::Utmctl(format!(
                "ip-address failed: {}",
                if stderr.is_empty() { &stdout } else { &stderr }
            )));
        }
        // utmctl ip-address may return multiple lines; take the first IPv4
        let raw = stdout.lines().next().unwrap_or("").trim().to_string();
        raw.parse::<IpAddr>()
            .map_err(|_| Error::Other(format!("could not parse IP '{raw}'")))
    }

    /// Poll for an IP address up to `timeout`, retrying every `interval`.
    ///
    /// Useful after starting a VM — the guest agent takes a moment to register.
    pub async fn wait_for_ip(
        &self,
        name: &str,
        timeout: Duration,
        interval: Duration,
    ) -> Result<IpAddr> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match self.ip_address(name).await {
                Ok(ip) => return Ok(ip),
                Err(_) if tokio::time::Instant::now() < deadline => {
                    sleep(interval).await;
                }
                Err(_) => {
                    return Err(Error::Timeout(format!(
                        "timed out waiting for IP address of '{name}'"
                    )))
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Waiting helpers
    // -----------------------------------------------------------------------

    /// Wait until a VM reaches `target_status`, polling every `interval` up to `timeout`.
    pub async fn wait_for_status(
        &self,
        name: &str,
        target: VmStatus,
        timeout: Duration,
        interval: Duration,
    ) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let status = self.status(name).await?;
            if status == target {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout(format!(
                    "timed out waiting for VM '{name}' to reach status {target:?}"
                )));
            }
            sleep(interval).await;
        }
    }

    /// Start a VM and wait until it has an IP address (i.e. boot + guest agent ready).
    pub async fn start_and_wait_for_ip(
        &self,
        name: &str,
        timeout: Duration,
    ) -> Result<IpAddr> {
        self.start(name).await?;
        self.wait_for_ip(name, timeout, Duration::from_secs(3)).await
    }
}

impl Default for UtmManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the AppleScript source that `create` sends to `osascript`.
///
/// This is exposed as a pure function so unit tests can assert the exact
/// generated text against known-working templates.
pub(crate) fn build_create_script(config: &VmConfig) -> String {
    let _mac = config
        .mac_address
        .clone()
        .unwrap_or_else(MacAddress::random_qemu);

    let (drive_vars, drives_expr) = build_drives_applescript(&config.drives);

    let notes_line = match &config.notes {
        Some(n) => format!("notes:\"{n}\", "),
        None => String::new(),
    };
    let hypervisor_line = if config.hypervisor { "hypervisor:true, " } else { "" };

    let mut script = String::new();
    script.push_str("tell application \"UTM\"\n");
    if !drive_vars.is_empty() {
        script.push_str(&drive_vars);
    }

    script.push_str("    make new virtual machine with properties {backend:qemu, configuration:{");
    script.push_str(&format!("name:\"{name}\", ", name = config.name));
    script.push_str(&notes_line);
    script.push_str(&format!("architecture:\"{}\", ", config.architecture.as_str()));
    script.push_str(&format!("memory:{}, ", config.memory_mb));
    script.push_str(&format!("cpu cores:{}, ", config.cpu_count));
    script.push_str(hypervisor_line);

    // drives: opens ONE brace for the list. Each drive record already wraps
    // itself in {…}, so we must NOT add an extra pair here.
    script.push_str("drives:{");
    script.push_str(&drives_expr);
    script.push_str("}, ");

    script.push_str("network interfaces:{{");
    if matches!(config.network_mode, crate::config::NetworkMode::Emulated)
        && !config.port_forwards.is_empty()
    {
        let pf = &config.port_forwards[0];
        script.push_str("mode:");
        script.push_str(config.network_mode.as_applescript_str());
        script.push_str(", port forwards:{{protocol:");
        script.push_str(&pf.protocol);
        script.push_str(", host port:");
        script.push_str(&pf.host_port.to_string());
        script.push_str(", guest port:");
        script.push_str(&pf.guest_port.to_string());
        // Close port-forward record, port-forwards list, interface record, interfaces list.
        script.push_str("}}}}");
    } else {
        script.push_str("mode:");
        script.push_str(config.network_mode.as_applescript_str());
        // Close interface record and interfaces list.
        script.push_str("}}");
    }

    // Close configuration record and properties record.
    script.push_str("}}\n");
    script.push_str("end tell\n");

    script
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse the tabular output of `utmctl list`.
///
/// Expected format (tab-separated):
/// ```text
/// Name            UUID                                   Status
/// my-vm           xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx   started
/// ```
fn parse_list_output(raw: &str) -> Vec<VmInfo> {
    raw.lines()
        .skip(1) // header
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            // Split on 2+ whitespace characters
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                // Try splitting on multiple spaces
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() >= 3 {
                    return Some(VmInfo {
                        name: cols[0].to_string(),
                        uuid: cols[1].to_string(),
                        status: VmStatus::from_str(cols[2]),
                    });
                }
                return None;
            }
            Some(VmInfo {
                name: parts[0].trim().to_string(),
                uuid: parts[1].trim().to_string(),
                status: VmStatus::from_str(parts[2]),
            })
        })
        .collect()
}

/// Build the AppleScript drives fragment for a list of drive configs.
///
/// Returns a tuple:
/// - `String` – variable declarations (e.g. `set drive1 to POSIX file "…"\n`)
/// - `String` – the drives expression for the `make` command record.
fn build_drives_applescript(drives: &[crate::config::DriveConfig]) -> (String, String) {
    let mut vars = String::new();
    let mut exprs = Vec::new();

    for (i, d) in drives.iter().enumerate() {
        let var = format!("drive{}", i + 1);
        if let Some(iso) = &d.iso_path {
            vars.push_str(&format!("    set {var} to POSIX file \"{path}\"\n",
                path = iso.display().to_string().replace('"', "\\\"")
            ));
            exprs.push(format!("{{removable:true, source:{var}}}"));
        } else if let Some(img) = &d.image_path {
            vars.push_str(&format!("    set {var} to POSIX file \"{path}\"\n",
                path = img.display().to_string().replace('"', "\\\"")
            ));
            exprs.push(format!("{{source:{var}}}"));
        } else {
            let size_mb = d.size_gb.unwrap_or(20) as u64 * 1024;
            exprs.push(format!("{{guest size:{size_mb}}}"));
        }
    }

    (vars, exprs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_list_tab_separated() {
        let raw = "Name\tUUID\tStatus\nmy-vm\tabc-123\tstarted\nother\tdef-456\tstopped\n";
        let vms = parse_list_output(raw);
        assert_eq!(vms.len(), 2);
        assert_eq!(vms[0].name, "my-vm");
        assert_eq!(vms[0].uuid, "abc-123");
        assert_eq!(vms[0].status, VmStatus::Started);
        assert_eq!(vms[1].status, VmStatus::Stopped);
    }

    #[test]
    fn test_parse_list_whitespace_separated() {
        let raw = "Name                UUID                                   Status\nmy-vm               abc-123                                started\n";
        let vms = parse_list_output(raw);
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].name, "my-vm");
    }

    #[test]
    fn test_build_drives_applescript_cdrom() {
        use crate::config::DriveConfig;
        let drives = vec![DriveConfig::cdrom("/path/to/installer.iso")];
        let (vars, expr) = build_drives_applescript(&drives);
        assert!(expr.contains("removable:true"));
        assert!(expr.contains("source:drive1"));
        assert!(vars.contains("set drive1 to POSIX file \"/path/to/installer.iso\""));
    }

    #[test]
    fn test_build_drives_applescript_disk() {
        use crate::config::DriveConfig;
        let drives = vec![DriveConfig::disk(20)];
        let (vars, expr) = build_drives_applescript(&drives);
        assert!(vars.is_empty());
        assert!(expr.contains("guest size:20480"));
    }

    #[test]
    fn test_build_drives_applescript_disk_image() {
        use crate::config::DriveConfig;
        let drives = vec![DriveConfig::disk_image("/path/to/disk.img")];
        let (vars, expr) = build_drives_applescript(&drives);
        assert!(expr.contains("source:drive1"));
        assert!(vars.contains("set drive1 to POSIX file \"/path/to/disk.img\""));
        assert!(!expr.contains("removable"));
    }

    // -----------------------------------------------------------------------
    // Regression test: the exact AppleScript that icbm sends to UTM must
    // match the format that was proven working before the utmvm migration.
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_script_icbm_regression() {
        use crate::config::{Architecture, DriveConfig, NetworkMode, PortForward, VmConfig};

        let config = VmConfig::builder("icbm-ubuntu")
            .architecture(Architecture::Aarch64)
            .memory_mb(4096)
            .cpu_count(2)
            .hypervisor(true)
            .drive(DriveConfig::disk_image("/tmp/noble.img"))
            .drive(DriveConfig::cdrom("/tmp/seed.iso"))
            .network_mode(NetworkMode::Emulated)
            .port_forward(PortForward {
                protocol: "TCP".to_string(),
                host_port: 2222,
                guest_port: 22,
            })
            .build();

        let mut script_config = config.clone();
        script_config.port_forwards.clear();
        let script = build_create_script(&script_config);

        // `create` clears port-forwards before calling build_create_script to
        // avoid a -1700 AppleScript coercion error; verify the script has no
        // port-forwards record in it.
        let expected = concat!(
            "tell application \"UTM\"\n",
            "    set drive1 to POSIX file \"/tmp/noble.img\"\n",
            "    set drive2 to POSIX file \"/tmp/seed.iso\"\n",
            "    make new virtual machine with properties {backend:qemu, configuration:{",
            "name:\"icbm-ubuntu\", ",
            "architecture:\"aarch64\", ",
            "memory:4096, ",
            "cpu cores:2, ",
            "hypervisor:true, ",
            "drives:{{source:drive1}, {removable:true, source:drive2}}, ",
            "network interfaces:{{mode:emulated}}}}\n",
            "end tell\n",
        );

        assert_eq!(
            script, expected,
            "Generated AppleScript does not match the known-working template.\n\n",
        );
    }

    #[test]
    fn test_create_script_balanced_braces() {
        use crate::config::{Architecture, DriveConfig, NetworkMode, PortForward, VmConfig};

        // Emulated branch (icbm path)
        let cfg1 = VmConfig::builder("test-vm")
            .architecture(Architecture::Aarch64)
            .memory_mb(2048)
            .cpu_count(2)
            .drive(DriveConfig::disk_image("/tmp/disk.img"))
            .drive(DriveConfig::cdrom("/tmp/seed.iso"))
            .network_mode(NetworkMode::Emulated)
            .port_forward(PortForward {
                protocol: "TCP".to_string(),
                host_port: 2222,
                guest_port: 22,
            })
            .build();

        let s1 = build_create_script(&cfg1);
        let opens1 = s1.chars().filter(|&c| c == '{').count();
        let closes1 = s1.chars().filter(|&c| c == '}').count();
        assert_eq!(
            opens1, closes1,
            "Emulated-branch AppleScript has unbalanced braces:\n{s1}"
        );

        // Bridged branch
        let cfg2 = VmConfig::builder("test-vm2")
            .architecture(Architecture::X86_64)
            .memory_mb(4096)
            .cpu_count(4)
            .drive(DriveConfig::disk(40))
            .network_mode(NetworkMode::Bridged)
            .build();

        let s2 = build_create_script(&cfg2);
        let opens2 = s2.chars().filter(|&c| c == '{').count();
        let closes2 = s2.chars().filter(|&c| c == '}').count();
        assert_eq!(
            opens2, closes2,
            "Bridged-branch AppleScript has unbalanced braces:\n{s2}"
        );
    }
}