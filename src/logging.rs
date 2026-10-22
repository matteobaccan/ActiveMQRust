// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Logging setup: one line per event with a local timestamp, to stdout, to a daily file, or both.
//!
//! The file being written is always `<log.dir>/mqrust.log`. When the local date changes it is
//! renamed `mqrust-YYYY-MM-DD.log` after the day it holds, compressed to `.log.gz` by a
//! background thread and only then removed; archives older than `log.retention_days` are
//! deleted. Logging never stops the broker: a folder that cannot be written costs the file
//! output, one warning on stderr and a new attempt a minute later.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::format::{DefaultFields, Format, Full, Writer};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

/// Name of the file the broker is writing; the archives carry a date instead.
pub const ACTIVE: &str = "mqrust.log";
const PREFIX: &str = "mqrust-";
const TIME: &str = "%Y-%m-%d %H:%M:%S";
/// How long the writer waits before trying a failed rename or open again.
const RETRY: Duration = Duration::from_secs(60);

struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format(TIME))
    }
}

fn level(name: &str) -> LevelFilter {
    match name {
        "error" => LevelFilter::ERROR,
        "warn" => LevelFilter::WARN,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        _ => LevelFilter::INFO,
    }
}

/// The clock the writer reads. Tests replace it to cross midnight without waiting.
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> DateTime<Local> + Send + Sync>);

impl Clock {
    pub fn local() -> Clock {
        Clock(Arc::new(Local::now))
    }

    /// A clock driven by a value a test can move.
    #[cfg(test)]
    fn fixed(at: Arc<Mutex<DateTime<Local>>>) -> Clock {
        Clock(Arc::new(move || *at.lock().unwrap()))
    }

    fn now(&self) -> DateTime<Local> {
        (self.0)()
    }

    fn today(&self) -> NaiveDate {
        self.now().date_naive()
    }
}

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Clock")
    }
}

/// What the file output needs: where to write, how long to keep archives, and the
/// configuration to name in the header line.
#[derive(Debug, Clone)]
pub struct FileOptions {
    pub dir: PathBuf,
    pub retention_days: u64,
    pub compress: bool,
    pub config: String,
}

// ---------------------------------------------------------------------------
// Archive names, compression and retention
// ---------------------------------------------------------------------------

/// The same path with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// The day an archive holds, read from a name `mqrust-YYYY-MM-DD[-N].log[.gz]`.
/// Every other file in the folder, `mqrust.log` included, gives `None`.
fn archive_date(name: &str) -> Option<NaiveDate> {
    let rest = name.strip_prefix(PREFIX)?;
    let rest = rest.strip_suffix(".log.gz").or_else(|| rest.strip_suffix(".log"))?;
    let (date, counter) = rest.split_at_checked(10)?;
    let numbered = match counter.strip_prefix('-') {
        Some(n) => !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()),
        None => counter.is_empty(),
    };
    numbered.then(|| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())?
}

/// Compresses an archive: `.log.gz.tmp`, synced and renamed to `.log.gz`, and only
/// then the uncompressed file is deleted, so no line is lost if this is interrupted.
fn gzip(path: &Path) -> io::Result<()> {
    let tmp = sibling(path, ".gz.tmp");
    {
        let mut input = io::BufReader::new(File::open(path)?);
        let mut out = flate2::write::GzEncoder::new(File::create(&tmp)?, flate2::Compression::default());
        io::copy(&mut input, &mut out)?;
        out.finish()?.sync_all()?;
    }
    std::fs::rename(&tmp, sibling(path, ".gz"))?;
    std::fs::remove_file(path)
}

/// Deletes archives older than `today - retention_days`; `0` keeps every archive.
fn prune(dir: &Path, retention_days: u64, today: NaiveDate) {
    if retention_days == 0 {
        return;
    }
    let Some(oldest) = today.checked_sub_days(chrono::Days::new(retention_days)) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(date) = name.to_str().and_then(archive_date) else {
            continue;
        };
        if date < oldest {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Work handed to the archiving thread, so logging threads never wait for it.
enum Job {
    /// Compress this renamed archive, then delete the uncompressed file.
    Compress(PathBuf),
    /// Apply the retention to the folder.
    Prune,
}

/// Archives left behind by an interrupted run: `.log.gz.tmp` files are deleted and
/// uncompressed archives are compressed again at the next start.
fn leftovers(dir: &Path, compress: bool) -> Vec<Job> {
    let mut jobs = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return jobs;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".log.gz.tmp") {
            let _ = std::fs::remove_file(entry.path());
        } else if compress && name.ends_with(".log") && archive_date(name).is_some() {
            jobs.push(Job::Compress(entry.path()));
        }
    }
    jobs
}

fn run_job(job: Job, dir: &Path, retention_days: u64, clock: &Clock) {
    match job {
        Job::Compress(path) => {
            if let Err(e) = gzip(&path) {
                eprintln!("warning: log file: cannot compress {}: {e}", path.display());
            }
        }
        Job::Prune => prune(dir, retention_days, clock.today()),
    }
}

/// Starts the archiving thread and queues the recovery of an interrupted run.
fn archiver(opts: &FileOptions, clock: &Clock) -> Option<Sender<Job>> {
    let (tx, rx) = mpsc::channel::<Job>();
    let dir = opts.dir.clone();
    let retention_days = opts.retention_days;
    let clock = clock.clone();
    let spawned = std::thread::Builder::new().name("log-archive".into()).spawn(move || {
        for job in &rx {
            run_job(job, &dir, retention_days, &clock);
        }
    });
    if spawned.is_err() {
        return None;
    }
    for job in leftovers(&opts.dir, opts.compress) {
        let _ = tx.send(job);
    }
    let _ = tx.send(Job::Prune);
    Some(tx)
}

// ---------------------------------------------------------------------------
// The rotating writer
// ---------------------------------------------------------------------------

/// The file output: appends to `mqrust.log` and rotates it when the local day changes.
pub struct RotatingFile {
    dir: PathBuf,
    file: Option<File>,
    /// Local date of the lines in the open file, which names its archive.
    date: NaiveDate,
    /// Header line, without timestamp and level, written to every file that is opened.
    header: String,
    compress: bool,
    clock: Clock,
    jobs: Option<Sender<Job>>,
    /// When the last failure was reported, to warn once and retry a minute later.
    failed: Option<Instant>,
}

impl RotatingFile {
    /// Opens the file output, rotating an `mqrust.log` left from an earlier day.
    pub fn open(opts: &FileOptions, clock: Clock) -> RotatingFile {
        let mut w = RotatingFile {
            dir: opts.dir.clone(),
            file: None,
            date: clock.today(),
            header: format!(
                "ActiveMQRust {} log started: pid {}, config {}",
                env!("CARGO_PKG_VERSION"),
                std::process::id(),
                opts.config
            ),
            compress: opts.compress,
            clock,
            jobs: None,
            failed: None,
        };
        if let Err(e) = std::fs::create_dir_all(&opts.dir) {
            w.fail(&format!("cannot create the folder {}: {e}", opts.dir.display()));
            return w;
        }
        w.jobs = archiver(opts, &w.clock);
        let today = w.clock.today();
        match stale_date(&w.dir.join(ACTIVE), today) {
            Some(day) => {
                w.date = day;
                w.rotate(today);
            }
            None => w.open_active(),
        }
        w
    }

    /// Rotates when the day of the open file is no longer today; the midnight thread
    /// calls this so a quiet broker still archives the day that ended.
    fn rotate_if_stale(&mut self) {
        let today = self.clock.today();
        if today != self.date && self.may_retry() {
            self.rotate(today);
        }
    }

    /// Renames the open file to the archive of the day it holds and starts a new one.
    /// A rename that fails keeps the lines in the current file and is tried again later.
    fn rotate(&mut self, today: NaiveDate) {
        let active = self.dir.join(ACTIVE);
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
        }
        let archive = self.archive_path(self.date);
        match std::fs::rename(&active, &archive) {
            Ok(()) => {
                self.date = today;
                if let Some(jobs) = &self.jobs {
                    if self.compress {
                        let _ = jobs.send(Job::Compress(archive));
                    }
                    let _ = jobs.send(Job::Prune);
                }
                self.open_active();
            }
            Err(e) => {
                self.fail(&format!("cannot rotate {}: {e}", active.display()));
                self.file = OpenOptions::new().create(true).append(true).open(&active).ok();
            }
        }
    }

    /// Opens `mqrust.log` and writes the header line, whatever the log level is.
    fn open_active(&mut self) {
        let path = self.dir.join(ACTIVE);
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => {
                // The same shape as a tracing line: timestamp, level right-aligned in five
                // columns, message.
                let line = format!("{} {:>5} {}\n", self.clock.now().format(TIME), "INFO", self.header);
                let _ = file.write_all(line.as_bytes());
                self.file = Some(file);
                self.failed = None;
            }
            Err(e) => self.fail(&format!("cannot open {}: {e}", path.display())),
        }
    }

    /// `mqrust-YYYY-MM-DD.log`, with a `-N` counter when that day already has an
    /// archive, compressed or not: an archive is never overwritten.
    fn archive_path(&self, date: NaiveDate) -> PathBuf {
        let base = format!("{PREFIX}{}", date.format("%Y-%m-%d"));
        let plain = self.dir.join(format!("{base}.log"));
        if !taken(&plain) {
            return plain;
        }
        let mut n = 1u32;
        loop {
            let numbered = self.dir.join(format!("{base}-{n}.log"));
            if !taken(&numbered) {
                return numbered;
            }
            n += 1;
        }
    }

    /// One warning per failure window, then the broker runs on without the file.
    fn fail(&mut self, what: &str) {
        if self.may_retry() {
            eprintln!("warning: log file: {what}");
        }
        self.failed = Some(Instant::now());
        self.file = None;
    }

    fn may_retry(&self) -> bool {
        self.failed.is_none_or(|at| at.elapsed() >= RETRY)
    }
}

/// True when that day already has an archive, in any of its three states.
fn taken(path: &Path) -> bool {
    path.exists() || sibling(path, ".gz").exists() || sibling(path, ".gz.tmp").exists()
}

/// The day of an `mqrust.log` written before today, which is rotated before the first
/// line; `None` when the file is missing or already holds today's lines.
fn stale_date(path: &Path, today: NaiveDate) -> Option<NaiveDate> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let day = DateTime::<Local>::from(modified).date_naive();
    (day < today).then_some(day)
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.rotate_if_stale();
        if self.file.is_none() && self.may_retry() {
            self.open_active();
        }
        if let Some(file) = &mut self.file {
            if let Err(e) = file.write_all(buf) {
                self.fail(&format!("cannot write {}: {e}", self.dir.join(ACTIVE).display()));
            }
        }
        // The line is dropped when there is no file; a logging error never fails a caller.
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.file {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// Clonable handle on the file output: the subscriber writes through it and the
/// midnight thread rotates through it.
#[derive(Clone)]
pub struct FileWriter(Arc<Mutex<RotatingFile>>);

/// The locked writer, which is what the subscriber writes a line to.
pub struct Locked<'a>(MutexGuard<'a, RotatingFile>);

impl Write for Locked<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl FileWriter {
    fn lock(&self) -> MutexGuard<'_, RotatingFile> {
        // A panic while a line was written must not take the file output with it.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<'a> MakeWriter<'a> for FileWriter {
    type Writer = Locked<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        Locked(self.lock())
    }
}

/// Opens the file output and starts the thread that rotates it after midnight even
/// when nothing is logged.
fn start_file(opts: &FileOptions) -> FileWriter {
    let clock = Clock::local();
    let writer = FileWriter(Arc::new(Mutex::new(RotatingFile::open(opts, clock.clone()))));
    let timer = writer.clone();
    let _ = std::thread::Builder::new()
        .name("log-midnight".into())
        .spawn(move || loop {
            std::thread::sleep(until_midnight(&clock));
            timer.lock().rotate_if_stale();
        });
    writer
}

/// Time left to the next local midnight, capped at an hour so that a clock
/// adjustment, a suspend or a daylight saving change is picked up on the next wake.
fn until_midnight(clock: &Clock) -> Duration {
    let now = clock.now();
    let tomorrow = now
        .date_naive()
        .succ_opt()
        .and_then(|d| d.and_hms_opt(0, 0, 1))
        .and_then(|at| at.and_local_timezone(Local).earliest());
    let left = tomorrow
        .map(|at| at.signed_duration_since(now))
        .and_then(|d| d.to_std().ok())
        .unwrap_or(Duration::from_secs(60));
    left.clamp(Duration::from_secs(1), Duration::from_secs(3600))
}

// ---------------------------------------------------------------------------
// Setting up the subscriber
// ---------------------------------------------------------------------------

type Layer<S, W> = tracing_subscriber::fmt::Layer<S, DefaultFields, Format<Full, LocalTime>, W>;

/// One line per event, local timestamp, no target and no colours: the same format
/// on stdout and in the file.
fn layer<S, W>(writer: W) -> Layer<S, W>
where
    S: tracing::Subscriber + for<'l> LookupSpan<'l>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer()
        .with_timer(LocalTime)
        .with_target(false)
        .with_ansi(false)
        .with_writer(writer)
}

/// Console mode: stdout, and the daily file as well unless `log.file` turned it off.
pub fn init_console(level_name: &str, file: Option<&FileOptions>) {
    let filter = level(level_name);
    match file {
        Some(opts) => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(layer(io::stdout))
                .with(layer(start_file(opts)))
                .try_init();
        }
        None => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(layer(io::stdout))
                .try_init();
        }
    }
}

/// Service mode: the daily file only, since there is no console to write to.
pub fn init_service(level_name: &str, opts: &FileOptions) {
    let _ = tracing_subscriber::registry()
        .with(level(level_name))
        .with(layer(start_file(opts)))
        .try_init();
}

/// The file output a configuration asks for.
pub fn file_options(cfg: &crate::config::Config) -> FileOptions {
    FileOptions {
        dir: cfg.log_dir.clone(),
        retention_days: cfg.log_retention_days,
        compress: cfg.log_compress,
        config: match &cfg.source {
            crate::config::ConfigSource::File(path) => path.display().to_string(),
            crate::config::ConfigSource::Defaults => "built-in defaults".into(),
        },
    }
}

/// The folder the service logs to when the configuration cannot be loaded: the default
/// `logs` next to the executable, so the reason is always findable.
pub fn fallback_options() -> FileOptions {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("logs")))
        .unwrap_or_else(|| PathBuf::from("logs"));
    FileOptions {
        dir,
        retention_days: 30,
        compress: true,
        config: "not loaded".into(),
    }
}

/// Checks that the log folder can be created and written, for `check-config`.
pub fn probe_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot be created: {e}"))?;
    let probe = dir.join(".mqrust-log-check");
    std::fs::write(&probe, b"").map_err(|e| format!("is not writable: {e}"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mqrust-log-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(text: &str) -> DateTime<Local> {
        DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S %z")
            .unwrap()
            .with_timezone(&Local)
    }

    fn opts(dir: &Path, compress: bool) -> FileOptions {
        FileOptions {
            dir: dir.to_path_buf(),
            retention_days: 30,
            compress,
            config: "built-in defaults".into(),
        }
    }

    /// A writer on a clock the test moves, with the archiving thread left out.
    fn writer(dir: &Path, now: &Arc<Mutex<DateTime<Local>>>, compress: bool) -> RotatingFile {
        RotatingFile::open(&opts(dir, compress), Clock::fixed(now.clone()))
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn lines(path: &Path) -> Vec<String> {
        read(path).lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn header_opens_every_file() {
        let d = dir("header");
        let now = Arc::new(Mutex::new(at("2026-10-05 10:00:00 +0000")));
        let mut w = writer(&d, &now, false);
        w.write_all(b"first\n").unwrap();
        let first = lines(&d.join(ACTIVE));
        assert!(first[0].contains("ActiveMQRust"), "{first:?}");
        assert!(first[0].contains("log started: pid"), "{first:?}");
        assert!(first[0].contains("config built-in defaults"), "{first:?}");
        assert!(first[0].contains("  INFO ActiveMQRust"), "{first:?}");
        assert_eq!(first[1], "first");
    }

    #[test]
    fn rotates_at_midnight_without_losing_a_line() {
        let d = dir("midnight");
        let now = Arc::new(Mutex::new(at("2026-10-05 23:59:59 +0000")));
        let mut w = writer(&d, &now, false);
        w.write_all(b"before\n").unwrap();
        *now.lock().unwrap() = at("2026-10-06 00:00:01 +0000");
        w.write_all(b"after\n").unwrap();

        let archive = lines(&d.join("mqrust-2026-10-05.log"));
        assert!(archive.contains(&"before".to_string()), "{archive:?}");
        assert!(!archive.contains(&"after".to_string()), "{archive:?}");
        let active = lines(&d.join(ACTIVE));
        assert!(active.contains(&"after".to_string()), "{active:?}");
        assert!(!active.contains(&"before".to_string()), "{active:?}");
        // The new file opens with its own header line.
        assert!(active[0].contains("log started"), "{active:?}");
    }

    #[test]
    fn rotates_without_traffic_after_midnight() {
        let d = dir("quiet");
        let now = Arc::new(Mutex::new(at("2026-10-05 12:00:00 +0000")));
        let mut w = writer(&d, &now, false);
        w.write_all(b"only line\n").unwrap();
        *now.lock().unwrap() = at("2026-10-06 00:00:01 +0000");
        // What the midnight thread does when nothing has been logged.
        w.rotate_if_stale();
        assert!(read(&d.join("mqrust-2026-10-05.log")).contains("only line"));
        assert!(read(&d.join(ACTIVE)).contains("log started"));
    }

    #[test]
    fn startup_rotates_a_file_from_an_earlier_day() {
        let d = dir("startup");
        let active = d.join(ACTIVE);
        std::fs::write(&active, "yesterday\n").unwrap();
        let two_days = std::time::SystemTime::now() - Duration::from_secs(2 * 86_400);
        OpenOptions::new()
            .write(true)
            .open(&active)
            .unwrap()
            .set_modified(two_days)
            .unwrap();
        let day = DateTime::<Local>::from(two_days).date_naive();

        let now = Arc::new(Mutex::new(Local::now()));
        let mut w = writer(&d, &now, false);
        w.write_all(b"today\n").unwrap();

        let archive = d.join(format!("{PREFIX}{}.log", day.format("%Y-%m-%d")));
        assert_eq!(read(&archive), "yesterday\n");
        let now_lines = lines(&active);
        assert!(now_lines[0].contains("log started"), "{now_lines:?}");
        assert_eq!(now_lines[1], "today");
    }

    #[test]
    fn startup_appends_on_the_same_day() {
        let d = dir("append");
        let active = d.join(ACTIVE);
        std::fs::write(&active, "earlier\n").unwrap();
        let now = Arc::new(Mutex::new(Local::now()));
        let mut w = writer(&d, &now, false);
        w.write_all(b"later\n").unwrap();
        let got = lines(&active);
        assert_eq!(got[0], "earlier");
        assert!(got[1].contains("log started"), "{got:?}");
        assert_eq!(got[2], "later");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no archive was made");
    }

    #[test]
    fn archive_name_takes_a_counter() {
        let d = dir("counter");
        std::fs::write(d.join("mqrust-2026-10-05.log.gz"), "old").unwrap();
        let now = Arc::new(Mutex::new(at("2026-10-05 10:00:00 +0000")));
        let mut w = writer(&d, &now, false);
        w.write_all(b"fifth\n").unwrap();
        *now.lock().unwrap() = at("2026-10-06 00:00:01 +0000");
        w.rotate_if_stale();
        assert_eq!(read(&d.join("mqrust-2026-10-05.log.gz")), "old", "kept as it was");
        assert!(read(&d.join("mqrust-2026-10-05-1.log")).contains("fifth"));
    }

    #[test]
    fn compression_keeps_every_line() {
        let d = dir("gzip");
        let archive = d.join("mqrust-2026-10-05.log");
        let text = "one\ntwo\nthree\n".repeat(100);
        std::fs::write(&archive, &text).unwrap();
        gzip(&archive).unwrap();
        assert!(!archive.exists(), "the uncompressed archive is removed");
        let mut out = String::new();
        flate2::read::GzDecoder::new(File::open(d.join("mqrust-2026-10-05.log.gz")).unwrap())
            .read_to_string(&mut out)
            .unwrap();
        assert_eq!(out, text);
    }

    #[test]
    fn startup_recovers_an_interrupted_compression() {
        let d = dir("recover");
        std::fs::write(d.join("mqrust-2026-10-05.log"), "lines\n").unwrap();
        std::fs::write(d.join("mqrust-2026-10-05.log.gz.tmp"), "half written").unwrap();
        let jobs = leftovers(&d, true);
        assert!(!d.join("mqrust-2026-10-05.log.gz.tmp").exists(), "the .tmp is deleted");
        let clock = Clock::local();
        for job in jobs {
            run_job(job, &d, 0, &clock);
        }
        assert!(d.join("mqrust-2026-10-05.log.gz").exists());
        assert!(!d.join("mqrust-2026-10-05.log").exists());
    }

    #[test]
    fn compression_off_keeps_plain_archives() {
        let d = dir("plain");
        std::fs::write(d.join("mqrust-2026-10-05.log"), "lines\n").unwrap();
        assert!(leftovers(&d, false).is_empty(), "nothing is queued for compression");
        let now = Arc::new(Mutex::new(at("2026-10-06 10:00:00 +0000")));
        let mut w = writer(&d, &now, false);
        w.write_all(b"today\n").unwrap();
        *now.lock().unwrap() = at("2026-10-07 00:00:01 +0000");
        w.rotate_if_stale();
        assert!(d.join("mqrust-2026-10-06.log").exists());
        assert!(!d.join("mqrust-2026-10-06.log.gz").exists());
    }

    #[test]
    fn retention_goes_by_the_date_in_the_name() {
        let d = dir("retention");
        let names = [
            "mqrust-2026-09-05.log.gz",
            "mqrust-2026-09-06.log.gz",
            "mqrust-2026-09-05-1.log",
            "mqrust-2026-10-06.log",
            "notes.txt",
            ACTIVE,
        ];
        for n in names {
            std::fs::write(d.join(n), "x").unwrap();
        }
        prune(&d, 30, NaiveDate::from_ymd_opt(2026, 10, 6).unwrap());
        assert!(!d.join("mqrust-2026-09-05.log.gz").exists(), "older than 30 days");
        assert!(!d.join("mqrust-2026-09-05-1.log").exists(), "counter, same day");
        assert!(d.join("mqrust-2026-09-06.log.gz").exists(), "exactly 30 days");
        assert!(d.join("mqrust-2026-10-06.log").exists());
        assert!(d.join("notes.txt").exists(), "foreign files are left alone");
        assert!(d.join(ACTIVE).exists(), "the active file is never pruned");
    }

    #[test]
    fn retention_zero_keeps_everything() {
        let d = dir("keep");
        std::fs::write(d.join("mqrust-2020-01-01.log.gz"), "x").unwrap();
        prune(&d, 0, NaiveDate::from_ymd_opt(2026, 10, 6).unwrap());
        assert!(d.join("mqrust-2020-01-01.log.gz").exists());
    }

    #[test]
    fn only_archive_names_carry_a_date() {
        assert!(archive_date("mqrust-2026-10-05.log").is_some());
        assert!(archive_date("mqrust-2026-10-05.log.gz").is_some());
        assert!(archive_date("mqrust-2026-10-05-12.log.gz").is_some());
        for other in [
            ACTIVE,
            "mqrust-2026-10-05.log.gz.tmp",
            "mqrust-2026-13-05.log",
            "mqrust-2026-10-05-.log",
            "mqrust-2026-10-05-a.log",
            "mqrust.log.gz",
            "notes.txt",
            "other-2026-10-05.log",
        ] {
            assert!(archive_date(other).is_none(), "{other}");
        }
    }

    #[test]
    fn a_folder_that_cannot_be_made_costs_only_the_file() {
        let d = dir("blocked");
        // A file where the folder should be: create_dir_all fails on every platform.
        let blocked = d.join("not-a-folder");
        std::fs::write(&blocked, "x").unwrap();
        let now = Arc::new(Mutex::new(Local::now()));
        let mut w = RotatingFile::open(&opts(&blocked.join("logs"), false), Clock::fixed(now.clone()));
        assert!(w.write_all(b"line\n").is_ok(), "writing never fails the caller");
        assert!(w.flush().is_ok());
    }
}
