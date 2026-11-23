//! Stages the ONNX Runtime DLL on Windows, fully offline.
//!
//! sherpa-onnx-sys's build script (`copy_windows_runtime_dlls`) already places
//! its prebuilt `onnxruntime.dll` (ORT 1.28.2, DirectML-capable) into the cargo
//! profile directory before this app's build script runs (dependency build
//! scripts run first). We copy that exact DLL into `binaries/onnxruntime/` and
//! verify it against pinned size/SHA-256 constants — no network access.
//!
//! Only `onnxruntime.dll` is staged. `onnxruntime_providers_shared.dll` is
//! deliberately NOT staged: the 1.28.2 DLL does not import it, and CI
//! (.github/verify-windows-dlls.ps1) enforces exactly one `onnxruntime*.dll`.

use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::Read,
    path::Path,
    process,
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[allow(dead_code)]
mod build_env {
    include!("../src/build_env.rs");
}

const WINDOWS_X64_TARGET: &str = "x86_64-pc-windows-msvc";
const DLL_NAME: &str = "onnxruntime.dll";
/// Pinned from the sherpa-onnx v1.13.8 win-x64 shared prebuilt (ORT 1.28.2).
const DLL_SIZE: u64 = 17_799_168;
const DLL_SHA256: &str = "7f66f939a881baf4f46a2216496798edf4a1429878b646d12674aa62f27d8a25";

pub fn ensure_onnxruntime_runtime() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=binaries/onnxruntime");

    let target = env::var("TARGET").expect("TARGET environment variable not set");
    if target != WINDOWS_X64_TARGET {
        panic!("ONNX Runtime is bundled only for {WINDOWS_X64_TARGET}; got {target}");
    }

    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR environment variable not set");
    let destination = Path::new(&manifest_dir)
        .join("binaries")
        .join("onnxruntime");

    match verify_staged_runtime(&destination) {
        Ok(()) => {
            println!(
                "cargo:warning=Using verified bundled ONNX Runtime from {}",
                destination.display()
            );
            return;
        }
        Err(verification_error) => match fs::symlink_metadata(&destination) {
            Ok(metadata) if is_link_or_reparse_point(&metadata) => {
                panic!(
                    "Refusing to replace linked or reparse-point ONNX Runtime stage at {}: {verification_error}",
                    destination.display()
                );
            }
            Ok(metadata) if metadata.is_dir() => {
                println!(
                    "cargo:warning=Replacing invalid bundled ONNX Runtime: {verification_error}"
                );
                fs::remove_dir_all(&destination).unwrap_or_else(|remove_error| {
                    panic!(
                        "Failed to remove invalid ONNX Runtime directory at {}: {remove_error}",
                        destination.display()
                    )
                });
            }
            Ok(metadata) if metadata.is_file() => {
                println!(
                    "cargo:warning=Replacing invalid bundled ONNX Runtime: {verification_error}"
                );
                fs::remove_file(&destination).unwrap_or_else(|remove_error| {
                    panic!(
                        "Failed to remove invalid ONNX Runtime file at {}: {remove_error}",
                        destination.display()
                    )
                });
            }
            Ok(_) => {
                panic!(
                    "Refusing to replace special ONNX Runtime stage at {}: {verification_error}",
                    destination.display()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                panic!(
                    "Failed to inspect invalid ONNX Runtime stage at {}: {error}",
                    destination.display()
                );
            }
        },
    }

    // Resolve the source DLL via the same OUT_DIR-ancestor walk as
    // build/sherpa.rs: OUT_DIR is target/<triple>/<profile>/build/<crate>/out
    // (with --target) or target/<profile>/build/<crate>/out (native), so this
    // covers both target/<profile>/onnxruntime.dll and
    // target/x86_64-pc-windows-msvc/<profile>/onnxruntime.dll.
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable not set");
    let profile = build_env::expect_profile(env::var("PROFILE"));
    let profile_dir = build_env::resolve_profile_dir(Path::new(&out_dir), &profile);
    let source = profile_dir.join(DLL_NAME);

    fs::create_dir_all(
        destination
            .parent()
            .expect("ONNX Runtime destination has no parent"),
    )
    .expect("Failed to create ONNX Runtime binaries directory");

    let temporary_destination =
        destination.with_file_name(format!(".onnxruntime-{}-{}", process::id(), target));
    let _ = fs::remove_dir_all(&temporary_destination);

    let result = stage_runtime(&source, &temporary_destination);

    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary_destination);
        panic!("Failed to stage bundled ONNX Runtime: {error}");
    }

    fs::rename(&temporary_destination, &destination).unwrap_or_else(|error| {
        let _ = fs::remove_dir_all(&temporary_destination);
        panic!(
            "Failed to finalize bundled ONNX Runtime at {}: {error}",
            destination.display()
        );
    });

    verify_staged_runtime(&destination).unwrap_or_else(|error| {
        panic!("Bundled ONNX Runtime verification failed after staging: {error}")
    });
    println!(
        "cargo:warning=Bundled verified ONNX Runtime at {}",
        destination.display()
    );
}

/// Copies the single prebuilt `onnxruntime.dll` from the cargo profile dir
/// into the temporary staging directory and verifies it before the atomic
/// rename. Never copies `onnxruntime_providers_shared.dll` or any other file.
fn stage_runtime(source: &Path, destination: &Path) -> Result<(), String> {
    let source_metadata = fs::symlink_metadata(source).map_err(|error| {
        format!(
            "failed to inspect source {} (sherpa-onnx-sys must stage it before this build script runs): {error}",
            source.display()
        )
    })?;
    if is_link_or_reparse_point(&source_metadata) {
        return Err(format!(
            "source {} is a link or reparse point; refusing to follow it",
            source.display()
        ));
    }
    if !source_metadata.is_file() {
        return Err(format!("source {} is not a regular file", source.display()));
    }

    fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;

    let output = destination.join(DLL_NAME);
    fs::copy(source, &output).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            source.display(),
            output.display()
        )
    })?;
    verify_file(&output, DLL_NAME, DLL_SIZE, DLL_SHA256)?;

    Ok(())
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        return metadata.file_attributes() & 0x0000_0400 != 0;
    }

    #[cfg(not(windows))]
    false
}

fn verify_staged_runtime(destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(destination)
        .map_err(|error| format!("failed to inspect runtime stage: {error}"))?;
    if is_link_or_reparse_point(&metadata) {
        return Err("runtime stage is a link or reparse point".to_string());
    }
    if !metadata.file_type().is_dir() {
        return Err("runtime stage is not a directory".to_string());
    }

    let entries = fs::read_dir(destination)
        .map_err(|error| format!("failed to enumerate runtime stage: {error}"))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to enumerate runtime stage entry: {error}"))?;
        let entry_metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!(
                "failed to inspect runtime stage entry {}: {error}",
                entry.path().display()
            )
        })?;
        if is_link_or_reparse_point(&entry_metadata) {
            return Err(format!(
                "runtime stage entry {} is a link or reparse point",
                entry.path().display()
            ));
        }
        if !entry_metadata.file_type().is_file() {
            return Err(format!(
                "runtime stage entry {} is not a regular file",
                entry.path().display()
            ));
        }

        // Exactly one staged file is allowed: onnxruntime.dll. Anything else
        // (e.g. a stray onnxruntime_providers_shared.dll) invalidates the stage.
        if entry.file_name().as_os_str() != DLL_NAME {
            return Err(format!(
                "runtime stage contains undeclared entry {}",
                entry.path().display()
            ));
        }
    }

    verify_file(&destination.join(DLL_NAME), DLL_NAME, DLL_SIZE, DLL_SHA256)
}

fn verify_file(
    path: &Path,
    name: &str,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("failed to inspect {name}: {error}"))?;
    if metadata.len() != expected_size {
        return Err(format!(
            "{name} has size {}, expected {expected_size}",
            metadata.len()
        ));
    }

    let mut file = File::open(path).map_err(|error| format!("failed to open {name}: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash {name}: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "{name} has SHA-256 {actual_sha256}, expected {expected_sha256}"
        ));
    }

    Ok(())
}