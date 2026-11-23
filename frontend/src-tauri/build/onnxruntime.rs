use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process,
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

// Official Microsoft.ML.OnnxRuntime NuGet package (a zip); it contains both the win-x64 and win-arm64 runtimes.
const ARCHIVE_URL: &str = "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/1.22.0/microsoft.ml.onnxruntime.1.22.0.nupkg";
const ARCHIVE_SHA256: &str = "d571e63a2329baacb713f441e65ad75284de354db6e1ac435fe4bebbb417986a";
const ARCHIVE_SIZE: u64 = 121_402_697;

struct Artifact {
    archive_path: &'static str,
    output_name: &'static str,
    size: u64,
    sha256: &'static str,
}

const X64_ARTIFACTS: [Artifact; 3] = [
    Artifact {
        archive_path: "runtimes/win-x64/native/onnxruntime.dll",
        output_name: "onnxruntime.dll",
        size: 12_418_080,
        sha256: "579b636403983254346a5c1d80bd28f1519cd1e284cd204f8d4ff41f8d711559",
    },
    Artifact {
        archive_path: "runtimes/win-x64/native/onnxruntime_providers_shared.dll",
        output_name: "onnxruntime_providers_shared.dll",
        size: 22_064,
        sha256: "ba00ea1ef846c9b909c7854bc56c51051a20f9773b3e1153dda118d4b85d0b93",
    },
    Artifact {
        archive_path: "LICENSE",
        output_name: "onnxruntime-LICENSE.txt",
        size: 1_094,
        sha256: "c250d6278f0b47a6439fb7592b08b58a55eb9f535aa49a1db63211c3f982b674",
    },
];

const ARM64_ARTIFACTS: [Artifact; 3] = [
    Artifact {
        archive_path: "runtimes/win-arm64/native/onnxruntime.dll",
        output_name: "onnxruntime.dll",
        size: 12_679_728,
        sha256: "79281671a386ed1baab9dbdbb09fe55f99577011472e9526cf9d0b468bb6bcc7",
    },
    Artifact {
        archive_path: "runtimes/win-arm64/native/onnxruntime_providers_shared.dll",
        output_name: "onnxruntime_providers_shared.dll",
        size: 21_048,
        sha256: "e80bf226d099fc040f4f18b5d65aa1ae33bcb3f5821f9570b866c4a64e410c83",
    },
    Artifact {
        archive_path: "LICENSE",
        output_name: "onnxruntime-LICENSE.txt",
        size: 1_094,
        sha256: "c250d6278f0b47a6439fb7592b08b58a55eb9f535aa49a1db63211c3f982b674",
    },
];

pub fn ensure_onnxruntime_runtime() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=binaries/onnxruntime");

    let target = env::var("TARGET").expect("TARGET environment variable not set");
    let artifacts = match target.as_str() {
        "x86_64-pc-windows-msvc" => &X64_ARTIFACTS,
        "aarch64-pc-windows-msvc" => &ARM64_ARTIFACTS,
        _ => panic!("ONNX Runtime is not bundled for {target}"),
    };

    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR environment variable not set");
    let destination = Path::new(&manifest_dir)
        .join("binaries")
        .join("onnxruntime");

    match verify_staged_runtime(artifacts, &destination) {
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

    fs::create_dir_all(
        destination
            .parent()
            .expect("ONNX Runtime destination has no parent"),
    )
    .expect("Failed to create ONNX Runtime binaries directory");

    let temporary_archive = env::temp_dir().join(format!(
        "meetily-onnxruntime-{}-{}.zip",
        process::id(),
        target
    ));
    let temporary_destination =
        destination.with_file_name(format!(".onnxruntime-{}-{}", process::id(), target));
    let _ = fs::remove_file(&temporary_archive);
    let _ = fs::remove_dir_all(&temporary_destination);

    let result = stage_runtime(artifacts, &temporary_archive, &temporary_destination);
    let _ = fs::remove_file(&temporary_archive);

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

    verify_staged_runtime(artifacts, &destination).unwrap_or_else(|error| {
        panic!("Bundled ONNX Runtime verification failed after staging: {error}")
    });
    println!(
        "cargo:warning=Bundled verified ONNX Runtime at {}",
        destination.display()
    );
}

fn stage_runtime(artifacts: &[Artifact], archive_path: &Path, destination: &Path) -> Result<(), String> {
    download_archive(archive_path)?;
    verify_file(archive_path, ARCHIVE_URL, ARCHIVE_SIZE, ARCHIVE_SHA256)?;

    fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;

    let archive_file = File::open(archive_path)
        .map_err(|error| format!("failed to open {}: {error}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|error| format!("failed to read ONNX Runtime archive: {error}"))?;

    for artifact in artifacts {
        let mut source = archive.by_name(artifact.archive_path).map_err(|error| {
            format!(
                "missing {} in ONNX Runtime archive: {error}",
                artifact.archive_path
            )
        })?;
        let output = destination.join(artifact.output_name);
        let mut file = File::create(&output)
            .map_err(|error| format!("failed to create {}: {error}", output.display()))?;
        std::io::copy(&mut source, &mut file)
            .map_err(|error| format!("failed to extract {}: {error}", artifact.archive_path))?;
        verify_file(
            &output,
            artifact.output_name,
            artifact.size,
            artifact.sha256,
        )?;
    }

    Ok(())
}

fn download_archive(destination: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|error| format!("failed to create download client: {error}"))?;
    let mut response = client
        .get(ARCHIVE_URL)
        .send()
        .map_err(|error| format!("failed to download ONNX Runtime: {error}"))?
        .error_for_status()
        .map_err(|error| format!("ONNX Runtime download failed: {error}"))?;
    let mut file = File::create(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    copy_exact(&mut response, &mut file, ARCHIVE_SIZE)
}

fn copy_exact<R: Read, W: Write>(
    source: &mut R,
    destination: &mut W,
    expected_size: u64,
) -> Result<(), String> {
    let copied = std::io::copy(
        &mut source.by_ref().take(expected_size),
        destination,
    )
    .map_err(|error| format!("failed to copy ONNX Runtime download: {error}"))?;
    if copied != expected_size {
        return Err(format!(
            "downloaded ONNX Runtime archive has size {copied}, expected {expected_size}"
        ));
    }

    let mut probe = [0_u8; 1];
    if source
        .read(&mut probe)
        .map_err(|error| format!("failed to copy ONNX Runtime download: {error}"))?
        != 0
    {
        return Err(format!(
            "downloaded ONNX Runtime archive exceeds expected size {expected_size}"
        ));
    }

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

fn verify_staged_runtime(artifacts: &[Artifact], destination: &Path) -> Result<(), String> {
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

        let name = entry.file_name();
        if !artifacts
            .iter()
            .any(|artifact| name.as_os_str() == artifact.output_name)
        {
            return Err(format!(
                "runtime stage contains undeclared entry {}",
                entry.path().display()
            ));
        }
    }

    for artifact in artifacts {
        verify_file(
            &destination.join(artifact.output_name),
            artifact.output_name,
            artifact.size,
            artifact.sha256,
        )?;
    }
    Ok(())
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
