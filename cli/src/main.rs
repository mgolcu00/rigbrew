//! `rigbrew` command line.

use rigbrew_core::hardware::{self, GpuKind, HardwareProfile};
use rigbrew_core::select::{recommend, Placement, Verdict};
use rigbrew_core::Manifest;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("scan") => scan(args.iter().any(|a| a == "--json")),
        Some("--version") | Some("-V") => println!("rigbrew {}", env!("CARGO_PKG_VERSION")),
        Some("recommend") => match args.get(1) {
            Some(path) => recommend_all(path, args.iter().any(|a| a == "--json")),
            None => usage(),
        },
        Some("chat") => match (args.get(1), args.get(2)) {
            (Some(manifest), Some(package)) if args.len() > 3 => chat(manifest, package, &args[3..].join(" ")),
            _ => usage(),
        },
        Some("pull") => match (args.get(1), args.get(2)) {
            (Some(manifest), Some(package)) => pull(manifest, package, args.get(3).map(String::as_str)),
            _ => usage(),
        },
        _ => usage(),
    }
}

fn load_manifest(path: &str) -> Manifest {
    let (mut manifest, signed) = rigbrew_core::manifest::load_file(std::path::Path::new(path)).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if !signed {
        eprintln!("warning: {path} is not signed (accepted in development builds or with RIGBREW_ALLOW_UNSIGNED=1)");
    }
    // Development override: pull from a local staging server instead of the registry.
    if let Ok(base) = std::env::var("RIGBREW_BASE_URL") {
        manifest.base_url = base;
    }
    manifest
}

fn pull(manifest_path: &str, package_id: &str, variant_id: Option<&str>) {
    let manifest = load_manifest(manifest_path);
    let Some(package) = manifest.package(package_id) else {
        eprintln!("no package {package_id} in the manifest");
        std::process::exit(1);
    };
    let profile = hardware::scan();
    let variant_id = match variant_id {
        Some(v) => v.to_string(),
        None => match recommend(&profile, package).verdict {
            Verdict::Run { variant, reason, .. } => {
                println!("{}: {variant} ({reason})", package.name);
                variant
            }
            Verdict::NeedsDisk { needed_mb, free_mb, .. } => {
                eprintln!("{} needs {} of disk and only {} is free", package.name, gb(needed_mb), gb(free_mb));
                std::process::exit(1);
            }
            Verdict::TooHeavy { reason, .. } => {
                eprintln!("{} does not fit this machine: {reason}", package.name);
                std::process::exit(1);
            }
        },
    };
    let Some(variant) = package.variants.iter().find(|v| v.id == variant_id) else {
        eprintln!("{package_id} has no variant {variant_id}");
        std::process::exit(1);
    };

    let data_dir = profile.storage.data_dir.join("packages");
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let client = rigbrew_core::download::client();
    let mut shown = (String::new(), u64::MAX);
    let mut progress = |p: rigbrew_core::download::Progress<'_>| {
        let pct = (p.done * 100).checked_div(p.total).unwrap_or(100);
        if shown.0 != p.path || shown.1 != pct {
            shown = (p.path.to_string(), pct);
            eprint!("\r{:<44} {pct:>3}%  {} / {}   ", p.path, gb(p.done / (1024 * 1024)), gb(p.total / (1024 * 1024)));
        }
    };
    let result = runtime.block_on(rigbrew_core::download::pull(
        &client,
        &manifest,
        package,
        variant,
        &data_dir,
        profile.storage.free_mb,
        &mut progress,
    ));
    eprintln!();
    match result {
        Ok(files) => {
            for (path, outcome) in files {
                println!("{outcome:?}  {}", data_dir.join(path).display());
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn usage() -> ! {
    eprintln!("usage: rigbrew scan [--json]\n       rigbrew recommend <manifest.json> [--json]\n       rigbrew pull <manifest.json> <package> [variant]\n       rigbrew chat <manifest.json> <package> <prompt>");
    std::process::exit(2);
}

fn recommend_all(path: &str, json: bool) {
    let manifest = load_manifest(path);
    let profile = hardware::scan();
    let recs: Vec<_> = manifest.packages.iter().map(|p| recommend(&profile, p)).collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&recs).expect("recommendations serialize"));
        return;
    }
    println!("{} — {}", profile.cpu.brand, profile.tier.label());
    for (p, r) in manifest.packages.iter().zip(&recs) {
        match &r.verdict {
            Verdict::Run { variant, placement, memory_mb, disk_mb, reason, .. } => {
                let place = if *placement == Placement::Accelerator { "GPU" } else { "CPU" };
                println!("{:<14} {variant} on {place}  ({} memory, {} download)", p.name, gb(*memory_mb), gb(*disk_mb));
                println!("{:<14} {reason}", "");
            }
            Verdict::NeedsDisk { variant, needed_mb, free_mb } => {
                println!("{:<14} {variant} fits, but needs {} of disk and only {} is free", p.name, gb(*needed_mb), gb(*free_mb));
            }
            Verdict::TooHeavy { reason, .. } => println!("{:<14} not on this machine: {reason}", p.name),
        }
    }
}

fn scan(json: bool) {
    let profile = hardware::scan();
    if json {
        println!("{}", serde_json::to_string_pretty(&profile).expect("profile serializes"));
    } else {
        print_profile(&profile);
    }
}

fn gb(mb: u64) -> String {
    format!("{:.1} GB", mb as f64 / 1024.0)
}

fn print_profile(p: &HardwareProfile) {
    println!("System   {} {} ({})", p.os.family, p.os.version, p.os.arch);
    println!(
        "CPU      {}, {} cores{}",
        p.cpu.brand,
        p.cpu.physical_cores,
        if p.cpu.features.is_empty() { String::new() } else { format!(" [{}]", p.cpu.features.join(", ")) }
    );
    println!("Memory   {} installed, {} available", gb(p.ram_total_mb), gb(p.ram_available_mb));
    if p.gpus.is_empty() {
        println!("GPU      none detected");
    }
    for g in &p.gpus {
        let kind = match g.kind {
            GpuKind::Unified => "unified memory",
            GpuKind::Discrete => "discrete",
            GpuKind::Integrated => "integrated",
        };
        println!("GPU      {} ({}, {})", g.name, kind, gb(g.memory_total_mb));
    }
    println!("Storage  {} free in {}", gb(p.storage.free_mb), p.storage.data_dir.display());
    println!();
    println!("Class    {}", p.tier.label());
    if p.accel_budget_mb > 0 {
        println!("Budget   {} for models on the GPU, {} on the CPU", gb(p.accel_budget_mb), gb(p.cpu_budget_mb));
    } else {
        println!("Budget   {} for models on the CPU", gb(p.cpu_budget_mb));
    }
}

/// Loads the installed variant of a chat package, answers one prompt and unloads it.
fn chat(manifest_path: &str, package_id: &str, prompt: &str) {
    use rigbrew_core::engine::{engine_path, LlamaConfig, LlamaServer};
    use rigbrew_core::manifest::Engine;
    use std::io::Write;

    let manifest = load_manifest(manifest_path);
    let Some(package) = manifest.package(package_id) else {
        eprintln!("no package {package_id} in the manifest");
        std::process::exit(1);
    };
    let profile = hardware::scan();
    let root = profile.storage.data_dir.clone();
    let packages = root.join("packages");
    // Use whichever variant is fully on disk, best quality first.
    let mut variants: Vec<_> = package.variants.iter().collect();
    variants.sort_by(|a, b| b.quality.total_cmp(&a.quality));
    let Some(variant) = variants.into_iter().find(|v| v.files.iter().all(|f| packages.join(&f.path).is_file())) else {
        eprintln!("{} is not installed; run: rigbrew pull {manifest_path} {package_id}", package.name);
        std::process::exit(1);
    };
    let file = |role: &str| variant.files.iter().find(|f| f.role == role).map(|f| packages.join(&f.path));
    let on_gpu = variant.accel_mem_mb <= profile.accel_budget_mb;
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    // Development: RIGBREW_ENGINE_DIR. Otherwise the engine from the signed manifest, installed on first use.
    let binary = if std::env::var_os("RIGBREW_ENGINE_DIR").is_some() {
        engine_path(&root, "llama-server")
    } else {
        let backends: Vec<_> = profile.gpus.iter().map(|g| g.backend).collect();
        let Some(build) = rigbrew_core::engines::pick(&manifest, Engine::Llama, &backends) else {
            eprintln!("there is no chat engine for {} in the manifest", rigbrew_core::engines::platform());
            std::process::exit(1);
        };
        if rigbrew_core::engines::installed(&root, build, "llama-server").is_none() {
            eprintln!("installing the chat engine ({})…", gb(build.archive.size / (1024 * 1024)));
            if let Err(e) =
                runtime.block_on(rigbrew_core::engines::install(&rigbrew_core::download::client(), &manifest, build, &root, &mut |_| {}))
            {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        rigbrew_core::engines::installed(&root, build, "llama-server").expect("installed above")
    };
    let cfg = LlamaConfig {
        binary,
        model: file("model").expect("chat package has a model file"),
        mmproj: file("mmproj"),
        context: 8192,
        gpu_layers: if on_gpu { 999 } else { 0 },
        log: root.join("logs").join("llama-server.log"),
    };
    let result = runtime.block_on(async {
        let server = LlamaServer::start(&cfg, std::time::Duration::from_secs(300)).await?;
        eprintln!("[{} {} on {}, loaded in {:.1} s]", package.name, variant.id, if on_gpu { "GPU" } else { "CPU" }, server.load_seconds);
        let messages = serde_json::json!([{"role": "user", "content": prompt}]);
        let stats = server
            .chat(&messages, 512, &mut |text| {
                print!("{text}");
                std::io::stdout().flush().ok();
            })
            .await;
        server.stop().await;
        stats
    });
    println!();
    match result {
        Ok(s) => eprintln!("[{} tokens, {:.1} tok/s]", s.generated_tokens, s.tokens_per_second),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
