use std::process::{Command, Child, Stdio};
use std::path::{Path, PathBuf};
use std::fs;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// Toggle to enable or disable custom cargo build for each pod.
/// If `false`, the runner will not build any binaries and expects them to be pre-built.
const USE_CUSTOM_CARGO_BUILD: bool = true;

/// If true, run `cargo clean` before building any pods.
const CLEAN_BEFORE_BUILD: bool = false;

/// Delay (in seconds) between launching each pod.
const LAUNCH_DELAY_SECS: u64 = 3;

/// Build mode for each pod: Debug, Release, or External (use whatever is present).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BuildMode {
    #[allow(dead_code)]
    Debug,
    #[allow(dead_code)]
    Release,
    #[allow(dead_code)]
    External, // Use pre-built binary, don't build
}

/// Specification for each pod to be launched.
#[derive(Copy, Clone, Debug)]
struct PodSpec {
    name: &'static str,
    build_mode: BuildMode,
}

/// List of pods to launch, with their desired build mode.
/// Add or modify entries here to control which pods are run and how.
const POD_CONFIG: &[PodSpec] = &[
    // use this to configure specific pods to be tested as Release or Debug
    PodSpec { name: "publish", build_mode: BuildMode::Release },
    PodSpec { name: "subscribe", build_mode: BuildMode::Release },
    // Example for future pods:
    // PodSpec { name: "analytics", build_mode: BuildMode::Release },
    // PodSpec { name: "iot_gateway", build_mode: BuildMode::External },
];

fn main() {
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };

    println!(
        "Steady State Pod Runner\n\
         ======================\n\
         USE_CUSTOM_CARGO_BUILD: {USE_CUSTOM_CARGO_BUILD}\n\
         CLEAN_BEFORE_BUILD: {CLEAN_BEFORE_BUILD}\n\
         LAUNCH_DELAY_SECS: {LAUNCH_DELAY_SECS}\n\
         exe_suffix: {exe_suffix}\n"
    );

    // Optionally clean before building
    if USE_CUSTOM_CARGO_BUILD && CLEAN_BEFORE_BUILD {
        println!("Running `cargo clean`...");
        let status = Command::new("cargo")
            .arg("clean")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to invoke cargo clean");
        if !status.success() {
            eprintln!("ERROR: Cargo clean failed.");
            return;
        }
        println!("Cargo clean complete.\n");
    }

    // === PHASE 1: BUILD ALL PODS FIRST ===
    if USE_CUSTOM_CARGO_BUILD {
        for pod in POD_CONFIG.iter() {
            if pod.build_mode == BuildMode::External {
                continue;
            }
            let profile = match pod.build_mode {
                BuildMode::Debug => "debug",
                BuildMode::Release => "release",
                BuildMode::External => unreachable!(),
            };
            let mut args = vec!["build"];
            if profile == "release" {
                args.push("--release");
            }
            args.push("--bin");
            args.push(pod.name);

            println!("Building pod '{}' (cargo {:?})...", pod.name, args);

            // Spinner setup
            let spinning = Arc::new(AtomicBool::new(true));
            let spinner_handle = {
                let spinning = Arc::clone(&spinning);
                spawn(move || {
                    let spinner_chars = ['|', '/', '-', '\\'];
                    let mut idx = 0;
                    let start = Instant::now();
                    while spinning.load(Ordering::Relaxed) {
                        print!("\rBuilding... {}", spinner_chars[idx % spinner_chars.len()]);
                        idx += 1;
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                        sleep(Duration::from_millis(100));
                    }
                    let elapsed = start.elapsed();
                    print!("\rBuild complete in {:.2?}        \n", elapsed);
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                })
            };

            // Capture output instead of hiding it
            let output = Command::new("cargo")
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .expect("Failed to invoke cargo build");

            spinning.store(false, Ordering::Relaxed);
            spinner_handle.join().ok();

            if !output.status.success() {
                eprintln!("\nERROR: Cargo build failed for pod '{}'.", pod.name);
                eprintln!("--- Cargo stdout ---");
                eprintln!("{}", String::from_utf8_lossy(&output.stdout));
                eprintln!("--- Cargo stderr ---");
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
                eprintln!("Aborting: No pods will be launched.");
                return;
            }
        }
        println!("\nAll pods built successfully.\n");
    }

    // === PHASE 2: LAUNCH ALL PODS IN ORDER ===
    let mut children: Vec<Child> = Vec::new();

    for (i, pod) in POD_CONFIG.iter().enumerate() {
        let profile = match pod.build_mode {
            BuildMode::Debug => "debug",
            BuildMode::Release => "release",
            BuildMode::External => {
                // Try release first, then debug
                if Path::new(&format!("./target/release/{}{}", pod.name, exe_suffix)).exists() {
                    "release"
                } else {
                    "debug"
                }
            }
        };

        let bin_path = format!("./target/{profile}/{}{}", pod.name, exe_suffix);

        println!("\nAttempting to launch: {bin_path}");

        let path = Path::new(&bin_path);

        // Stepwise path existence check for better diagnostics
        let mut current = PathBuf::new();
        let mut found = true;
        for comp in path.components() {
            current.push(comp);
            if !current.exists() {
                found = false;
                eprintln!("ERROR: Path component not found: {}", current.display());
                if let Some(parent) = current.parent() {
                    println!("Listing contents of parent: {}", parent.display());
                    match fs::read_dir(parent) {
                        Ok(entries) => {
                            for entry in entries.flatten() {
                                println!("  - {}", entry.file_name().to_string_lossy());
                            }
                        }
                        Err(e) => {
                            println!("  (Could not read directory: {e})");
                        }
                    }
                } else {
                    println!("No parent directory to list.");
                }
                break;
            }
        }

        if !found {
            continue;
        }

        // If the full path exists, try to launch
        match Command::new(&path).spawn() {
            Ok(child) => {
                println!("Launched: {bin_path}");
                children.push(child);
            }
            Err(e) => {
                eprintln!("ERROR: Failed to start {bin_path}: {e}");
            }
        }

        // Delay before launching the next pod, except after the last one
        if i + 1 < POD_CONFIG.len() {
            println!("Waiting {LAUNCH_DELAY_SECS} second(s) before launching next pod...");
            sleep(Duration::from_secs(LAUNCH_DELAY_SECS));
        }
    }

    // Wait for all children to finish
    for (i, mut child) in children.into_iter().enumerate() {
        match child.wait() {
            Ok(status) => println!("Process {i} exited with: {status}"),
            Err(e) => eprintln!("ERROR: Failed to wait for process {i}: {e}"),
        }
    }
}