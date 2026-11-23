//! Stages the sherpa-onnx SHARED runtime DLL on Windows.
//!
//! sherpa-onnx's prebuilt Windows STATIC libraries embed the /MT CRT, which
//! collides with the /MD CRT used by the Rust app (and whisper), producing
//! LNK2038/LNK2005/LNK1169 link errors. On Windows we therefore build against
//! the `shared` feature and stage ONLY `sherpa-onnx-c-api.dll` here.
//! It imports just `OrtGetApiBase@ordinal1` from onnxruntime, satisfied by the
//! sherpa-onnx v1.13.8 prebuilt onnxruntime.dll (ORT 1.28.2), which
//! build/onnxruntime.rs stages offline into `binaries/onnxruntime/` against a
//! pinned size/SHA-256. This module itself copies no onnxruntime DLL.

use std::path::{Path, PathBuf};

#[allow(dead_code)]
mod build_env {
    include!("../src/build_env.rs");
}

use build_env::{
    describe_prebuilt_search, expect_profile, find_prebuilt_dir, resolve_profile_dir,
    staged_file_state, StagedFileState,
};

fn is_file_checked(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => panic!("Failed to inspect {}: {}", path.display(), error),
    }
}

fn resolve_prebuilt_root(profile_dir: &Path) -> PathBuf {
    if let Some(override_dir) = std::env::var_os("SHERPA_ONNX_PREBUILT") {
        let dir = PathBuf::from(override_dir);
        return match std::fs::metadata(&dir) {
            Ok(metadata) if metadata.is_dir() => dir,
            Ok(_) => panic!(
                "SHERPA_ONNX_PREBUILT={} is not a directory; point it at the prebuilt \
                 sherpa-onnx cache root (the directory containing sherpa-onnx-c-api.dll \
                 or <archive>/lib/sherpa-onnx-c-api.dll)",
                dir.display()
            ),
            Err(error) => panic!(
                "Failed to inspect SHERPA_ONNX_PREBUILT={}: {}",
                dir.display(),
                error
            ),
        };
    }

    find_prebuilt_dir(profile_dir).unwrap_or_else(|| {
        panic!(
            "Could not find a `sherpa-onnx-prebuilt` directory in any ancestor of {} \
             (searched: {}). Fix: set SHERPA_ONNX_PREBUILT to the directory containing the \
             prebuilt sherpa-onnx-c-api.dll, or run the sherpa-onnx prebuilt download step.",
            profile_dir.display(),
            describe_prebuilt_search(profile_dir)
        )
    })
}

/// Locates `sherpa-onnx-c-api.dll` inside sherpa-onnx-sys's prebuilt cache
/// (`<target>/sherpa-onnx-prebuilt/<archive>/lib/`), preferring the shared
/// archive. Returns None if the cache exists but holds no matching DLL.
fn find_prebuilt_dll(profile_dir: &Path) -> Option<PathBuf> {
    let prebuilt = resolve_prebuilt_root(profile_dir);

    let direct = prebuilt.join("sherpa-onnx-c-api.dll");
    if is_file_checked(&direct) {
        return Some(direct);
    }

    let mut shared: Option<PathBuf> = None;
    let mut any: Option<PathBuf> = None;
    let entries = match std::fs::read_dir(&prebuilt) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "cargo:warning=sherpa-onnx prebuilt cache {} disappeared while staging; \
                 continuing without a fallback copy",
                prebuilt.display()
            );
            return None;
        }
        Err(error) => panic!(
            "Failed to read sherpa-onnx prebuilt cache directory {}: {}",
            prebuilt.display(),
            error
        ),
    };

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "Failed to read an entry in sherpa-onnx prebuilt cache {}: {}",
                prebuilt.display(),
                error
            )
        });
        let lib = entry.path().join("lib").join("sherpa-onnx-c-api.dll");
        if is_file_checked(&lib) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains("shared") {
                shared = Some(lib);
            } else if any.is_none() {
                any = Some(lib);
            }
        }
    }
    shared.or(any)
}

/// Copies `sherpa-onnx-c-api.dll` from the cargo profile dir into
/// `binaries/sherpa/` so Tauri bundles it as a resource. Windows-only;
/// a no-op on every other target OS.
pub fn stage_sherpa_dlls() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // OUT_DIR looks like target/<triple>/<profile>/build/<crate>/out (with
    // --target) or target/<profile>/build/<crate>/out (without); walking
    // ancestors finds the profile dir in both layouts.
    let out_dir =
        std::env::var("OUT_DIR").expect("OUT_DIR env var not set by cargo; cannot stage DLLs");
    let profile = expect_profile(std::env::var("PROFILE"));
    let profile_dir = resolve_profile_dir(Path::new(&out_dir), &profile);

    let src = profile_dir.join("sherpa-onnx-c-api.dll");
    let dst_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/sherpa");
    std::fs::create_dir_all(&dst_dir)
        .expect("Failed to create binaries/sherpa staging directory");
    let dst = dst_dir.join("sherpa-onnx-c-api.dll");

    // Skip identical copies: under `tauri dev` the DLL may be loaded and
    // locked by a running app, so a redundant copy would fail for no reason.
    // Compare size AND mtime so a same-size rebuild is not mistaken for fresh.
    if dst.exists() {
        if let (Ok(src_meta), Ok(dst_meta)) = (std::fs::metadata(&src), std::fs::metadata(&dst))
        {
            if staged_file_state(&src_meta, &dst_meta) == StagedFileState::Fresh {
                return;
            }
        }
    }

    // Copy only sherpa-onnx-c-api.dll here; never glob *.dll. The matching
    // onnxruntime.dll (sherpa-onnx v1.13.8 prebuilt, ORT 1.28.2) is staged
    // offline by build/onnxruntime.rs into binaries/onnxruntime/ with a pinned
    // size/SHA-256; onnxruntime_providers_shared.dll is not bundled.
    if let Err(err) = std::fs::copy(&src, &dst) {
        if dst.exists() {
            println!(
                "cargo:warning=failed to update staged sherpa-onnx-c-api.dll ({}); keeping existing copy at {}",
                err,
                dst.display()
            );
        } else if profile == "debug" {
            println!(
                "cargo:warning=debug build: no sherpa-onnx-c-api.dll at {} and no staged copy at {}; skipping sherpa staging",
                src.display(),
                dst.display()
            );
        } else {
            // The profile-dir copy can be missing when the rust-cache restore
            // is partial (sherpa-onnx-sys's build script is cached and does not
            // re-copy its runtime DLLs). Fall back to the prebuilt cache.
            if let Some(prebuilt_src) = find_prebuilt_dll(&profile_dir) {
                if let Err(fb_err) = std::fs::copy(&prebuilt_src, &dst) {
                    panic!(
                        "Failed to stage sherpa-onnx-c-api.dll from prebuilt cache {}: {}. \
                         The `sherpa-onnx` `shared` feature must be enabled for Windows (check \
                         the [target.'cfg(target_os = \"windows\")'.dependencies] sherpa-onnx \
                         entry in Cargo.toml). Fix: set SHERPA_ONNX_PREBUILT to the directory \
                         containing the prebuilt sherpa-onnx-c-api.dll, or run the sherpa-onnx \
                         prebuilt download step.",
                        prebuilt_src.display(),
                        fb_err
                    );
                }
                println!(
                    "cargo:warning=staged sherpa-onnx-c-api.dll from prebuilt cache {}",
                    prebuilt_src.display()
                );
            } else {
                panic!(
                    "Failed to stage sherpa-onnx-c-api.dll: source {} not found ({}). \
                     The `sherpa-onnx` `shared` feature must be enabled for Windows (check the \
                     [target.'cfg(target_os = \"windows\")'.dependencies] sherpa-onnx entry in \
                     Cargo.toml). Fix: set SHERPA_ONNX_PREBUILT to the directory containing the \
                     prebuilt sherpa-onnx-c-api.dll, or run the sherpa-onnx prebuilt download step.",
                    src.display(),
                    err
                );
            }
        }
    }
}
