//! Hardware tiers and memory budgets.
//!
//! A tier is the coarse class shown to people and used to group measurements. The budgets are
//! what the selector actually compares package sizes against.

use serde::{Deserialize, Serialize};

use super::{Gpu, GpuKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// No usable accelerator and under 12 GB of memory.
    Cpu8,
    /// No discrete GPU, 12 GB or more.
    Igpu16,
    /// Discrete GPU with 5 to 11 GB.
    Vram8,
    /// Discrete GPU with 12 to 19 GB.
    Vram16,
    /// Apple Silicon with under 24 GB.
    Mac16,
    /// Apple Silicon with 24 to 39 GB.
    Mac32,
    /// Discrete GPU with 20 GB or more, or Apple Silicon with 40 GB or more.
    Big,
}

impl Tier {
    pub fn classify(gpus: &[Gpu], ram_total_mb: u64) -> Tier {
        let gb = |mb: u64| mb as f64 / 1024.0;
        if gpus.iter().any(|g| g.kind == GpuKind::Unified) {
            return match gb(ram_total_mb) {
                x if x >= 40.0 => Tier::Big,
                x if x >= 24.0 => Tier::Mac32,
                _ => Tier::Mac16,
            };
        }
        let best = gpus.iter().filter(|g| g.kind == GpuKind::Discrete).map(|g| g.memory_total_mb).max();
        match best.map(gb) {
            Some(x) if x >= 19.5 => Tier::Big,
            Some(x) if x >= 11.5 => Tier::Vram16,
            Some(x) if x >= 4.5 => Tier::Vram8,
            _ if gb(ram_total_mb) >= 11.5 => Tier::Igpu16,
            _ => Tier::Cpu8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::Cpu8 => "CPU only, small memory",
            Tier::Igpu16 => "CPU or integrated graphics",
            Tier::Vram8 => "Graphics card, 6-8 GB",
            Tier::Vram16 => "Graphics card, 12-16 GB",
            Tier::Mac16 => "Apple Silicon, up to 16 GB",
            Tier::Mac32 => "Apple Silicon, 24-36 GB",
            Tier::Big => "Workstation class",
        }
    }
}

/// Memory models may take on the accelerator.
///
/// Discrete GPU: total VRAM minus 10% (at least 700 MB) for the desktop and engine overhead.
/// Unified memory: the system and other apps need room too, so two thirds of RAM up to 16 GB
/// and three quarters above that, never leaving less than 4 GB for the system.
pub fn accel_budget_mb(gpus: &[Gpu], ram_total_mb: u64) -> u64 {
    if gpus.iter().any(|g| g.kind == GpuKind::Unified) {
        let share = if ram_total_mb > 16 * 1024 { ram_total_mb * 3 / 4 } else { ram_total_mb * 2 / 3 };
        return share.min(ram_total_mb.saturating_sub(4 * 1024));
    }
    gpus.iter()
        .filter(|g| g.kind == GpuKind::Discrete)
        .map(|g| g.memory_total_mb.saturating_sub((g.memory_total_mb / 10).max(700)))
        .max()
        .unwrap_or(0)
}

/// Memory models may take when they run on the CPU: RAM minus 3 GB for the system, at most 70%.
pub fn cpu_budget_mb(ram_total_mb: u64) -> u64 {
    ram_total_mb.saturating_sub(3 * 1024).min(ram_total_mb * 7 / 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::Backend;

    fn gpu(kind: GpuKind, mb: u64) -> Gpu {
        Gpu { vendor: "test".into(), name: "test".into(), kind, backend: Backend::Vulkan, memory_total_mb: mb, memory_free_mb: None }
    }

    #[test]
    fn tiers() {
        assert_eq!(Tier::classify(&[], 8 * 1024), Tier::Cpu8);
        assert_eq!(Tier::classify(&[], 16 * 1024), Tier::Igpu16);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Integrated, 2048)], 16 * 1024), Tier::Igpu16);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Discrete, 6 * 1024)], 16 * 1024), Tier::Vram8);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Discrete, 8 * 1024)], 32 * 1024), Tier::Vram8);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Discrete, 12 * 1024)], 32 * 1024), Tier::Vram16);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Discrete, 24 * 1024)], 64 * 1024), Tier::Big);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Unified, 16 * 1024)], 16 * 1024), Tier::Mac16);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Unified, 36 * 1024)], 36 * 1024), Tier::Mac32);
        assert_eq!(Tier::classify(&[gpu(GpuKind::Unified, 48 * 1024)], 48 * 1024), Tier::Big);
    }

    #[test]
    fn a_4gb_card_is_not_a_gpu_tier() {
        // Under 4.5 GB the models we ship do not fit; treat the machine by its RAM.
        assert_eq!(Tier::classify(&[gpu(GpuKind::Discrete, 4 * 1024)], 16 * 1024), Tier::Igpu16);
    }

    #[test]
    fn budgets() {
        assert_eq!(accel_budget_mb(&[], 16 * 1024), 0);
        assert_eq!(accel_budget_mb(&[gpu(GpuKind::Discrete, 6 * 1024)], 16 * 1024), 6 * 1024 - 700);
        assert_eq!(accel_budget_mb(&[gpu(GpuKind::Discrete, 24 * 1024)], 64 * 1024), 24 * 1024 - 24 * 1024 / 10);
        assert_eq!(accel_budget_mb(&[gpu(GpuKind::Unified, 8 * 1024)], 8 * 1024), 4 * 1024);
        assert_eq!(accel_budget_mb(&[gpu(GpuKind::Unified, 16 * 1024)], 16 * 1024), 16 * 1024 * 2 / 3);
        assert_eq!(accel_budget_mb(&[gpu(GpuKind::Unified, 48 * 1024)], 48 * 1024), 36 * 1024);
        assert_eq!(cpu_budget_mb(8 * 1024), 5 * 1024);
        assert_eq!(cpu_budget_mb(64 * 1024), 64 * 1024 * 7 / 10);
    }
}
