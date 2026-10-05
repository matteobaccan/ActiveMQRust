// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Command line and setup: help texts, set-admin, user commands, password rules, safe editing of
//! the configuration file, start-up messages and an end-to-end run with the configured users.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

const EXE: &str = env!("CARGO_BIN_EXE_mqrust");
const ADMIN_PORT: u16 = 8714;
const PASSWORD: &str = "S3cure-pass";

/// OpenWire port of the brokers started here (61714 falls in a Windows excluded port range).
fn port() -> u16 {
    62714
}

/// Brokers started by these tests share the same ports: one at a time.
static BROKER: Mutex<()> = Mutex::new(());

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mqrust-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(EXE)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        if let Some(text) = stdin {
            input.write_all(text.as_bytes()).unwrap();
        }
    }
    child.wait_with_output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn cfg_arg(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

fn set_admin(path: &Path, user: &str) -> Output {
    run(&["set-admin", "--config", &cfg_arg(path), "--username", user, "--password-stdin"], Some(&format!("{PASSWORD}\r\n")))
}

fn user_add(path: &Path, user: &str, password: &str) -> Output {
    run(&["user", "add", user, "--config", &cfg_arg(path), "--password-stdin"], Some(&format!("{password}\n")))
}

fn check_config(path: &Path) -> Output {
    run(&["check-config", "--config", &cfg_arg(path)], None)
}

#[test]
fn short_help_has_getting_started() {
    let o = run(&["-h"], None);
    assert_eq!(code(&o), 0);
    let out = String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n");
    assert!(out.starts_with("ActiveMQRust "), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.len() <= 40, "{} lines:\n{out}", lines.len());
    assert!(lines.iter().all(|l| l.chars().count() <= 100), "{out}");
    for step in [
        "1. mqrust.exe init-config",
        "2. mqrust.exe set-admin",
        "3. mqrust.exe user add <name>",
        "4. mqrust.exe ",
        "mqrust.exe service install",
    ] {
        assert!(out.contains(step), "missing {step}:\n{out}");
    }
    let order = ["Getting started:", "Setup:", "Windows service:", "Configuration:", "Network:", "Performance:"];
    let positions: Vec<usize> = order.iter().map(|h| out.find(h).unwrap_or_else(|| panic!("{h}:\n{out}"))).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{out}");
    assert!(lines.last().unwrap().contains("--help"), "{out}");
}

#[test]
fn long_help_snapshot() {
    let o = run(&["--help"], None);
    assert_eq!(code(&o), 0);
    let out = String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n").replace(env!("CARGO_PKG_VERSION"), "<version>");
    for needle in ["[admin]", "[[users]]", "admin/admin", "mqrust.exe set-admin", "mqrust.exe user add", "Exit codes:", "--config <FILE>", "mqrust.toml next to mqrust.exe", "built-in defaults"] {
        assert!(out.contains(needle), "missing {needle}:\n{out}");
    }
    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/help-long.txt");
    if std::env::var_os("MQRUST_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&snapshot, &out).unwrap();
    }
    let expected = std::fs::read_to_string(&snapshot).unwrap().replace("\r\n", "\n");
    assert_eq!(out, expected, "--help changed: run with MQRUST_UPDATE_SNAPSHOTS=1 to update {}", snapshot.display());
}

#[test]
fn every_command_help_has_an_example() {
    let commands: &[&[&str]] = &[
        &["init-config"],
        &["set-admin"],
        &["user"],
        &["user", "add"],
        &["user", "passwd"],
        &["user", "remove"],
        &["user", "list"],
        &["hash-password"],
        &["check-config"],
        &["service"],
        &["service", "install"],
        &["service", "uninstall"],
        &["service", "start"],
        &["service", "stop"],
        &["service", "status"],
    ];
    for cmd in commands {
        let mut args = cmd.to_vec();
        args.push("--help");
        let o = run(&args, None);
        assert_eq!(code(&o), 0, "{cmd:?}");
        let out = text(&o);
        assert!(out.contains("Example") && out.contains("mqrust.exe "), "{cmd:?}:\n{out}");
    }
}

#[test]
fn command_groups_without_subcommand_print_help() {
    for group in ["user", "service"] {
        let o = run(&[group], None);
        assert_eq!(code(&o), 2, "{group}");
        let out = text(&o);
        assert!(out.contains("Usage:") && out.contains("Example"), "{group}:\n{out}");
    }
}

#[test]
fn first_setup_creates_the_file_from_the_template() {
    let dir = temp_dir("first");
    let path = dir.join("mqrust.toml");
    let o = set_admin(&path, "ops");
    assert_eq!(code(&o), 0, "{}", text(&o));
    let out = text(&o);
    assert!(out.contains("Created"), "{out}");
    assert!(out.contains(&path.display().to_string()), "{out}");
    assert!(out.contains("http://127.0.0.1:8161"), "{out}");
    assert!(out.contains("Restart the broker (or `mqrust.exe service stop` and `service start`) to apply."), "{out}");
    assert!(!out.contains(PASSWORD), "{out}");

    let file = std::fs::read_to_string(&path).unwrap();
    let admin = file.split("
[admin]").nth(1).unwrap().split("\n[").next().unwrap();
    assert!(admin.contains("username = \"ops\""), "{admin}");
    assert!(admin.contains("password_hash = \"$argon2id$"), "{admin}");
    assert!(!admin.contains("password = "), "{admin}");
    assert!(!file.contains(PASSWORD));
    assert!(file.contains("# compress_threshold_kb = 32"), "template comments kept");
    assert_eq!(code(&check_config(&path)), 0);
}

#[test]
fn scripted_setup_users_and_list() {
    let dir = temp_dir("scripted");
    let path = dir.join("mqrust.toml");
    assert_eq!(code(&run(&["init-config", "--config", &cfg_arg(&path)], None)), 0);
    assert_eq!(code(&set_admin(&path, "ops")), 0);
    let o = user_add(&path, "app1", "app1-Secret");
    assert_eq!(code(&o), 0, "{}", text(&o));
    assert!(text(&o).contains("user remove admin"), "{}", text(&o));
    let o = run(&["user", "remove", "admin", "--config", &cfg_arg(&path)], None);
    assert_eq!(code(&o), 0, "{}", text(&o));
    let o = run(&["user", "passwd", "app1", "--config", &cfg_arg(&path), "--password-stdin"], Some("app1-Other\n"));
    assert_eq!(code(&o), 0, "{}", text(&o));
    assert!(text(&o).contains("Restart the broker"));

    let o = run(&["user", "list", "--config", &cfg_arg(&path)], None);
    assert_eq!(code(&o), 0);
    let out = text(&o);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "app1");
    assert!(!out.contains("argon2") && !out.contains("app1-Other"), "{out}");
    assert_eq!(code(&check_config(&path)), 0);
}

#[test]
fn duplicate_user_refused() {
    let dir = temp_dir("duplicate");
    let path = dir.join("mqrust.toml");
    assert_eq!(code(&user_add(&path, "app1", PASSWORD)), 0);
    let before = std::fs::read(&path).unwrap();
    let o = user_add(&path, "app1", "Another-pass");
    assert_eq!(code(&o), 2);
    assert!(text(&o).contains("already exists") && text(&o).contains("user passwd app1"), "{}", text(&o));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn last_user_cannot_be_removed() {
    let dir = temp_dir("last");
    let path = dir.join("mqrust.toml");
    assert_eq!(code(&user_add(&path, "app1", PASSWORD)), 0);
    assert_eq!(code(&run(&["user", "remove", "admin", "--config", &cfg_arg(&path)], None)), 0);
    let before = std::fs::read(&path).unwrap();
    let o = run(&["user", "remove", "app1", "--config", &cfg_arg(&path)], None);
    assert_eq!(code(&o), 2);
    assert!(text(&o).contains("could no longer connect"), "{}", text(&o));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn weak_password_and_bad_username_refused() {
    let dir = temp_dir("weak");
    let path = dir.join("mqrust.toml");
    for weak in ["admin", "short", "password", "ops"] {
        let o = run(&["set-admin", "--config", &cfg_arg(&path), "--username", "ops", "--password-stdin"], Some(&format!("{weak}\n")));
        assert_eq!(code(&o), 2, "{weak}");
        assert!(text(&o).contains("password refused"), "{}", text(&o));
    }
    assert!(!path.exists(), "nothing written for a refused password");
    let o = user_add(&path, "bad name", PASSWORD);
    assert_eq!(code(&o), 2);
    assert!(text(&o).contains("letters, digits, '.', '_', '-' and '@'"), "{}", text(&o));
}

#[test]
fn redirected_stdin_without_password_stdin_fails_fast() {
    let dir = temp_dir("noterm");
    let path = dir.join("mqrust.toml");
    for args in [
        vec!["set-admin", "--config", path.to_str().unwrap()],
        vec!["user", "add", "app1", "--config", path.to_str().unwrap()],
        vec!["hash-password"],
    ] {
        let o = run(&args, Some(""));
        assert_eq!(code(&o), 2, "{args:?}");
        assert!(text(&o).contains("--password-stdin"), "{}", text(&o));
    }
    assert!(!path.exists());
}

#[test]
fn hash_password_from_stdin() {
    let o = run(&["hash-password", "--password-stdin"], Some(&format!("{PASSWORD}\r\n")));
    assert_eq!(code(&o), 0, "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stdout).trim().starts_with("$argon2id$"));
}

#[test]
fn invalid_file_left_unchanged() {
    let dir = temp_dir("invalid");
    let path = dir.join("mqrust.toml");
    let broken = "[broker]\nport = 61616\nname = = \"x\"\n";
    std::fs::write(&path, broken).unwrap();
    let o = user_add(&path, "app1", PASSWORD);
    assert_eq!(code(&o), 2);
    assert!(text(&o).contains("line 3"), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
}

#[test]
fn comments_and_other_settings_kept() {
    let dir = temp_dir("comments");
    let path = dir.join("mqrust.toml");
    let head = "# Production broker\r\n\r\n[broker]\r\nport = 61700   # agreed with the network team\r\nmax_memory_mb = 2048\r\n\r\n";
    let tail = "\r\n[[users]]\r\nusername = \"app1\"\r\npassword = \"app1-secret\"\r\n";
    std::fs::write(&path, format!("{head}[admin]\r\nusername = \"admin\"\r\npassword = \"admin\"\r\n{tail}")).unwrap();
    assert_eq!(code(&set_admin(&path, "ops")), 0);
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(file.starts_with(head), "{file}");
    assert!(file.ends_with(tail), "{file}");
    assert!(file.contains("username = \"ops\""), "{file}");
    assert_eq!(code(&check_config(&path)), 0);
}

#[test]
fn example_file_matches_the_template() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("mqrust.example.toml");
    let content = std::fs::read_to_string(&example).unwrap().replace("\r\n", "\n");
    assert_eq!(
        content,
        mqrust::config::TEMPLATE,
        "mqrust.example.toml differs from the init-config template: regenerate it with \
         `mqrust.exe init-config --config mqrust.example.toml` after deleting it"
    );
    let o = check_config(&example);
    assert_eq!(code(&o), 0, "{}", text(&o));
    let cfg = mqrust::config::load(Some(&example), &Default::default()).unwrap();
    let defaults = mqrust::config::build(Default::default(), mqrust::config::ConfigSource::Defaults, &Default::default()).unwrap();
    assert_eq!(cfg.port, defaults.port);
    assert_eq!(cfg.admin_port, defaults.admin_port);
    assert_eq!(cfg.compress_threshold_bytes, defaults.compress_threshold_bytes);
    assert_eq!(cfg.compress_min_saving_pct, defaults.compress_min_saving_pct);
    assert_eq!(cfg.expiry_check_interval_ms, defaults.expiry_check_interval_ms);
    assert_eq!(cfg.max_frame_size, defaults.max_frame_size);
}

/// A broker process started by a test; killed when dropped.
struct Broker {
    child: Child,
    log: mpsc::Receiver<String>,
    lines: Vec<String>,
}

impl Broker {
    fn start(config: Option<&Path>) -> Broker {
        let mut cmd = Command::new(EXE);
        cmd.args(["--port", &port().to_string(), "--admin-port", &ADMIN_PORT.to_string()]);
        if let Some(c) = config {
            cmd.args(["--config", c.to_str().unwrap()]);
        }
        let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let (tx, rx) = mpsc::channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut b = Broker { child, log: rx, lines: Vec::new() };
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !b.lines.iter().any(|l| l.ends_with(" ready")) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match b.log.recv_timeout(left) {
                Ok(l) => b.lines.push(l),
                Err(_) => panic!("broker not ready:\n{}", b.lines.join("\n")),
            }
        }
        b
    }

    fn log(&self) -> String {
        self.lines.join("\n")
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn startup_log_with_defaults() {
    let next_to_exe = Path::new(EXE).with_file_name("mqrust.toml");
    if next_to_exe.exists() {
        eprintln!("skipped: {} exists, the broker would not use the built-in defaults", next_to_exe.display());
        return;
    }
    let _guard = BROKER.lock().unwrap_or_else(|e| e.into_inner());
    let b = Broker::start(None);
    let log = b.log();
    assert!(log.contains("configuration: built-in defaults"), "{log}");
    assert!(log.contains(&format!("OpenWire listening on 0.0.0.0:{}", port())), "{log}");
    assert!(log.contains(&format!("admin console on http://127.0.0.1:{ADMIN_PORT} (login with the [admin] user admin)")), "{log}");
    assert!(log.contains("1 messaging user"), "{log}");
    let warn = log.lines().find(|l| l.contains("WARN") && l.contains("admin/admin")).unwrap_or_else(|| panic!("{log}"));
    assert!(warn.contains("mqrust.exe set-admin") && warn.contains("mqrust.exe user add <name>"), "{warn}");
    assert!(!log.contains("hint: run `mqrust.exe init-config`"), "no hint without a console:\n{log}");
}

fn configured(dir: &Path) -> PathBuf {
    let path = dir.join("mqrust.toml");
    assert_eq!(code(&set_admin(&path, "ops")), 0);
    assert_eq!(code(&user_add(&path, "app1", "app1-Secret")), 0);
    assert_eq!(code(&user_add(&path, "app2", "app2-Secret")), 0);
    assert_eq!(code(&run(&["user", "remove", "admin", "--config", &cfg_arg(&path)], None)), 0);
    path
}

#[test]
fn startup_log_with_configured_file() {
    let dir = temp_dir("startlog");
    let path = configured(&dir);
    let _guard = BROKER.lock().unwrap_or_else(|e| e.into_inner());
    let b = Broker::start(Some(&path));
    let log = b.log();
    assert!(log.contains(&format!("configuration: {}", path.display())), "{log}");
    assert!(log.contains(&format!("OpenWire listening on 0.0.0.0:{}", port())), "{log}");
    assert!(log.contains(&format!("admin console on http://127.0.0.1:{ADMIN_PORT} (login with the [admin] user ops)")), "{log}");
    assert!(log.contains("2 messaging users"), "{log}");
    assert!(!log.contains("WARN"), "{log}");
}

fn http_status(path: &str, user: &str, password: &str) -> u16 {
    use base64::Engine;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", ADMIN_PORT)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {auth}\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    let _ = s.read_to_string(&mut response);
    response.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
}

/// End to end: users set with the commands, broker started with that file, console login with the
/// new admin and, when the Java acceptance jar is built, a JMS client connected as the new user.
#[test]
fn end_to_end_with_configured_users() {
    let dir = temp_dir("e2e");
    let path = configured(&dir);
    let _guard = BROKER.lock().unwrap_or_else(|e| e.into_inner());
    let _b = Broker::start(Some(&path));
    assert_eq!(http_status("/api/overview", "ops", PASSWORD), 200);
    assert_eq!(http_status("/api/overview", "admin", "admin"), 401);

    let url = format!("tcp://127.0.0.1:{}", port());
    for profile in ["amq5", "amq6"] {
        let jar = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/java-it/target/{profile}/mqrust-acceptance.jar"));
        if !jar.exists() {
            eprintln!("Java client part skipped: build {} first", jar.display());
            continue;
        }
        let args = ["-jar", jar.to_str().unwrap(), "accept", "--url", &url, "--user", "app1", "--password", "app1-Secret", "--only", "1"];
        match Command::new("java").args(args).output() {
            Ok(o) => assert!(o.status.success(), "{profile}: {}", text(&o)),
            Err(e) => eprintln!("Java client part skipped: {e}"),
        }
    }
}
