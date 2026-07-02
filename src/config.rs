use crate::network::MacAddress;
use std::path::PathBuf;

/// Target CPU architecture for the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Architecture {
    /// 64-bit ARM (Apple Silicon native, also works on Intel via emulation).
    Aarch64,
    /// x86-64.
    X86_64,
}

impl Architecture {
    /// Returns the string UTM / QEMU uses for this architecture.
    pub fn as_str(&self) -> &'static str {
        match self {
            Architecture::Aarch64 => "aarch64",
            Architecture::X86_64 => "x86_64",
        }
    }

    /// The UEFI fallback EFI binary name for this architecture.
    pub fn efi_boot_binary(&self) -> &'static str {
        match self {
            Architecture::Aarch64 => "BOOTAA64.EFI",
            Architecture::X86_64 => "BOOTX64.EFI",
        }
    }
}

/// Number of virtual CPUs.
pub type CpuCount = u32;

/// RAM in megabytes.
pub type MemoryMb = u32;

/// Disk size in gigabytes.
pub type DiskSizeGb = u32;

/// Network interface mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkMode {
    /// Bridged — VM gets a real IP from the host LAN DHCP.
    /// Recommended for automation; requires macOS network permissions.
    Bridged,
    /// Shared (NAT) — VM is behind 10.0.2.x NAT. No admin privileges needed.
    Shared,
    /// Host-only — private network, no internet access.
    HostOnly,
    /// Emulated (QEMU user networking) — supports port forwarding without admin privileges.
    Emulated,
}

impl NetworkMode {
    pub(crate) fn as_applescript_str(&self) -> &'static str {
        match self {
            NetworkMode::Bridged => "bridged",
            NetworkMode::Shared => "shared",
            NetworkMode::HostOnly => "host only",
            NetworkMode::Emulated => "emulated",
        }
    }

    pub(crate) fn as_plist_str(&self) -> &'static str {
        match self {
            NetworkMode::Bridged => "Bridged",
            NetworkMode::Shared => "Shared",
            NetworkMode::HostOnly => "HostOnly",
            NetworkMode::Emulated => "Emulated",
        }
    }
}

/// Display (GPU) hardware model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayHardware {
    /// VirtIO OpenGL-accelerated display (recommended for Linux guests).
    VirtioGpuGlPci,
    /// Standard VGA.
    Vga,
}

impl DisplayHardware {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            DisplayHardware::VirtioGpuGlPci => "virtio-gpu-gl-pci",
            DisplayHardware::Vga => "VGA",
        }
    }
}

/// A port-forward rule for emulated network mode.
#[derive(Debug, Clone)]
pub struct PortForward {
    /// Protocol name used in UTM AppleScript (e.g. "TCP").
    pub protocol: String,
    /// Host-side port.
    pub host_port: u16,
    /// Guest-side port.
    pub guest_port: u16,
}

/// Drive interface type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveInterface {
    /// VirtIO block device — best performance for Linux guests.
    VirtIO,
    /// USB storage — required for CD-ROM / ISO images in UTM.
    Usb,
    /// NVMe.
    NvMe,
}

impl DriveInterface {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            DriveInterface::VirtIO => "VirtIO",
            DriveInterface::Usb => "USB",
            DriveInterface::NvMe => "NVMe",
        }
    }
}

/// Configuration for a single drive attached to a VM.
#[derive(Debug, Clone)]
pub struct DriveConfig {
    /// Size in GB. `None` for CD-ROM/ISO or image-backed drives.
    pub size_gb: Option<DiskSizeGb>,
    /// Path to an ISO image. When set this is treated as a read-only CD-ROM.
    pub iso_path: Option<PathBuf>,
    /// Path to an existing disk image (raw or qcow2) to use as the drive source.
    pub image_path: Option<PathBuf>,
    /// Interface type. Defaults to VirtIO for disks, USB for CD-ROMs.
    pub interface: DriveInterface,
    /// Read-only flag.
    pub read_only: bool,
}

impl DriveConfig {
    /// Create a new blank disk drive.
    pub fn disk(size_gb: DiskSizeGb) -> Self {
        Self {
            size_gb: Some(size_gb),
            iso_path: None,
            image_path: None,
            interface: DriveInterface::VirtIO,
            read_only: false,
        }
    }

    /// Create a CD-ROM drive backed by an ISO file.
    pub fn cdrom(iso_path: impl Into<PathBuf>) -> Self {
        Self {
            size_gb: None,
            iso_path: Some(iso_path.into()),
            image_path: None,
            interface: DriveInterface::Usb,
            read_only: true,
        }
    }

    /// Create a drive backed by an existing disk image.
    pub fn disk_image(image_path: impl Into<PathBuf>) -> Self {
        Self {
            size_gb: None,
            iso_path: None,
            image_path: Some(image_path.into()),
            interface: DriveInterface::VirtIO,
            read_only: false,
        }
    }
}

/// Complete configuration for creating a new VM.
///
/// # Example
///
/// ```rust
/// use utmvm::{VmConfig, Architecture, NetworkMode, DriveConfig};
///
/// let cfg = VmConfig::builder("my-vm")
///     .architecture(Architecture::Aarch64)
///     .memory_mb(2048)
///     .cpu_count(2)
///     .drive(DriveConfig::cdrom("/path/to/installer.iso"))
///     .drive(DriveConfig::disk(20))
///     .network_mode(NetworkMode::Bridged)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// VM display name.
    pub name: String,
    /// Optional human-readable notes shown in UTM.
    pub notes: Option<String>,
    /// Target CPU architecture.
    pub architecture: Architecture,
    /// RAM in megabytes.
    pub memory_mb: MemoryMb,
    /// Number of virtual CPUs.
    pub cpu_count: CpuCount,
    /// Attached drives (in order). CD-ROM should come first for install images.
    pub drives: Vec<DriveConfig>,
    /// Network mode.
    pub network_mode: NetworkMode,
    /// Port-forward rules (only used in emulated network mode).
    pub port_forwards: Vec<PortForward>,
    /// MAC address. `None` → randomly generated.
    pub mac_address: Option<MacAddress>,
    /// Display hardware.
    pub display: DisplayHardware,
    /// Enable hypervisor (HVF on macOS). Defaults to `false`.
    pub hypervisor: bool,
    /// Extra QEMU command-line arguments appended to the VM launch.
    pub qemu_additional_args: Vec<String>,
}

impl VmConfig {
    /// Start building a [`VmConfig`] with the given name and sensible defaults.
    pub fn builder(name: impl Into<String>) -> VmConfigBuilder {
        VmConfigBuilder::new(name)
    }
}

/// Fluent builder for [`VmConfig`].
pub struct VmConfigBuilder {
    inner: VmConfig,
}

impl VmConfigBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            inner: VmConfig {
                name: name.into(),
                notes: None,
                architecture: Architecture::Aarch64,
                memory_mb: 2048,
                cpu_count: 2,
                drives: Vec::new(),
                network_mode: NetworkMode::Bridged,
                port_forwards: Vec::new(),
                mac_address: None,
                display: DisplayHardware::VirtioGpuGlPci,
                hypervisor: false,
                qemu_additional_args: Vec::new(),
            },
        }
    }

    /// Set notes shown in UTM UI.
    pub fn notes(mut self, notes: impl Into<String>) -> Self {
        self.inner.notes = Some(notes.into());
        self
    }

    /// Set target architecture.
    pub fn architecture(mut self, arch: Architecture) -> Self {
        self.inner.architecture = arch;
        self
    }

    /// Set RAM in megabytes.
    pub fn memory_mb(mut self, mb: MemoryMb) -> Self {
        self.inner.memory_mb = mb;
        self
    }

    /// Set number of virtual CPUs.
    pub fn cpu_count(mut self, count: CpuCount) -> Self {
        self.inner.cpu_count = count;
        self
    }

    /// Append a drive.
    pub fn drive(mut self, drive: DriveConfig) -> Self {
        self.inner.drives.push(drive);
        self
    }

    /// Set network mode.
    pub fn network_mode(mut self, mode: NetworkMode) -> Self {
        self.inner.network_mode = mode;
        self
    }

    /// Append a port-forward rule (emulated mode only).
    pub fn port_forward(mut self, pf: PortForward) -> Self {
        self.inner.port_forwards.push(pf);
        self
    }

    /// Pin a specific MAC address (otherwise randomly generated at creation time).
    pub fn mac_address(mut self, mac: MacAddress) -> Self {
        self.inner.mac_address = Some(mac);
        self
    }

    /// Set display hardware.
    pub fn display(mut self, display: DisplayHardware) -> Self {
        self.inner.display = display;
        self
    }

    /// Enable or disable hypervisor acceleration.
    pub fn hypervisor(mut self, enabled: bool) -> Self {
        self.inner.hypervisor = enabled;
        self
    }

    /// Add an extra QEMU command-line argument.
    pub fn qemu_arg(mut self, arg: impl Into<String>) -> Self {
        self.inner.qemu_additional_args.push(arg.into());
        self
    }

    /// Consume the builder and return the [`VmConfig`].
    pub fn build(self) -> VmConfig {
        self.inner
    }
}