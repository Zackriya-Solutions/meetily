// ============================================================================
// FFmpeg Binary Bundling
// ============================================================================

/// Download and bundle FFmpeg binary for current target platform.
/// Checks the local cached binary first and only emits output when work is needed.
pub fn ensure_ffmpeg_binary() {
    let target = std::env::var("TARGET")
        .or_else(|_| std::env::var("HOST"))
        .expect("Neither TARGET nor HOST environment variable set");

    let binary_name = if target.contains("windows") {
        format!("ffmpeg-{}.exe", target)
    } else {
        format!("ffmpeg-{}", target)
    };

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable not set");
    let binaries_dir = std::path::PathBuf::from(&manifest_dir).join("binaries");
    let binary_path = binaries_dir.join(&binary_name);

    if binary_path.exists() {
        if verify_ffmpeg_binary(&binary_path) {
            return;
        }

        println!("cargo:warning=FFmpeg binary is invalid, re-downloading");
        let _ = std::fs::remove_file(&binary_path);
    }

    println!("cargo:warning=Downloading FFmpeg binary for {}", target);

    if !binaries_dir.exists() {
        std::fs::create_dir_all(&binaries_dir).expect("Failed to create binaries directory");
    }

    match download_and_extract_ffmpeg(&target, &binary_path) {
        Ok(()) => {
            if !verify_ffmpeg_binary(&binary_path) {
                panic!("Downloaded FFmpeg binary verification failed");
            }
        }
        Err(e) => {
            panic!("Failed to download FFmpeg: {}", e);
        }
    }
}

fn download_and_extract_ffmpeg(
    target: &str,
    output_path: &std::path::PathBuf,
) -> Result<(), String> {
    use std::io::Write;

    let url = get_ffmpeg_url_for_target(target)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .map_err(|e| format!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let temp_dir = std::env::temp_dir();
    let archive_filename = url.split('/').last().unwrap_or("ffmpeg-archive");
    let archive_path = temp_dir.join(format!("ffmpeg-build-{}-{}", target, archive_filename));

    {
        let mut file = std::fs::File::create(&archive_path)
            .map_err(|e| format!("Failed to create temp file: {}", e))?;

        let content = response
            .bytes()
            .map_err(|e| format!("Failed to read response: {}", e))?;

        file.write_all(&content)
            .map_err(|e| format!("Failed to write archive: {}", e))?;
    }

    extract_ffmpeg_from_archive(&archive_path, target, output_path)?;

    let _ = std::fs::remove_file(&archive_path);

    Ok(())
}

fn get_ffmpeg_url_for_target(target: &str) -> Result<String, String> {
    let url = if target.contains("windows") {
        "https://github.com/Zackriya-Solutions/ffmpeg-binaries/releases/download/0.0.1/ffmpeg-8.0.1-essentials_build.zip"
    } else if target.contains("apple") {
        if target.contains("aarch64") {
            "https://github.com/Zackriya-Solutions/ffmpeg-binaries/releases/download/0.0.1/ffmpeg80arm.zip"
        } else {
            "https://github.com/Zackriya-Solutions/ffmpeg-binaries/releases/download/0.0.1/ffmpeg-8.0.1.zip"
        }
    } else if target.contains("linux") {
        if target.contains("aarch64") || target.contains("arm") {
            "https://github.com/Zackriya-Solutions/ffmpeg-binaries/releases/download/0.0.1/ffmpeg-release-arm64-static.tar.xz"
        } else {
            "https://github.com/Zackriya-Solutions/ffmpeg-binaries/releases/download/0.0.1/ffmpeg-release-amd64-static.tar.xz"
        }
    } else {
        return Err(format!("Unsupported target platform: {}", target));
    };

    Ok(url.to_string())
}

fn extract_ffmpeg_from_archive(
    archive_path: &std::path::Path,
    target: &str,
    output_path: &std::path::PathBuf,
) -> Result<(), String> {
    let extract_dir = std::env::temp_dir().join(format!("ffmpeg-extract-{}", target));

    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)
        .map_err(|e| format!("Failed to create extract dir: {}", e))?;

    let archive_str = archive_path.to_string_lossy();

    if archive_str.ends_with(".zip") {
        extract_zip(archive_path, &extract_dir)?;
    } else if archive_str.ends_with(".tar.xz") || archive_str.ends_with(".txz") {
        extract_tar_xz(archive_path, &extract_dir)?;
    } else {
        return Err(format!("Unsupported archive format: {}", archive_str));
    }

    let ffmpeg_binary = find_ffmpeg_in_extracted_dir(&extract_dir, target)?;

    std::fs::copy(&ffmpeg_binary, output_path)
        .map_err(|e| format!("Failed to copy binary to binaries/: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(output_path)
            .map_err(|e| format!("Failed to get metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(output_path, perms)
            .map_err(|e| format!("Failed to set executable permissions: {}", e))?;
    }

    let _ = std::fs::remove_dir_all(&extract_dir);

    Ok(())
}

fn extract_zip(
    archive_path: &std::path::Path,
    extract_dir: &std::path::Path,
) -> Result<(), String> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("Failed to open ZIP: {}", e))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read ZIP archive: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry {}: {}", i, e))?;

        let outpath = match file.enclosed_name() {
            Some(name) => extract_dir.join(name),
            None => {
                println!("cargo:warning=Skipping suspicious ZIP entry: {}", file.name());
                continue;
            }
        };

        if file.is_dir() {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent directory: {}", e))?;
            }

            let mut outfile = std::fs::File::create(&outpath)
                .map_err(|e| format!("Failed to create output file: {}", e))?;

            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to extract file: {}", e))?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode)).ok();
            }
        }
    }

    Ok(())
}

fn extract_tar_xz(
    archive_path: &std::path::Path,
    extract_dir: &std::path::Path,
) -> Result<(), String> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("Failed to open TAR.XZ: {}", e))?;

    let decompressor = xz2::read::XzDecoder::new(file);
    let mut archive = tar::Archive::new(decompressor);
    archive
        .unpack(extract_dir)
        .map_err(|e| format!("Failed to extract TAR: {}", e))?;

    Ok(())
}

fn find_ffmpeg_in_extracted_dir(
    extract_dir: &std::path::Path,
    target: &str,
) -> Result<std::path::PathBuf, String> {
    let executable_name = if target.contains("windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };

    let search_patterns = [
        extract_dir.join(executable_name),
        extract_dir.join("bin").join(executable_name),
    ];

    for pattern in &search_patterns {
        if pattern.exists() && pattern.is_file() {
            return Ok(pattern.clone());
        }
    }

    for entry in std::fs::read_dir(extract_dir)
        .map_err(|e| format!("Failed to read extract dir: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            let bin_path = path.join("bin").join(executable_name);
            if bin_path.exists() && bin_path.is_file() {
                return Ok(bin_path);
            }

            let root_path = path.join(executable_name);
            if root_path.exists() && root_path.is_file() {
                return Ok(root_path);
            }
        }
    }

    Err(format!(
        "FFmpeg binary '{}' not found in extracted archive",
        executable_name
    ))
}

fn verify_ffmpeg_binary(path: &std::path::PathBuf) -> bool {
    match std::process::Command::new(path).arg("-version").output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}
