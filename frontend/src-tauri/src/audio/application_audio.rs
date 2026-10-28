//! macOS application discovery. Every snapshot resolves fresh PIDs and HAL object
//! IDs; only bundle identity survives between snapshots or recording sessions.
use super::source::{ApplicationIdentity, AudioApplication};
use anyhow::{anyhow, Result};
use cidre::{core_audio as ca, ns};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub fn is_supported() -> bool {
    let version = ns::ProcessInfo::current().os_version();
    (version.major, version.minor) >= (14, 2)
        && objc::runtime::Class::get("CATapDescription").is_some()
}

#[derive(Clone)]
struct App {
    identity: ApplicationIdentity,
    pid: u32,
}
#[derive(Clone)]
struct Process {
    parent: Option<u32>,
    executable: Option<PathBuf>,
}

// Require ownership evidence, not a name or a historical PID. Stop at another
// application's bundle so launching an unrelated app does not include its audio.
fn belongs_to_app(pid: u32, app: &App, processes: &HashMap<u32, Process>) -> bool {
    let mut next = Some(pid);
    let mut visited = HashSet::new();
    while let Some(pid) = next {
        if !visited.insert(pid) {
            return false;
        }
        if pid == app.pid {
            return true;
        }
        let Some(process) = processes.get(&pid) else {
            return false;
        };
        if let Some(exe) = &process.executable {
            if exe.starts_with(Path::new(&app.identity.bundle_path)) {
                return true;
            }
            if exe
                .components()
                .any(|c| c.as_os_str().to_string_lossy().ends_with(".app"))
            {
                return false;
            }
        }
        next = process.parent;
    }
    false
}

struct Snapshot {
    apps: Vec<App>,
    processes: HashMap<u32, Process>,
    audio: Vec<(u32, u32, bool)>, // HAL object ID, PID, output activity
}

fn snapshot() -> Result<Snapshot> {
    anyhow::ensure!(
        is_supported(),
        "Application audio requires macOS 14.2 or later."
    );
    objc::rc::autoreleasepool(|| {
        let mut apps = Vec::new();
        for app in ns::Workspace::shared().running_apps().iter() {
            if app.pid() as u32 == std::process::id() || app.is_terminated() {
                continue;
            }
            let (Some(bundle), Some(url)) = (app.bundle_id(), app.bundle_url()) else {
                continue;
            };
            let Some(path) = url.path() else {
                continue;
            };
            let bundle_path = path.to_string();
            // Helpers nested inside a main app are represented by that main app.
            if Path::new(&bundle_path)
                .components()
                .filter(|c| c.as_os_str().to_string_lossy().ends_with(".app"))
                .count()
                != 1
            {
                continue;
            }
            apps.push(App {
                pid: app.pid() as u32,
                identity: ApplicationIdentity {
                    bundle_id: bundle.to_string(),
                    bundle_path,
                    name: app
                        .localized_name()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| bundle.to_string()),
                },
            });
        }
        let mut system = sysinfo::System::new();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::new().with_exe(sysinfo::UpdateKind::Always),
        );
        let processes = system
            .processes()
            .iter()
            .map(|(pid, p)| {
                (
                    pid.as_u32(),
                    Process {
                        parent: p.parent().map(|p| p.as_u32()),
                        executable: p.exe().map(Path::to_path_buf),
                    },
                )
            })
            .collect();
        let audio = ca::System::processes()
            .map_err(|e| anyhow!("Cannot list application audio: {e:?}"))?
            .into_iter()
            .filter_map(|p| {
                p.pid()
                    .ok()
                    .map(|pid| (p.0 .0, pid as u32, p.is_running_output().unwrap_or(false)))
            })
            .collect();
        Ok(Snapshot {
            apps,
            processes,
            audio,
        })
    })
}

pub fn list_applications() -> Result<Vec<AudioApplication>> {
    let snapshot = snapshot()?;
    let mut result: Vec<_> = snapshot
        .apps
        .iter()
        .map(|app| AudioApplication {
            identity: app.identity.clone(),
            audio_active: snapshot
                .audio
                .iter()
                .any(|(_, pid, active)| *active && belongs_to_app(*pid, app, &snapshot.processes)),
        })
        .collect();
    result.sort_by_key(|a| {
        (
            a.identity.name.to_lowercase(),
            a.identity.bundle_path.clone(),
        )
    });
    result.dedup_by(|a, b| {
        a.identity.bundle_id == b.identity.bundle_id
            && a.identity.bundle_path == b.identity.bundle_path
    });
    Ok(result)
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedApplication {
    pub running: bool,
    // Include PID in the generation key as HAL IDs can be reused too.
    pub processes: Vec<(u32, u32)>,
    pub output_device: u32,
    pub output_sample_rate: u32,
}

pub fn resolve(identity: &ApplicationIdentity) -> Result<ResolvedApplication> {
    let snapshot = snapshot()?;
    let apps: Vec<_> = snapshot
        .apps
        .iter()
        .filter(|app| {
            app.identity.bundle_id == identity.bundle_id
                && app.identity.bundle_path == identity.bundle_path
        })
        .collect();
    let mut processes: Vec<_> = snapshot
        .audio
        .iter()
        .filter(|(_, pid, _)| {
            apps.iter()
                .any(|app| belongs_to_app(*pid, app, &snapshot.processes))
        })
        .map(|(id, pid, _)| (*id, *pid))
        .collect();
    processes.sort_unstable();
    processes.dedup();
    let output = ca::System::default_output_device()
        .map_err(|e| anyhow!("Cannot resolve audio output: {e:?}"))?;
    let output_device = output.0 .0;
    let output_sample_rate = output
        .nominal_sample_rate()
        .map_err(|e| anyhow!("Cannot read output format: {e:?}"))?
        as u32;
    Ok(ResolvedApplication {
        running: !apps.is_empty(),
        processes,
        output_device,
        output_sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        App {
            pid: 10,
            identity: ApplicationIdentity {
                bundle_id: "test.zoom".into(),
                bundle_path: "/Applications/Zoom.app".into(),
                name: "Zoom".into(),
            },
        }
    }
    #[test]
    #[ignore = "reads native running applications and audio hardware"]
    fn native_discovery_smoke() {
        assert!(is_supported());
        let apps = list_applications().unwrap();
        assert!(!apps.is_empty());
        for app in &apps {
            assert!(!app.identity.bundle_id.is_empty());
            assert!(Path::new(&app.identity.bundle_path).is_absolute());
        }
        let missing = resolve(&ApplicationIdentity {
            bundle_id: "invalid.meetily.nonexistent".into(),
            bundle_path: "/Applications/NonexistentMeetilyTest.app".into(),
            name: "Missing".into(),
        })
        .unwrap();
        assert!(!missing.running);
        assert!(missing.processes.is_empty());
        println!(
            "Enumerated {} applications; missing app resolved to no processes",
            apps.len()
        );
    }
    #[test]
    fn groups_nested_and_descendant_helpers_but_not_other_apps() {
        let mut processes = HashMap::new();
        processes.insert(
            11,
            Process {
                parent: Some(1),
                executable: Some(
                    "/Applications/Zoom.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"
                        .into(),
                ),
            },
        );
        processes.insert(
            12,
            Process {
                parent: Some(10),
                executable: Some("/usr/libexec/audio-helper".into()),
            },
        );
        processes.insert(
            13,
            Process {
                parent: Some(10),
                executable: Some("/Applications/Music.app/Contents/MacOS/Music".into()),
            },
        );
        assert!(belongs_to_app(11, &app(), &processes));
        assert!(belongs_to_app(12, &app(), &processes));
        assert!(!belongs_to_app(13, &app(), &processes));
        assert!(!belongs_to_app(99, &app(), &processes));
        // Reused PID is evaluated from current ownership, not remembered.
        processes.insert(12, processes[&13].clone());
        assert!(!belongs_to_app(12, &app(), &processes));
    }
    #[test]
    fn path_prefixes_and_process_cycles_do_not_match() {
        let processes = HashMap::from([
            (
                11,
                Process {
                    parent: Some(12),
                    executable: Some("/Applications/Zoom.app-copy/Contents/MacOS/x".into()),
                },
            ),
            (
                12,
                Process {
                    parent: Some(11),
                    executable: None,
                },
            ),
        ]);
        assert!(!belongs_to_app(11, &app(), &processes));
    }
}
