// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether a program of the suite is open, from the processes that run (D-157).
//!
//! A program is never updated or removed while it runs: Windows cannot replace the files of a
//! running program. Opening its exe for writing tells that only where the account may write the
//! file: in `Program Files` it fails with "access denied" whether the program runs or not. So the
//! launcher lists the processes (a ToolHelp snapshot) and reads each one's image path
//! (`QueryFullProcessImageNameW`). A process whose path it cannot read, such as another account's,
//! counts by its image name.
//!
//! OpenFarm keeps running without a window, so it has no visible way to be closed: the launcher
//! can ask it to quit through its local app, as this computer's own programs do ([`ask_to_quit`]),
//! and wait for it to end ([`close`]).
//!
//! Open3D's own code, not part of ArtCraft Launcher.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use open3d_launcher_core::SuiteApp;

use crate::http::Stop;

/// A running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Process {
    /// Its image name (`OpenRef.exe`).
    pub name: String,
    /// Its exe, when this account may read it.
    pub path: Option<PathBuf>,
}

/// The processes that run now. Tests give their own list.
pub(crate) trait Processes {
    fn list(&self) -> Vec<Process>;
}

/// This computer's processes.
pub(crate) struct SystemProcesses;

impl Processes for SystemProcesses {
    fn list(&self) -> Vec<Process> {
        use winsafe::{HPROCESS, HPROCESSLIST, co};
        let mut snapshot = match HPROCESSLIST::CreateToolhelp32Snapshot(co::TH32CS::SNAPPROCESS, None) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                log::warn!("couldn't list the running programs ({e})");
                return Vec::new();
            }
        };
        let mut found = Vec::new();
        for entry in snapshot.iter_processes() {
            let Ok(entry) = entry else { break };
            let path = HPROCESS::OpenProcess(co::PROCESS::QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID)
                .ok()
                .and_then(|process| process.QueryFullProcessImageName(co::PROCESS_NAME::WIN32).ok())
                .filter(|path| !path.is_empty())
                .map(PathBuf::from);
            found.push(Process { name: entry.szExeFile(), path });
        }
        found
    }
}

/// Whether one of `processes` runs one of `exes`, the copies of a program the launcher looks after.
/// A process whose path is known counts only when it is one of them: the same program started from
/// somewhere else (a development build) is not this copy. One whose path is unknown counts when its
/// image name is theirs.
pub(crate) fn is_open(exes: &[PathBuf], processes: &[Process]) -> bool {
    processes.iter().any(|process| match &process.path {
        Some(path) => exes.iter().any(|exe| same_path(path, exe)),
        None => exes.iter().filter_map(|exe| exe.file_name()).any(|name| name.to_string_lossy().eq_ignore_ascii_case(&process.name)),
    })
}

/// What a job that would change an open program says (D-157).
pub(crate) fn open_error(app: &SuiteApp) -> String {
    format!("{} is open; close it and try again.", app.name)
}

/// What an update says when the program did not close when asked.
pub(crate) fn did_not_close(app: &SuiteApp) -> String {
    format!("{} didn't close when asked, so it wasn't updated. Close it and try again.", app.name)
}

/// Whether the launcher can ask `app` to quit: OpenFarm, through its local app (D-157).
pub(crate) fn closable(app: &SuiteApp) -> bool {
    app.id == "openfarm"
}

/// The port of OpenFarm's local app when its `config.json` names none (OpenFarm's `config.rs`).
const OPENFARM_PORT: u16 = 47102;

/// The `config.json` files OpenFarm may read its local port from, in the order it reads them: in
/// its settings folder among the Open3D folders, then in the folder it used before,
/// `%LOCALAPPDATA%\OpenFarm`, which it reads until it has copied that one over.
pub(crate) fn openfarm_configs() -> Vec<PathBuf> {
    let new = crate::program_data::folders_of("OpenFarm").settings.map(|dir| dir.join("config.json"));
    let old = dirs::data_local_dir().map(|dir| dir.join("OpenFarm").join("config.json"));
    new.into_iter().chain(old).collect()
}

/// The port of OpenFarm's local app: `local_port` in the first of `configs` that is there, or
/// [`OPENFARM_PORT`].
pub(crate) fn openfarm_port(configs: &[PathBuf]) -> u16 {
    let Some(bytes) = configs.iter().find_map(|file| std::fs::read(file).ok()) else { return OPENFARM_PORT };
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|config| config.get("local_port")?.as_u64())
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port != 0)
        .unwrap_or(OPENFARM_PORT)
}

/// Asks the OpenFarm listening on `port` of this computer to quit: `POST /local/quit`, addressed to
/// `127.0.0.1` with that port and with no `Origin`, the way this computer's programs call it
/// (OpenFarm's `loopback.rs` refuses what a web page could send). `Ok` once it has said yes.
pub(crate) fn ask_to_quit(port: u16) -> Result<(), String> {
    use std::io::{Read, Write};
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream =
        std::net::TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|e| format!("nothing answered on port {port} ({e})"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let request = format!("POST /local/quit HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|e| format!("couldn't ask it ({e})"))?;
    let mut answer = Vec::new();
    // It may end before it has closed the connection: what came is enough.
    let _ = stream.take(1024).read_to_end(&mut answer);
    let answer = String::from_utf8_lossy(&answer);
    let status = answer.lines().next().and_then(|line| line.split(' ').nth(1)).unwrap_or("nothing");
    if status == "200" { Ok(()) } else { Err(format!("it answered {status}")) }
}

/// Asks `app` to quit (`ask`) and waits, up to `limit`, until none of `exes` runs. `Err` when it
/// could not be asked or did not end in time, and nothing is to be changed.
pub(crate) fn close(
    app: &SuiteApp,
    exes: &[PathBuf],
    processes: &dyn Processes,
    ask: impl FnOnce() -> Result<(), String>,
    limit: Duration,
    stop: &Stop,
) -> Result<(), String> {
    if let Err(e) = ask() {
        log::warn!("{}: couldn't ask it to quit: {e}", app.name);
        return Err(did_not_close(app));
    }
    log::info!("{}: asked it to quit; waiting for it to end", app.name);
    let started = Instant::now();
    while is_open(exes, &processes.list()) {
        stop.check()?;
        if started.elapsed() >= limit {
            log::warn!("{}: still running {} s after it was asked to quit", app.name, limit.as_secs());
            return Err(did_not_close(app));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

/// Whether two paths name the same file: Windows ignores case and the direction of the slashes, and
/// reads `\\?\C:\…` as `C:\…`.
fn same_path(left: &Path, right: &Path) -> bool {
    let plain = |p: &Path| {
        let text = p.to_string_lossy().replace('/', "\\").to_lowercase();
        text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text).trim_end_matches('\\').to_string()
    };
    plain(left) == plain(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, path: Option<&str>) -> Process {
        Process { name: name.into(), path: path.map(PathBuf::from) }
    }

    #[test]
    fn a_program_is_open_when_one_of_its_copies_runs() {
        let exes = [PathBuf::from(r"C:\Program Files\Open3D\OpenRef\OpenRef.exe"), PathBuf::from(r"C:\Local\Programs\Open3D\OpenRef\OpenRef.exe")];
        let others = [process("explorer.exe", Some(r"C:\Windows\explorer.exe")), process("System", None)];
        assert!(!is_open(&exes, &others));
        for running in [
            r"C:\Program Files\Open3D\OpenRef\OpenRef.exe",
            r"c:\program files\open3d\openref\OPENREF.EXE",
            r"\\?\C:\Local\Programs\Open3D\OpenRef\OpenRef.exe",
        ] {
            let mut list = others.to_vec();
            list.push(process("OpenRef.exe", Some(running)));
            assert!(is_open(&exes, &list), "{running}");
        }
        // The same program started from elsewhere is another copy.
        assert!(!is_open(&exes, &[process("OpenRef.exe", Some(r"D:\dev\target\release\OpenRef.exe"))]));
        // Another account's copy, whose path can't be read, counts by its name.
        assert!(is_open(&exes, &[process("openref.exe", None)]));
        assert!(!is_open(&exes, &[process("OpenView.exe", None)]));
        assert!(!is_open(&[], &[process("OpenRef.exe", None)]));
    }

    /// The real list: this test's own process is in it, with its path.
    #[test]
    fn lists_this_process_with_its_path() {
        let me = std::env::current_exe().unwrap();
        let list = SystemProcesses.list();
        assert!(list.len() > 1);
        assert!(is_open(std::slice::from_ref(&me), &list), "{} is not in the list", me.display());
        assert!(list.iter().any(|p| p.path.as_deref().is_some_and(|path| same_path(path, &me))));
    }

    #[test]
    fn says_the_program_is_open() {
        let app = SuiteApp::by_id("openref").unwrap();
        assert_eq!(open_error(app), "OpenRef is open; close it and try again.");
        assert!(!closable(app));
        assert!(closable(SuiteApp::by_id("openfarm").unwrap()));
    }

    #[test]
    fn openfarms_port_is_the_one_its_config_names() {
        let tmp = tempfile::tempdir().unwrap();
        let (new, old) = (tmp.path().join("new.json"), tmp.path().join("old.json"));
        assert_eq!(openfarm_port(&[new.clone(), old.clone()]), 47102);
        std::fs::write(&old, br#"{"role":"agent","local_port":47302}"#).unwrap();
        assert_eq!(openfarm_port(&[new.clone(), old.clone()]), 47302);
        // The new folder's file, once it is there, is the one OpenFarm reads.
        std::fs::write(&new, br#"{"local_port":47202}"#).unwrap();
        assert_eq!(openfarm_port(&[new.clone(), old.clone()]), 47202);
        for text in ["not json", "{}", r#"{"local_port":"47202"}"#, r#"{"local_port":70000}"#, r#"{"local_port":0}"#] {
            std::fs::write(&new, text).unwrap();
            assert_eq!(openfarm_port(&[new.clone(), old.clone()]), 47102, "{text}");
        }
    }

    /// The request is the one OpenFarm's local app takes from this computer's programs.
    #[test]
    fn asks_openfarm_to_quit_as_its_own_programs_do() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in ["HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}", "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 1024];
                let read = stream.read(&mut request).unwrap();
                asked.push(String::from_utf8_lossy(&request[..read]).into_owned());
                stream.write_all(answer.as_bytes()).unwrap();
            }
            asked
        });
        assert_eq!(ask_to_quit(port), Ok(()));
        assert_eq!(ask_to_quit(port), Err("it answered 403".into()));
        let asked = server.join().unwrap();
        let request = &asked[0];
        assert!(request.starts_with("POST /local/quit HTTP/1.1\r\n"), "{request}");
        assert!(request.contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")), "{request}");
        assert!(!request.to_ascii_lowercase().contains("origin:") && !request.to_ascii_lowercase().contains("sec-fetch"), "{request}");
        // Nothing listening: it says so.
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(ask_to_quit(free).unwrap_err().starts_with("nothing answered"));
    }

    /// Processes that end after a number of looks, or never.
    struct Ending {
        looks: std::cell::Cell<u32>,
        ends_after: Option<u32>,
    }

    impl Processes for Ending {
        fn list(&self) -> Vec<Process> {
            let looks = self.looks.get();
            self.looks.set(looks + 1);
            if self.ends_after.is_some_and(|after| looks >= after) {
                Vec::new()
            } else {
                vec![process("OpenFarm.exe", Some(r"C:\Program Files\Open3D\OpenFarm\OpenFarm.exe"))]
            }
        }
    }

    #[test]
    fn closing_waits_for_the_program_to_end_and_gives_up() {
        let app = SuiteApp::by_id("openfarm").unwrap();
        let exes = [PathBuf::from(r"C:\Program Files\Open3D\OpenFarm\OpenFarm.exe")];
        let stop = Stop::default();
        let ending = Ending { looks: std::cell::Cell::new(0), ends_after: Some(3) };
        let asked = std::cell::Cell::new(false);
        let ask = || {
            asked.set(true);
            Ok(())
        };
        assert_eq!(close(app, &exes, &ending, ask, Duration::from_secs(10), &stop), Ok(()));
        assert!(asked.get());
        let never = Ending { looks: std::cell::Cell::new(0), ends_after: None };
        let message = "OpenFarm didn't close when asked, so it wasn't updated. Close it and try again.";
        assert_eq!(close(app, &exes, &never, || Ok(()), Duration::from_millis(300), &stop), Err(message.into()));
        // It could not be asked: nothing to wait for.
        let ending = Ending { looks: std::cell::Cell::new(0), ends_after: Some(0) };
        assert_eq!(close(app, &exes, &ending, || Err("nothing answered".into()), Duration::from_secs(10), &stop), Err(message.into()));
        assert_eq!(ending.looks.get(), 0);
    }
}
