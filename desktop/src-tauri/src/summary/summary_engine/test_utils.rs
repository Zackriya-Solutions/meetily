use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use once_cell::sync::{Lazy, OnceCell};

use crate::brand;

static TEST_ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static FAKE_HELPER_PATH: OnceCell<PathBuf> = OnceCell::new();

const FAKE_HELPER_SOURCE: &str = r####"
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

fn respond(line: &str) -> &'static str {
    if line.contains(r#""type":"ping""#) {
        r#"{"type":"pong"}"#
    } else if line.contains(r#""type":"shutdown""#) {
        r#"{"type":"goodbye"}"#
    } else if line.contains("TEST_TIMEOUT") || line.contains("TEST_CANCEL") {
        thread::sleep(Duration::from_secs(5));
        r#"{"type":"response","text":"late-response","error":null}"#
    } else if line.contains("TEST_SLOW") {
        thread::sleep(Duration::from_millis(250));
        r#"{"type":"response","text":"slow-response","error":null}"#
    } else {
        r#"{"type":"response","text":"ok-response","error":null}"#
    }
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let response = respond(&line);
        println!("{}", response);
        let _ = stdout.flush();

        if response.contains(r#""goodbye""#) {
            break;
        }
    }
}
"####;

pub(crate) fn test_env_lock() -> MutexGuard<'static, ()> {
    TEST_ENV_LOCK.lock().unwrap()
}

pub(crate) fn clear_test_env() {
    env::remove_var(brand::LLAMA_HELPER_ENV);
    env::remove_var(brand::LLAMA_HELPER_ALLOW_FUZZY_ENV);
    env::remove_var("LLAMA_IDLE_TIMEOUT");
}

pub(crate) fn set_fake_helper_env() {
    env::set_var(brand::LLAMA_HELPER_ENV, fake_helper_path());
}

fn fake_helper_path() -> PathBuf {
    FAKE_HELPER_PATH
        .get_or_init(|| build_fake_helper().expect("failed to build fake helper"))
        .clone()
}

fn build_fake_helper() -> std::io::Result<PathBuf> {
    let build_dir = env::temp_dir().join("meetfree-fake-helper");
    std::fs::create_dir_all(&build_dir)?;

    let source_path = build_dir.join("fake_helper.rs");
    std::fs::write(&source_path, FAKE_HELPER_SOURCE)?;

    let binary_name = if cfg!(windows) {
        "fake-helper.exe"
    } else {
        "fake-helper"
    };
    let binary_path = build_dir.join(binary_name);

    let status = Command::new("rustc")
        .arg("--edition=2021")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .status()?;

    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("rustc failed with status {status}"),
        ));
    }

    Ok(binary_path)
}
