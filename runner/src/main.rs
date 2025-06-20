use std::process::{Command, Child};
use std::path::{Path, PathBuf};
use std::fs;
use std::thread::sleep;
use std::time::Duration;

const BINARIES: &[&str] = &["publish", "subscribe"];
const LAUNCH_DELAY_SECS: u64 = 3; // Default delay between launches

fn main() {
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };

    println!(
        "Runner starting with profile: {profile}, exe_suffix: {exe_suffix}, delay: {LAUNCH_DELAY_SECS}s"
    );

    let mut children: Vec<Child> = Vec::new();

    for (i, &bin) in BINARIES.iter().enumerate() {
        // let use_profile = if "subscribe".eq(bin) {
        //     "release"
        // } else {
        //     profile
        // };
        let path_str = format!("./target/{profile}/{bin}{exe_suffix}");
        println!("\nAttempting to launch: {path_str}");

        let path = Path::new(&path_str);

        // Stepwise path existence check
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
                println!("Launched: {path_str}");
                children.push(child);
            }
            Err(e) => {
                eprintln!("ERROR: Failed to start {path_str}: {e}");
            }
        }

        // Delay before launching the next binary, except after the last one
        if i + 1 < BINARIES.len() {
            println!("Waiting {LAUNCH_DELAY_SECS} second(s) before launching next binary...");
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