//! Auto-update from a shared folder (by default `\\server\hole\update`).
//!
//! Publishing a new version = putting `NassiRS-Setup-<version>.exe` into that folder
//! (`installer\build-installer.ps1 -Publish` does it). The program looks there at start and
//! then every hour; the newest installer with a version above its own is copied to
//! %TEMP%\NassiRS-update and run silently, the program quits, the installer replaces it and
//! starts it again (`/relaunch=1`, or `/relaunch=2` = back into the tray).
//! The installer needs no administrator rights, so nobody is asked anything.

use crate::app::{App, Level};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

pub const DEFAULT_DIR: &str = r"\\server\hole\update";
const PREFIX: &str = "nassirs-setup-";
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// the first check: a few seconds after start, when the window and the network are up
const FIRST_CHECK: Duration = Duration::from_secs(5);
/// an update that was started but did not replace this version is not retried before
const RETRY_AFTER_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug)]
pub struct Found {
    pub version: String,
    /// the local copy in %TEMP%
    pub path: PathBuf,
}

pub struct Updater {
    rx: Option<Receiver<Result<Option<Found>, String>>>,
    next_check: Instant,
    /// downloaded, waiting for a moment when nobody uses the program
    pending: Option<Found>,
    /// "Проверить сейчас": install at once, report the result
    manual: bool,
    /// a check at start: install at once too (nothing is open yet)
    at_start: bool,
    last_error: Option<String>,
    pub status: String,
}

impl Default for Updater {
    fn default() -> Self {
        Updater {
            rx: None,
            next_check: Instant::now() + FIRST_CHECK,
            pending: None,
            manual: false,
            at_start: true,
            last_error: None,
            status: "Ещё не проверялось".into(),
        }
    }
}

/// "0.2.10" -> [0, 2, 10]; None for anything that is not dot-separated numbers
pub fn parse_version(s: &str) -> Option<Vec<u32>> {
    let v: Option<Vec<u32>> = s.trim().split('.').map(|p| p.parse().ok()).collect();
    v.filter(|v| !v.is_empty())
}

fn newer(a: &[u32], b: &[u32]) -> bool {
    let n = a.len().max(b.len());
    let get = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n).map(|i| get(a, i).cmp(&get(b, i))).find(|o| o.is_ne()) == Some(std::cmp::Ordering::Greater)
}

/// The newest `NassiRS-Setup-<ver>.exe` in `dir` with a version above `current`.
pub fn find_newest(names: impl IntoIterator<Item = String>, current: &str) -> Option<(String, String)> {
    let cur = parse_version(current)?;
    let mut best: Option<(Vec<u32>, String, String)> = None;
    for name in names {
        let low = name.to_lowercase();
        let Some(ver) = low.strip_prefix(PREFIX).and_then(|s| s.strip_suffix(".exe")) else { continue };
        let Some(v) = parse_version(ver) else { continue };
        if newer(&v, &cur) && best.as_ref().map_or(true, |(b, _, _)| newer(&v, b)) {
            best = Some((v, ver.to_string(), name));
        }
    }
    best.map(|(_, ver, name)| (ver, name))
}

/// Runs in a thread: a network folder that is not reachable can block for half a minute.
fn check(dir: &Path, current: &str) -> Result<Option<Found>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("папка {} недоступна: {e}", dir.display()))?;
    let names = rd.flatten().filter(|e| e.file_type().map_or(false, |t| t.is_file())).filter_map(|e| e.file_name().into_string().ok());
    let Some((version, name)) = find_newest(names, current) else { return Ok(None) };
    let tmp = std::env::temp_dir().join("NassiRS-update");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let dst = tmp.join(&name);
    let part = tmp.join(format!("{name}.part"));
    let n = std::fs::copy(dir.join(&name), &part).map_err(|e| format!("не удалось скопировать {name}: {e}"))?;
    if n == 0 {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{name}: пустой файл"));
    }
    let _ = std::fs::remove_file(&dst);
    std::fs::rename(&part, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(Some(Found { version, path: dst }))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl App {
    fn update_marker(&self) -> PathBuf {
        self.paths.data.join("update-tried.txt")
    }

    /// The update to `version` was already started less than a day ago and we are still
    /// the old version: the installer failed, do not close the program again and again.
    fn update_recently_failed(&self, version: &str) -> bool {
        let Ok(s) = std::fs::read_to_string(self.update_marker()) else { return false };
        let mut it = s.split_whitespace();
        let (Some(v), Some(t)) = (it.next(), it.next().and_then(|t| t.parse::<u64>().ok())) else { return false };
        v == version && now_unix().saturating_sub(t) < RETRY_AFTER_SECS
    }

    /// "Проверить сейчас" in the settings (with the folder typed there, maybe not saved yet).
    pub fn check_update_now(&mut self, dir: &str) {
        if self.updater.rx.is_some() {
            return;
        }
        let _ = std::fs::remove_file(self.update_marker());
        self.updater.manual = true;
        self.start_update_check(dir.to_string());
    }

    fn start_update_check(&mut self, dir: String) {
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        let current = crate::proto::VERSION.to_string();
        let spawned = std::thread::Builder::new().name("update".into()).spawn(move || {
            let _ = tx.send(check(Path::new(dir.trim()), &current));
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => {
                self.updater.rx = Some(rx);
                self.updater.status = "Проверка…".into();
            }
            Err(e) => self.updater.status = format!("Не удалось начать проверку: {e}"),
        }
        self.updater.next_check = Instant::now() + CHECK_EVERY;
    }

    /// Called from tick() (once a second).
    pub fn update_tick(&mut self) {
        if let Some(rx) = &self.updater.rx {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.updater.rx = None,
                Ok(res) => {
                    self.updater.rx = None;
                    let stamp = chrono::Local::now().format("%d.%m.%Y %H:%M");
                    match res {
                        Ok(None) => {
                            self.updater.status = format!("{stamp}: установлена последняя версия ({})", crate::proto::VERSION);
                            if self.updater.manual {
                                self.flash("Обновлений нет");
                            }
                            self.updater.manual = false;
                            self.updater.at_start = false;
                            self.updater.last_error = None;
                        }
                        Ok(Some(f)) => {
                            self.updater.status = format!("{stamp}: найдена версия {}", f.version);
                            self.updater.last_error = None;
                            if !self.updater.manual && self.update_recently_failed(&f.version) {
                                self.updater.status = format!(
                                    "{stamp}: версия {} уже устанавливалась, но не установилась — следующая попытка через сутки (или «Проверить сейчас»)",
                                    f.version
                                );
                                self.updater.at_start = false;
                            } else {
                                self.log(Level::Info, "Обновление", format!("Найдена версия {}: {}", f.version, f.path.display()));
                                self.updater.pending = Some(f);
                            }
                        }
                        Err(e) => {
                            self.updater.status = format!("{stamp}: {e}");
                            // the log gets the error once, not every hour
                            if self.updater.manual || self.updater.last_error.as_deref() != Some(e.as_str()) {
                                self.log(Level::Warn, "Обновление", e.clone());
                            }
                            if self.updater.manual {
                                self.flash(format!("Обновление: {e}"));
                            }
                            self.updater.last_error = Some(e);
                            self.updater.manual = false;
                            self.updater.at_start = false;
                        }
                    }
                }
            }
        }

        if self.updater.pending.is_some() {
            if self.can_install_update() {
                let f = self.updater.pending.take().unwrap();
                self.install_update(f);
            }
            return;
        }
        if self.updater.rx.is_none() && Instant::now() >= self.updater.next_check {
            if self.cfg.general.auto_update && !self.cfg.general.update_dir.trim().is_empty() {
                self.start_update_check(self.cfg.general.update_dir.clone());
            } else {
                self.updater.next_check = Instant::now() + CHECK_EVERY;
                self.updater.at_start = false;
                self.updater.status = "Автообновление выключено".into();
            }
        }
    }

    /// Do not close the program under someone's hands: not during a file transfer or a remote
    /// session, and (after the start) only when the window is not being used.
    fn can_install_update(&self) -> bool {
        use crate::app::TState;
        let busy = self.transfers.iter().any(|t| matches!(t.state, TState::Running | TState::Waiting)) || !self.host_sessions.is_empty() || !self.viewers.is_empty();
        if busy {
            return false;
        }
        if self.updater.manual || self.updater.at_start {
            return true;
        }
        !self.focused || !self.visible || self.minimized || crate::platform::idle_secs() >= 120
    }

    fn install_update(&mut self, f: Found) {
        self.updater.manual = false;
        self.updater.at_start = false;
        let _ = std::fs::write(self.update_marker(), format!("{} {}\n", f.version, now_unix()));
        let log = f.path.with_extension("log");
        // Inno Setup switches; /S is the same for the NSIS installer (each ignores the other's)
        let relaunch = if self.visible { "/relaunch=1" } else { "/relaunch=2" };
        let res = std::process::Command::new(&f.path)
            .arg("/VERYSILENT")
            .arg("/SUPPRESSMSGBOXES")
            .arg("/NORESTART")
            .arg("/SP-")
            .arg(format!("/LOG={}", log.display()))
            .arg("/S")
            .arg(relaunch)
            .spawn();
        match res {
            Ok(_) => {
                self.log(Level::Info, "Обновление", format!("Устанавливается версия {}, программа перезапустится", f.version));
                self.updater.status = format!("Устанавливается версия {}…", f.version);
                self.quit = true;
            }
            Err(e) => {
                self.log(Level::Error, "Обновление", format!("Не удалось запустить {}: {e}", f.path.display()));
                self.updater.status = format!("Не удалось запустить установщик: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_newest_above_current() {
        let names = ["NassiRS-Setup-0.2.3.exe", "NassiRS-Setup-0.2.10.exe", "NassiRS-Setup-0.2.9.exe", "readme.txt", "NassiRS-Setup-x.exe", "nassirs-setup-0.3.exe.part"];
        let got = find_newest(names.iter().map(|s| s.to_string()), "0.2.3");
        assert_eq!(got, Some(("0.2.10".into(), "NassiRS-Setup-0.2.10.exe".into())));
    }

    #[test]
    fn nothing_when_same_or_older() {
        let names = ["NassiRS-Setup-0.2.3.exe", "NassiRS-Setup-0.2.2.exe", "NassiRS-Setup-0.2.exe"];
        assert_eq!(find_newest(names.iter().map(|s| s.to_string()), "0.2.3"), None);
        assert!(newer(&[0, 3], &[0, 2, 9]));
        assert!(!newer(&[0, 2, 0], &[0, 2]));
    }
}
