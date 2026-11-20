//! GPU detection.
//!
//! macOS on Apple Silicon: one unified-memory GPU (Metal).
//! Elsewhere: NVIDIA cards through `nvidia-smi` (CUDA).
//! Not done yet: AMD and Intel adapters on Windows (DXGI) and Linux (Vulkan); those machines
//! are currently reported without a GPU and fall into a CPU tier.

use std::process::Command;

use super::{Backend, Cpu, Gpu, GpuKind};

pub fn detect(cpu: &Cpu, ram_total_mb: u64) -> Vec<Gpu> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return vec![Gpu {
            vendor: "Apple".into(),
            name: cpu.brand.clone(),
            kind: GpuKind::Unified,
            backend: Backend::Metal,
            memory_total_mb: ram_total_mb,
            memory_free_mb: None,
        }];
    }
    nvidia_smi().unwrap_or_default()
}

fn nvidia_smi() -> Option<Vec<Gpu>> {
    let out =
        Command::new("nvidia-smi").args(["--query-gpu=name,memory.total,memory.free", "--format=csv,noheader,nounits"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_nvidia_smi(&String::from_utf8_lossy(&out.stdout)))
}

/// Parses lines like `NVIDIA GeForce RTX 3060 Laptop GPU, 6144, 5890`.
fn parse_nvidia_smi(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(',').map(str::trim);
            let name = parts.next()?.to_string();
            let total = parts.next()?.parse().ok()?;
            let free = parts.next().and_then(|v| v.parse().ok());
            Some(Gpu {
                vendor: "NVIDIA".into(),
                name,
                kind: GpuKind::Discrete,
                backend: Backend::Cuda,
                memory_total_mb: total,
                memory_free_mb: free,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_smi_output() {
        let gpus = parse_nvidia_smi("NVIDIA GeForce RTX 3060 Laptop GPU, 6144, 5890\nNVIDIA RTX A4000, 16376, [N/A]\n\n");
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 3060 Laptop GPU");
        assert_eq!(gpus[0].memory_total_mb, 6144);
        assert_eq!(gpus[0].memory_free_mb, Some(5890));
        assert_eq!(gpus[1].memory_free_mb, None);
    }

    #[test]
    fn ignores_garbage() {
        assert!(parse_nvidia_smi("NVIDIA-SMI has failed because it couldn't communicate with the driver").is_empty());
    }
}
