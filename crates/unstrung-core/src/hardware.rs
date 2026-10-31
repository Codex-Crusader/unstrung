//! The hardware check: GPU, VRAM, RAM, free disk space and CPU features.
//! The result sets the hardware tier, and the tier picks the recommended models.

use serde::{Deserialize, Serialize};
use std::path::Path;

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// No usable GPU, 8 GB RAM or more. Small models.
    Basic,
    /// 6 to 8 GB VRAM. 7B to 9B models.
    Standard,
    /// 12 GB VRAM or more. 12B to 14B models.
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gpu {
    pub name: String,
    pub vendor: String,
    pub vram_bytes: u64,
    pub driver: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hardware {
    /// The GPU with the most dedicated memory, if any.
    pub gpu: Option<Gpu>,
    pub ram_bytes: u64,
    /// Free space on the drive of the models folder.
    pub disk_free_bytes: u64,
    pub avx2: bool,
    pub tier: Tier,
    /// True when the PC is under the minimum (8 GB RAM).
    pub below_minimum: bool,
}

impl Hardware {
    /// Runs the full check. `models` is the models folder, for the free space.
    pub fn check(models: &Path) -> Self {
        let gpu = gpus().into_iter().max_by_key(|g| g.vram_bytes);
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let ram_bytes = system.total_memory();
        let disk_free_bytes = free_space(models);
        Self::from_parts(gpu, ram_bytes, disk_free_bytes, avx2())
    }

    pub fn from_parts(gpu: Option<Gpu>, ram_bytes: u64, disk_free_bytes: u64, avx2: bool) -> Self {
        let vram = gpu
            .as_ref()
            .filter(|g| usable(g))
            .map_or(0, |g| g.vram_bytes);
        // Cards sold as "12 GB" or "6 GB" report a little less.
        let tier = if vram >= GIB * 23 / 2 {
            Tier::High
        } else if vram >= GIB * 11 / 2 {
            Tier::Standard
        } else {
            Tier::Basic
        };
        Self {
            gpu,
            ram_bytes,
            disk_free_bytes,
            avx2,
            tier,
            below_minimum: ram_bytes < GIB * 15 / 2,
        }
    }

    /// Changes when the GPU or its driver changes. The core runs the check again then.
    pub fn fingerprint(&self) -> String {
        self.gpu.as_ref().map_or_else(
            || "no gpu".to_owned(),
            |g| format!("{} | {}", g.name, g.driver),
        )
    }
}

/// The chat engine runs on NVIDIA and AMD cards. Other GPUs count as no GPU.
fn usable(gpu: &Gpu) -> bool {
    matches!(gpu.vendor.as_str(), "NVIDIA" | "AMD")
}

fn vendor_name(id: &str) -> String {
    match id.to_ascii_lowercase().as_str() {
        "10de" => "NVIDIA".into(),
        "1002" | "1022" => "AMD".into(),
        "8086" => "Intel".into(),
        other => other.to_owned(),
    }
}

/// The vendor ID from a device ID like `pci\ven_10de&dev_28a0`.
fn vendor_of(device_id: &str) -> Option<String> {
    let lower = device_id.to_ascii_lowercase();
    let at = lower.find("ven_")? + 4;
    lower.get(at..at + 4).map(vendor_name)
}

/// The display adapters from the registry. The registry has the full VRAM size,
/// which WMI cuts at 4 GB.
#[cfg(windows)]
fn gpus() -> Vec<Gpu> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    const DISPLAY_CLASS: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let Ok(class) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(DISPLAY_CLASS) else {
        return Vec::new();
    };
    class
        .enum_keys()
        .filter_map(std::result::Result::ok)
        .filter(|k| k.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|k| class.open_subkey(k).ok())
        .filter_map(|key| {
            let device: String = key.get_value("MatchingDeviceId").ok()?;
            // Only real PCI cards. This skips the basic display driver and remote displays.
            if !device.to_ascii_lowercase().starts_with("pci\\") {
                return None;
            }
            let vram = key
                .get_value::<u64, _>("HardwareInformation.qwMemorySize")
                .or_else(|_| {
                    key.get_value::<u32, _>("HardwareInformation.MemorySize")
                        .map(u64::from)
                })
                .unwrap_or_default();
            Some(Gpu {
                name: key.get_value("DriverDesc").unwrap_or_default(),
                vendor: vendor_of(&device).unwrap_or_default(),
                vram_bytes: vram,
                driver: key.get_value("DriverVersion").unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(not(windows))]
fn gpus() -> Vec<Gpu> {
    Vec::new()
}

fn free_space(path: &Path) -> u64 {
    disk_of(path).map_or(0, |d| d.available)
}

pub(crate) struct DiskInfo {
    pub available: u64,
    pub removable: bool,
}

/// The local disk that holds `path`, or `None` for a network share or a
/// drive that is not connected. A missing folder counts by its nearest parent.
pub(crate) fn disk_of(path: &Path) -> Option<DiskInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let existing = path.ancestors().find(|p| p.exists()).unwrap_or(path);
    let target = std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    let target = target
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_lowercase();
    // The disk with the longest mount point that holds the path.
    disks
        .list()
        .iter()
        .filter(|d| target.starts_with(&d.mount_point().to_string_lossy().to_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| DiskInfo {
            available: d.available_space(),
            removable: d.is_removable(),
        })
}

fn avx2() -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(vendor: &str, gib_tenths: u64) -> Option<Gpu> {
        Some(Gpu {
            name: "card".into(),
            vendor: vendor.into(),
            vram_bytes: GIB * gib_tenths / 10,
            driver: "1".into(),
        })
    }

    #[test]
    fn tiers() {
        let tier = |g, ram| Hardware::from_parts(g, GIB * ram, 0, true).tier;
        assert_eq!(tier(gpu("NVIDIA", 80), 16), Tier::Standard);
        assert_eq!(tier(gpu("NVIDIA", 60), 16), Tier::Standard);
        assert_eq!(tier(gpu("NVIDIA", 40), 16), Tier::Basic);
        assert_eq!(tier(gpu("AMD", 119), 32), Tier::High);
        assert_eq!(tier(gpu("Intel", 160), 16), Tier::Basic);
        assert_eq!(tier(None, 16), Tier::Basic);
        assert!(Hardware::from_parts(None, GIB * 4, 0, true).below_minimum);
        assert!(!Hardware::from_parts(None, GIB * 8, 0, true).below_minimum);
    }

    #[test]
    fn vendors() {
        assert_eq!(
            vendor_of(r"pci\ven_10de&dev_28a0&subsys_8bcd103c").as_deref(),
            Some("NVIDIA")
        );
        assert_eq!(vendor_of(r"PCI\VEN_1002&DEV_15BF").as_deref(), Some("AMD"));
        assert_eq!(vendor_of("root\\basicdisplay"), None);
    }

    #[test]
    fn this_pc() {
        // Runs the real check. It must not fail, whatever the PC has.
        let hw = Hardware::check(Path::new("."));
        assert!(hw.ram_bytes > 0);
        assert!(!hw.fingerprint().is_empty());
    }
}
