//! Hardware scan: what this machine is, and how much memory models may use on it.

mod gpu;
mod tier;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

pub use tier::Tier;

/// Everything the selector needs to know about a machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub os: Os,
    pub cpu: Cpu,
    /// Installed memory.
    pub ram_total_mb: u64,
    /// Memory that could be handed to a new process right now.
    pub ram_available_mb: u64,
    pub gpus: Vec<Gpu>,
    pub storage: Storage,
    /// Memory a model may occupy when it runs on the accelerator (GPU or unified memory).
    /// Zero when there is no usable accelerator.
    pub accel_budget_mb: u64,
    /// Memory a model may occupy when it runs on the CPU.
    pub cpu_budget_mb: u64,
    pub tier: Tier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Os {
    /// "macos", "windows" or "linux".
    pub family: String,
    pub version: String,
    /// "aarch64" or "x86_64".
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cpu {
    pub brand: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    /// Instruction sets the engines care about: "neon", "avx2", "avx512f", "fma", "f16c".
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuKind {
    /// Shares system memory with the CPU and is fast (Apple Silicon).
    Unified,
    Discrete,
    Integrated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Metal,
    Cuda,
    Vulkan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    pub vendor: String,
    pub name: String,
    pub kind: GpuKind,
    pub backend: Backend,
    pub memory_total_mb: u64,
    /// Free memory at scan time; `None` when the platform does not report it.
    pub memory_free_mb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Storage {
    /// Where packages are stored.
    pub data_dir: PathBuf,
    pub free_mb: u64,
}

const MB: u64 = 1024 * 1024;

/// (accelerator budget, CPU budget) in MB for a set of GPUs and installed memory.
pub fn tier_budgets(gpus: &[Gpu], ram_total_mb: u64) -> (u64, u64) {
    (tier::accel_budget_mb(gpus, ram_total_mb), tier::cpu_budget_mb(ram_total_mb))
}

/// Where Rigbrew keeps packages, engines, logs and the event queue: `$RIGBREW_DATA_DIR` if set,
/// otherwise the platform's application-data folder.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RIGBREW_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Rigbrew")
}

/// Scan the machine. Never fails: anything that cannot be detected is reported as absent.
pub fn scan() -> HardwareProfile {
    scan_with_data_dir(&default_data_dir())
}

pub fn scan_with_data_dir(data_dir: &Path) -> HardwareProfile {
    // If the platform refuses to report (sandbox, unsupported OS), continue with zeros.
    let (ram_total_mb, ram_available_mb, brand, logical_cores) = match System::new() {
        Ok(mut sys) => {
            sys.refresh_memory();
            sys.refresh_cpu_all();
            (
                sys.total_memory() / MB,
                sys.available_memory() / MB,
                sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
                sys.cpus().len(),
            )
        }
        Err(_) => (0, 0, String::new(), 0),
    };
    let cpu = Cpu { brand, physical_cores: System::physical_core_count().unwrap_or(0), logical_cores, features: cpu_features() };
    let os = Os {
        family: std::env::consts::OS.to_string(),
        version: System::os_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
    };
    let gpus = gpu::detect(&cpu, ram_total_mb);
    let storage = Storage { data_dir: data_dir.to_path_buf(), free_mb: free_space_mb(data_dir) };

    let (accel_budget_mb, cpu_budget_mb) = tier_budgets(&gpus, ram_total_mb);
    let tier = Tier::classify(&gpus, ram_total_mb);

    HardwareProfile { os, cpu, ram_total_mb, ram_available_mb, gpus, storage, accel_budget_mb, cpu_budget_mb, tier }
}

fn cpu_features() -> Vec<String> {
    let mut f = Vec::new();
    #[cfg(target_arch = "aarch64")]
    {
        f.push("neon".to_string());
    }
    #[cfg(target_arch = "x86_64")]
    {
        for (name, present) in [
            ("avx2", std::is_x86_feature_detected!("avx2")),
            ("avx512f", std::is_x86_feature_detected!("avx512f")),
            ("fma", std::is_x86_feature_detected!("fma")),
            ("f16c", std::is_x86_feature_detected!("f16c")),
        ] {
            if present {
                f.push(name.to_string());
            }
        }
    }
    f
}

/// Free space on the volume that holds `path` (or its nearest existing parent).
fn free_space_mb(path: &Path) -> u64 {
    let Ok(disks) = Disks::new_with_refreshed_list() else {
        return 0;
    };
    let target = path.ancestors().find(|p| p.exists()).unwrap_or(Path::new("/"));
    disks
        .iter()
        .filter(|d| target.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space() / MB)
        .unwrap_or(0)
}
