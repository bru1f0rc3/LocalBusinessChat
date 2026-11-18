//! History of received files: who sent what and when (`received_files.jsonl` in the
//! profile folder, one JSON record per line). Shown by Действия → «История принятых файлов».

use crate::config::Paths;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Received {
    /// unix seconds, when the file was completely received
    pub time: i64,
    /// sender uid; 0 for records imported from the event log (only the nick is known there)
    pub from: u64,
    pub nick: String,
    pub computer: String,
    pub name: String,
    pub size: u64,
    /// where it was saved
    pub path: String,
}

impl Received {
    /// Key of the sender for the "От кого" filter: the uid (survives nick changes), or the
    /// nick when the uid is unknown.
    pub fn sender_key(&self) -> String {
        if self.from != 0 {
            format!("u{:x}", self.from)
        } else {
            format!("n{}", self.nick)
        }
    }

    pub fn local_time(&self) -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local.timestamp_opt(self.time, 0).single().unwrap_or_else(chrono::Local::now)
    }
}

/// Period of the "Когда" filter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Period {
    #[default]
    All,
    Today,
    Yesterday,
    Week,
    Month,
    Custom,
}

impl Period {
    pub const ALL: [Period; 6] = [Period::All, Period::Today, Period::Yesterday, Period::Week, Period::Month, Period::Custom];
    pub fn title(self) -> &'static str {
        match self {
            Period::All => "За всё время",
            Period::Today => "Сегодня",
            Period::Yesterday => "Вчера",
            Period::Week => "За 7 дней",
            Period::Month => "За 30 дней",
            Period::Custom => "Период…",
        }
    }
}

/// State of the history window (open while Some).
#[derive(Clone, Debug, Default)]
pub struct Filter {
    /// Received::sender_key, empty = everybody
    pub sender: String,
    pub name: String,
    pub period: Period,
    /// Period::Custom: "ДД.ММ.ГГГГ", both inclusive, empty = no limit
    pub from_date: String,
    pub to_date: String,
}

fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    let s = s.trim();
    ["%d.%m.%Y", "%d.%m.%y", "%Y-%m-%d"].iter().find_map(|f| chrono::NaiveDate::parse_from_str(s, f).ok())
}

impl Filter {
    /// Allowed dates (inclusive) for the period; None = no limit on that side.
    /// Err: a date typed in the custom period cannot be read.
    pub fn date_range(&self, today: chrono::NaiveDate) -> Result<(Option<chrono::NaiveDate>, Option<chrono::NaiveDate>), &'static str> {
        let days = |n: i64| today - chrono::Duration::days(n);
        Ok(match self.period {
            Period::All => (None, None),
            Period::Today => (Some(today), Some(today)),
            Period::Yesterday => (Some(days(1)), Some(days(1))),
            Period::Week => (Some(days(6)), None),
            Period::Month => (Some(days(29)), None),
            Period::Custom => {
                let get = |s: &str| if s.trim().is_empty() { Ok(None) } else { parse_date(s).map(Some).ok_or("дата должна быть в виде ДД.ММ.ГГГГ") };
                (get(&self.from_date)?, get(&self.to_date)?)
            }
        })
    }

    pub fn matches(&self, r: &Received, range: (Option<chrono::NaiveDate>, Option<chrono::NaiveDate>)) -> bool {
        if !self.sender.is_empty() && r.sender_key() != self.sender {
            return false;
        }
        let q = self.name.trim().to_lowercase();
        if !q.is_empty() && !r.name.to_lowercase().contains(&q) {
            return false;
        }
        let d = r.local_time().date_naive();
        range.0.map_or(true, |a| d >= a) && range.1.map_or(true, |b| d <= b)
    }
}

pub fn history_path(paths: &Paths) -> PathBuf {
    paths.data.join("received_files.jsonl")
}

pub fn append(paths: &Paths, r: &Received) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(history_path(paths)) {
        if let Ok(s) = serde_json::to_string(r) {
            let _ = writeln!(f, "{s}");
        }
    }
}

/// The history, oldest first. The first time (no history file yet) the files received before
/// this version are taken from the event log, where every received file was written down.
pub fn load(paths: &Paths) -> Vec<Received> {
    let file = history_path(paths);
    if let Ok(s) = std::fs::read_to_string(&file) {
        return s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    }
    let v = std::fs::read_to_string(&paths.events).map(|s| import_event_log(&s)).unwrap_or_default();
    let body: String = v.iter().filter_map(|r| serde_json::to_string(r).ok()).map(|s| s + "\n").collect();
    let _ = std::fs::write(&file, body);
    v
}

/// "2026-11-18 23:29:05 [Info] Файлы: Файл «a.txt» от Вася получен: C:\…\a.txt"
fn import_event_log(log: &str) -> Vec<Received> {
    use chrono::TimeZone;
    let mut v = Vec::new();
    for line in log.lines() {
        let Some((stamp, rest)) = line.split_at_checked(19) else { continue };
        let Some(rest) = rest.strip_prefix(" [Info] Файлы: Файл «") else { continue };
        let Some((name, rest)) = rest.split_once("» от ") else { continue };
        let Some((nick, path)) = rest.rsplit_once(" получен: ") else { continue };
        let Ok(t) = chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S") else { continue };
        let Some(t) = chrono::Local.from_local_datetime(&t).earliest() else { continue };
        let size = std::fs::metadata(Path::new(path)).map(|m| m.len()).unwrap_or(0);
        v.push(Received { time: t.timestamp(), from: 0, nick: nick.to_string(), computer: String::new(), name: name.to_string(), size, path: path.to_string() });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(nick: &str, name: &str, ymd: (i32, u32, u32)) -> Received {
        use chrono::TimeZone;
        let t = chrono::Local.with_ymd_and_hms(ymd.0, ymd.1, ymd.2, 12, 0, 0).unwrap();
        Received { time: t.timestamp(), from: 0, nick: nick.into(), name: name.into(), ..Default::default() }
    }

    #[test]
    fn filters_by_sender_name_and_period() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        let v = [rec("Вася", "Отчёт.docx", (2026, 11, 18)), rec("Петя", "отчёт.xlsx", (2026, 11, 17)), rec("Вася", "photo.jpg", (2026, 10, 1))];
        let count = |f: &Filter| {
            let r = f.date_range(today).unwrap();
            v.iter().filter(|x| f.matches(x, r)).count()
        };
        assert_eq!(count(&Filter::default()), 3);
        assert_eq!(count(&Filter { name: "ОТЧЁТ".into(), ..Default::default() }), 2);
        assert_eq!(count(&Filter { sender: "nВася".into(), ..Default::default() }), 2);
        assert_eq!(count(&Filter { period: Period::Today, ..Default::default() }), 1);
        assert_eq!(count(&Filter { period: Period::Yesterday, ..Default::default() }), 1);
        assert_eq!(count(&Filter { period: Period::Week, ..Default::default() }), 2);
        let custom = Filter { period: Period::Custom, from_date: "01.10.2026".into(), to_date: "17.11.2026".into(), ..Default::default() };
        assert_eq!(count(&custom), 2);
        let bad = Filter { period: Period::Custom, from_date: "1 октября".into(), ..Default::default() };
        assert!(bad.date_range(today).is_err());
    }

    #[test]
    fn imports_received_files_from_event_log() {
        let log = "2026-11-18 23:29:05 [Info] Сеть: UDP 192.168.1.62:50240\n\
                   2026-11-18 23:30:00 [Info] Файлы: Файл «a b.txt» от Вася Пупкин получен: C:\\x\\a b.txt\n\
                   2026-11-18 23:31:00 [Info] Файлы: Файл «c.txt» отправлен Петя\n";
        let v = import_event_log(log);
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].name.as_str(), v[0].nick.as_str(), v[0].path.as_str()), ("a b.txt", "Вася Пупкин", "C:\\x\\a b.txt"));
    }
}
