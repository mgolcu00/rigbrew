//! The selector: which variant of a package this machine should run, and why.
//!
//! Rule: among the variants that fit, take the lightest one whose measured quality is at or
//! above the package's floor; if none reaches the floor, take the best one that fits. The
//! accelerator is preferred over the CPU whenever something fits there.

use serde::Serialize;

use crate::hardware::{Backend, HardwareProfile};
use crate::manifest::{Package, Variant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Accelerator,
    Cpu,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub package: String,
    #[serde(flatten)]
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Run {
        variant: String,
        placement: Placement,
        /// True when the choice is below the package's quality floor because nothing better fits.
        compromise: bool,
        memory_mb: u64,
        disk_mb: u64,
        reason: String,
    },
    /// The machine could run it, but the disk is too full to download it.
    NeedsDisk { variant: String, needed_mb: u64, free_mb: u64 },
    /// Nothing fits.
    TooHeavy { smallest_mb: u64, budget_mb: u64, reason: String },
}

fn gb(mb: u64) -> String {
    format!("{:.1} GB", mb as f64 / 1024.0)
}

/// Variants that fit `budget`, as (variant, memory needed).
fn fitting(variants: &[Variant], budget: u64, mem: impl Fn(&Variant) -> Option<u64>) -> Vec<(&Variant, u64)> {
    variants.iter().filter_map(|v| mem(v).filter(|m| *m <= budget).map(|m| (v, m))).collect()
}

/// Lightest variant at or above the floor, else the best quality. Returns (variant, memory, compromise).
fn pick<'a>(fits: &[(&'a Variant, u64)], floor: f32) -> Option<(&'a Variant, u64, bool)> {
    let good = fits.iter().filter(|(v, _)| v.quality >= floor).min_by_key(|(_, m)| *m);
    if let Some((v, m)) = good {
        return Some((v, *m, false));
    }
    fits.iter().max_by(|a, b| a.0.quality.total_cmp(&b.0.quality)).map(|(v, m)| (*v, *m, true))
}

pub fn recommend(profile: &HardwareProfile, package: &Package) -> Recommendation {
    let backends: Vec<Backend> = profile.gpus.iter().map(|g| g.backend).collect();
    let on_accel =
        fitting(&package.variants, profile.accel_budget_mb, |v| v.backends.iter().any(|b| backends.contains(b)).then_some(v.accel_mem_mb));
    let on_cpu = fitting(&package.variants, profile.cpu_budget_mb, |v| v.cpu_mem_mb);

    let choice = pick(&on_accel, package.quality_floor)
        .map(|c| (c, Placement::Accelerator))
        .or_else(|| pick(&on_cpu, package.quality_floor).map(|c| (c, Placement::Cpu)));

    let verdict = match choice {
        Some(((variant, memory_mb, compromise), placement)) => {
            let disk_mb = variant.disk_mb();
            if disk_mb > profile.storage.free_mb {
                Verdict::NeedsDisk { variant: variant.id.clone(), needed_mb: disk_mb, free_mb: profile.storage.free_mb }
            } else {
                let (place, budget) = match placement {
                    Placement::Accelerator => ("GPU", profile.accel_budget_mb),
                    Placement::Cpu => ("CPU", profile.cpu_budget_mb),
                };
                let reason = if compromise {
                    format!(
                        "best that fits: needs {} of the {} {place} budget; a higher-quality variant does not fit",
                        gb(memory_mb),
                        gb(budget)
                    )
                } else {
                    format!("lightest variant that meets the quality bar: needs {} of the {} {place} budget", gb(memory_mb), gb(budget))
                };
                Verdict::Run { variant: variant.id.clone(), placement, compromise, memory_mb, disk_mb, reason }
            }
        }
        None => {
            let smallest =
                package.variants.iter().map(|v| v.cpu_mem_mb.map_or(v.accel_mem_mb, |c| c.min(v.accel_mem_mb))).min().unwrap_or(0);
            let budget = profile.accel_budget_mb.max(profile.cpu_budget_mb);
            Verdict::TooHeavy {
                smallest_mb: smallest,
                budget_mb: budget,
                reason: format!("the smallest variant needs {}, this machine can give {}", gb(smallest), gb(budget)),
            }
        }
    };
    Recommendation { package: package.id.clone(), verdict }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::hardware::{Cpu, Gpu, GpuKind, Os, Storage, Tier};
    use crate::manifest::{Access, Engine, FileRef, Modality, Source};

    fn machine(gpu: Option<(GpuKind, Backend, u64)>, ram_gb: u64, free_gb: u64) -> HardwareProfile {
        let ram = ram_gb * 1024;
        let gpus: Vec<Gpu> = gpu
            .map(|(kind, backend, mb)| Gpu {
                vendor: "t".into(),
                name: "t".into(),
                kind,
                backend,
                memory_total_mb: mb,
                memory_free_mb: None,
            })
            .into_iter()
            .collect();
        HardwareProfile {
            os: Os { family: "test".into(), version: String::new(), arch: "x".into() },
            cpu: Cpu { brand: "t".into(), physical_cores: 8, logical_cores: 8, features: vec![] },
            ram_total_mb: ram,
            ram_available_mb: ram,
            accel_budget_mb: crate::hardware::tier_budgets(&gpus, ram).0,
            cpu_budget_mb: crate::hardware::tier_budgets(&gpus, ram).1,
            tier: Tier::classify(&gpus, ram),
            gpus,
            storage: Storage { data_dir: PathBuf::from("/tmp"), free_mb: free_gb * 1024 },
        }
    }

    fn variant(id: &str, mb: u64, quality: f32, cpu: bool) -> Variant {
        Variant {
            id: id.into(),
            files: vec![FileRef { role: "model".into(), path: format!("p/{id}.gguf"), size: mb * 1024 * 1024, sha256: "0".repeat(64) }],
            accel_mem_mb: mb + 600,
            cpu_mem_mb: cpu.then_some(mb + 600),
            backends: vec![Backend::Metal, Backend::Cuda, Backend::Vulkan],
            quality,
            metrics: serde_json::Value::Null,
        }
    }

    fn chat() -> Package {
        Package {
            id: "steep-4b".into(),
            name: "Steep 4B".into(),
            modality: Modality::Chat,
            version: 1,
            engine: Engine::Llama,
            access: Access::Public,
            source: Source { name: "s".into(), url: "u".into(), license: "apache-2.0".into(), changes: "c".into() },
            quality_floor: 0.96,
            variants: vec![
                variant("q3", 2300, 0.868, true),
                variant("q4", 2800, 0.932, true),
                variant("q5", 3200, 0.964, true),
                variant("q6", 3600, 0.973, true),
                variant("q8", 4600, 0.991, true),
            ],
        }
    }

    fn run(r: &Recommendation) -> (&str, Placement, bool) {
        match &r.verdict {
            Verdict::Run { variant, placement, compromise, .. } => (variant.as_str(), *placement, *compromise),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn big_machine_takes_the_lightest_variant_above_the_floor_not_the_largest() {
        let m = machine(Some((GpuKind::Unified, Backend::Metal, 48 * 1024)), 48, 400);
        assert_eq!(run(&recommend(&m, &chat())), ("q5", Placement::Accelerator, false));
    }

    #[test]
    fn small_card_falls_back_to_the_best_that_fits() {
        // 4 GB card is ignored by the tier but still has a budget of 3396 MB: q4 (3400) does not fit.
        let m = machine(Some((GpuKind::Discrete, Backend::Cuda, 4 * 1024)), 8, 400);
        assert_eq!(run(&recommend(&m, &chat())), ("q3", Placement::Accelerator, true));
    }

    #[test]
    fn no_gpu_runs_on_the_cpu() {
        let m = machine(None, 8, 400);
        assert_eq!(run(&recommend(&m, &chat())), ("q5", Placement::Cpu, false));
    }

    #[test]
    fn gpu_only_package_is_too_heavy_without_a_gpu() {
        let mut p = chat();
        p.variants = vec![variant("i8", 9000, 0.97, false)];
        let m = machine(None, 16, 400);
        assert!(matches!(recommend(&m, &p).verdict, Verdict::TooHeavy { .. }));
    }

    #[test]
    fn full_disk_is_reported_separately() {
        let m = machine(Some((GpuKind::Unified, Backend::Metal, 48 * 1024)), 48, 1);
        assert!(matches!(recommend(&m, &chat()).verdict, Verdict::NeedsDisk { needed_mb: 3200, .. }));
    }

    #[test]
    fn a_backend_the_machine_lacks_is_not_used() {
        let mut p = chat();
        for v in &mut p.variants {
            v.backends = vec![Backend::Cuda];
        }
        let m = machine(Some((GpuKind::Unified, Backend::Metal, 16 * 1024)), 16, 400);
        assert_eq!(run(&recommend(&m, &p)).1, Placement::Cpu);
    }
}
