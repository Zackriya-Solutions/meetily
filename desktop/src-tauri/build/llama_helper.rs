use std::path::{Path, PathBuf};

pub fn ensure_llama_helper_binary() {
    let target = std::env::var("TARGET")
        .or_else(|_| std::env::var("HOST"))
        .expect("Neither TARGET nor HOST environment variable set");
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR environment variable not set"),
    );
    let binaries_dir = manifest_dir.join("binaries");
    let bundled_binary = binaries_dir.join(sidecar_name(&target));

    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("../..").join("llama-helper/Cargo.toml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("../..").join("llama-helper/src/main.rs").display()
    );

    if bundled_binary.exists() {
        return;
    }

    std::fs::create_dir_all(&binaries_dir).expect("Failed to create binaries directory");

    if let Some(existing) = find_existing_sidecar(&manifest_dir, &target, &profile) {
        copy_sidecar(&existing, &bundled_binary);
        return;
    }

    panic!(
        "llama-helper sidecar is missing for target {}. Build it first with 'cargo build -p llama-helper' and then rerun the Tauri build.",
        target,
    );
}

fn find_existing_sidecar(manifest_dir: &Path, target: &str, profile: &str) -> Option<PathBuf> {
    let workspace_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("Failed to determine workspace root");
    let sidecar_target_dir = workspace_root.join("target").join("llama-helper-sidecar");
    let binary_file = if target.contains("windows") {
        "llama-helper.exe"
    } else {
        "llama-helper"
    };

    let candidates = [
        workspace_root.join("target").join(profile).join(binary_file),
        workspace_root
            .join("target")
            .join(target)
            .join(profile)
            .join(binary_file),
        sidecar_target_dir.join(profile).join(binary_file),
        sidecar_target_dir.join(target).join(profile).join(binary_file),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn copy_sidecar(source: &Path, destination: &Path) {
    std::fs::copy(source, destination).unwrap_or_else(|error| {
        panic!(
            "Failed to copy llama-helper sidecar from {} to {}: {error}",
            source.display(),
            destination.display()
        )
    });
}

fn sidecar_name(target: &str) -> String {
    if target.contains("windows") {
        format!("llama-helper-{target}.exe")
    } else {
        format!("llama-helper-{target}")
    }
}
