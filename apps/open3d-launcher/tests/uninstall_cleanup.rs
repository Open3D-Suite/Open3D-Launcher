// SPDX-License-Identifier: GPL-3.0-or-later
//! The launcher started by its MSI with `--uninstall-cleanup` removes its caches from the folder
//! `OPEN3D_DATA_DIR` names and exits, without opening a window or claiming the single instance; its
//! settings and the record of its apps stay (D-045, D-158).

#![cfg(windows)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_open3d-launcher");

/// A scratch folder of its own, removed at the end.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether this account's start-at-login entry starts the exe under test: the cleanup would then
/// remove it, and a test must never change the real registry.
fn login_entry_starts_this_exe() -> bool {
    let run = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let value: Option<String> = run.ok().and_then(|key| key.get_value("Open3D Launcher").ok());
    value.is_some_and(|value| value.to_lowercase().contains(&EXE.to_lowercase().replace('/', "\\")))
}

#[test]
fn the_uninstall_cleanup_removes_the_caches_and_exits() {
    if login_entry_starts_this_exe() {
        eprintln!("skipped: this account's start-at-login entry starts the exe under test");
        return;
    }
    let tmp = Scratch(std::env::temp_dir().join(format!("open3d-launcher-uninstall-cleanup-{}", std::process::id())));
    let (data, settings) = (tmp.0.join("Local"), tmp.0.join("Roaming"));
    let own = data.join("Launcher");
    std::fs::create_dir_all(own.join("downloads")).unwrap();
    std::fs::create_dir_all(own.join("logs")).unwrap();
    std::fs::create_dir_all(settings.join("Launcher")).unwrap();
    for file in ["downloads/openref-0.2.0-windows-x64.msi", "releases.json", "feed.json", "installed.json", "logs/open3d-launcher.log"] {
        std::fs::write(own.join(file), b"x").unwrap();
    }
    std::fs::write(settings.join("Launcher").join("app.ron"), b"()").unwrap();
    let mut cleanup = Command::new(EXE)
        .arg("--uninstall-cleanup")
        .env("OPEN3D_DATA_DIR", &data)
        .env("OPEN3D_SETTINGS_DIR", &settings)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = cleanup.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = cleanup.kill();
            panic!("the launcher started instead of cleaning up");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success());
    let mut left: Vec<String> = std::fs::read_dir(&own).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
    left.sort();
    assert_eq!(left, ["installed.json", "logs"], "only the caches go");
    assert!(settings.join("Launcher").join("app.ron").is_file(), "its settings stay");
}
