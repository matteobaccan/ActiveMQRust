// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Configuration: optional TOML file, built-in defaults and command-line overrides.

use serde::Deserialize;
use std::collections::HashSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

pub const DEFAULT_USER: &str = "admin";
pub const DEFAULT_PASSWORD: &str = "admin";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BrokerSection {
    pub name: String,
    pub bind: String,
    pub port: i64,
    pub max_frame_size_mb: i64,
    pub socket_buffer_kb: i64,
    pub processors: i64,
    pub allow_anonymous: bool,
    pub max_memory_mb: i64,
    pub auto_delete_empty_after_secs: i64,
    pub topic_max_pending_per_consumer: i64,
    pub compress_threshold_kb: i64,
    pub compress_min_saving_pct: i64,
    /// Open the plain OpenWire listener; `false` leaves only the TLS one.
    pub plain: bool,
}

impl Default for BrokerSection {
    fn default() -> Self {
        BrokerSection {
            name: "ActiveMQRust".into(),
            bind: "0.0.0.0".into(),
            port: 61616,
            max_frame_size_mb: 100,
            socket_buffer_kb: 1024,
            processors: 0,
            allow_anonymous: false,
            max_memory_mb: 0,
            auto_delete_empty_after_secs: 0,
            topic_max_pending_per_consumer: 10_000,
            compress_threshold_kb: 0,
            compress_min_saving_pct: 10,
            plain: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExpirySection {
    pub check_interval_ms: i64,
    pub use_broker_clock: bool,
    pub ttl_ceiling_ms: i64,
    pub default_ttl_ms: i64,
}

impl Default for ExpirySection {
    fn default() -> Self {
        ExpirySection {
            check_interval_ms: 1000,
            use_broker_clock: false,
            ttl_ceiling_ms: 0,
            default_ttl_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AdminSection {
    pub bind: String,
    pub port: i64,
    pub username: String,
    pub password: Option<String>,
    pub password_hash: Option<String>,
    pub session_idle_minutes: i64,
    pub session_max_hours: i64,
    pub login_max_failures: i64,
    pub login_lockout_seconds: i64,
    /// Refuse every console and API write operation.
    pub read_only: bool,
}

impl Default for AdminSection {
    fn default() -> Self {
        AdminSection {
            bind: "127.0.0.1".into(),
            port: 8161,
            username: DEFAULT_USER.into(),
            password: None,
            password_hash: None,
            session_idle_minutes: 30,
            session_max_hours: 8,
            login_max_failures: 5,
            login_lockout_seconds: 60,
            read_only: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TlsSection {
    pub enabled: bool,
    /// Empty takes `broker.bind`.
    pub bind: String,
    pub port: i64,
    /// PEM certificate chain, leaf first, with its PEM private key.
    pub cert: String,
    pub key: String,
    /// PKCS#12 keystore and its password, instead of `cert` and `key`.
    pub keystore: String,
    pub keystore_password: String,
    /// Lowest version accepted: `1.2` or `1.3`.
    pub min_version: String,
    /// `none`, `optional` or `required`.
    pub client_auth: String,
    /// PEM bundle the client certificates must chain to.
    pub client_ca: String,
}

impl Default for TlsSection {
    fn default() -> Self {
        TlsSection {
            enabled: false,
            bind: String::new(),
            port: 61617,
            cert: String::new(),
            key: String::new(),
            keystore: String::new(),
            keystore_password: String::new(),
            min_version: "1.2".into(),
            client_auth: "none".into(),
            client_ca: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogSection {
    pub level: String,
    /// Folder of the log files; relative paths are resolved against the executable.
    pub dir: String,
    /// In console mode, write to the log file as well as to stdout.
    pub file: bool,
    /// Days an archive is kept; 0 keeps every archive.
    pub retention_days: i64,
    pub compress: bool,
}

impl Default for LogSection {
    fn default() -> Self {
        LogSection {
            level: "info".into(),
            dir: "logs".into(),
            file: true,
            retention_days: 30,
            compress: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserEntry {
    pub username: String,
    pub password: Option<String>,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct FileConfig {
    pub broker: BrokerSection,
    pub expiry: ExpirySection,
    pub admin: AdminSection,
    pub tls: TlsSection,
    pub log: LogSection,
    pub users: Vec<UserEntry>,
}

/// A secret as configured: plain text or Argon2 hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Secret {
    Plain(String),
    Hash(String),
}

#[derive(Debug, Clone)]
pub struct User {
    pub username: String,
    pub secret: Secret,
}

/// Validated, ready-to-use configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub source: ConfigSource,
    pub broker_name: String,
    pub bind: IpAddr,
    pub port: u16,
    pub max_frame_size: i64,
    /// TCP send/receive buffer of OpenWire sockets; 0 = operating system default.
    pub socket_buffer_bytes: u32,
    /// Processors the broker uses; 0 = those available to the process (see `cpu`).
    pub processors: usize,
    pub allow_anonymous: bool,
    /// The plain OpenWire listener is open.
    pub plain: bool,
    pub tls_enabled: bool,
    pub tls_bind: IpAddr,
    pub tls_port: u16,
    /// Resolved against the folder of the configuration file.
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_keystore: Option<PathBuf>,
    pub tls_keystore_password: String,
    pub tls_min_version: String,
    pub tls_client_auth: String,
    pub tls_client_ca: Option<PathBuf>,
    pub max_memory_bytes: u64,
    pub auto_delete_empty_after_secs: u64,
    pub topic_max_pending_per_consumer: u64,
    pub compress_threshold_bytes: u64,
    pub compress_min_saving_pct: u64,
    pub expiry_check_interval_ms: u64,
    pub use_broker_clock: bool,
    pub ttl_ceiling_ms: u64,
    pub default_ttl_ms: u64,
    pub admin_bind: IpAddr,
    pub admin_port: u16,
    pub admin_user: User,
    /// Console session idle timeout, in minutes.
    pub admin_session_idle_minutes: u64,
    /// Console session absolute lifetime, in hours.
    pub admin_session_max_hours: u64,
    /// Failed console logins from one IP before a lockout; 0 = no throttling.
    pub admin_login_max_failures: u32,
    pub admin_login_lockout_seconds: u64,
    /// The console and the API refuse every write operation.
    pub admin_read_only: bool,
    /// Where the bans are stored: next to the configuration file, or to the executable.
    pub bans_path: PathBuf,
    pub users: Vec<User>,
    pub log_level: String,
    /// Folder of the log files, already resolved against the executable.
    pub log_dir: PathBuf,
    /// Console mode writes to the log file as well as to stdout.
    pub log_to_file: bool,
    pub log_retention_days: u64,
    pub log_compress: bool,
    /// True when built-in default credentials are in use.
    pub default_credentials: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    File(PathBuf),
    Defaults,
}

/// Values given on the command line; they win over the file.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub bind: Option<String>,
    pub port: Option<u16>,
    pub admin_bind: Option<String>,
    pub admin_port: Option<u16>,
    pub processors: Option<i64>,
    pub log_level: Option<String>,
}

/// A configuration error; the message names the offending field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Path of `mqrust.toml` next to the executable.
pub fn default_config_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mqrust.toml")))
}

/// Locates and loads the configuration: `--config`, then next to the executable, then defaults.
pub fn load(explicit: Option<&Path>, overrides: &Overrides) -> Result<Config, ConfigError> {
    match explicit {
        Some(path) => {
            if !path.exists() {
                return Err(ConfigError(format!("configuration file not found: {}", path.display())));
            }
            let text = std::fs::read_to_string(path)
                .map_err(|e| ConfigError(format!("cannot read {}: {e}", path.display())))?;
            from_toml(&text, ConfigSource::File(path.to_path_buf()), overrides)
        }
        None => match default_config_path().filter(|p| p.exists()) {
            Some(path) => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| ConfigError(format!("cannot read {}: {e}", path.display())))?;
                from_toml(&text, ConfigSource::File(path), overrides)
            }
            None => build(FileConfig::default(), ConfigSource::Defaults, overrides),
        },
    }
}

pub fn from_toml(text: &str, source: ConfigSource, overrides: &Overrides) -> Result<Config, ConfigError> {
    let file: FileConfig = toml::from_str(text).map_err(|e| ConfigError(describe_toml_error(text, &e)))?;
    build(file, source, overrides)
}

fn describe_toml_error(text: &str, e: &toml::de::Error) -> String {
    let msg = e.message().trim().to_string();
    let offset = e.span().map(|s| s.start.min(text.len()));
    let section = offset.and_then(|o| section_at(text, o));
    let qualify = |key: &str| match &section {
        Some(s) => format!("{s}.{key}"),
        None => key.to_string(),
    };
    // Unknown keys: make the full key path explicit, e.g. "unknown key broker.prot".
    if let Some(rest) = msg.strip_prefix("unknown field `") {
        let field = rest.split('`').next().unwrap_or(rest);
        return format!("configuration error: unknown key {}", qualify(field));
    }
    // Other errors (wrong type, bad syntax): name the key on the offending line.
    match offset.and_then(|o| key_at(text, o)) {
        Some(key) => format!("configuration error: {}: {msg}", qualify(&key)),
        None => format!("configuration error: {msg}"),
    }
}

/// The table that contains byte `offset` of the source: `broker`, `users[1]`, or `None` at top level.
fn section_at(text: &str, offset: usize) -> Option<String> {
    let mut section = None;
    let mut users = 0usize;
    for line in text[..offset].lines() {
        let l = line.trim();
        if let Some(name) = l.strip_prefix("[[").and_then(|r| r.split("]]").next()) {
            let name = name.trim();
            if name == "users" {
                users += 1;
                section = Some(format!("users[{}]", users - 1));
            } else {
                section = Some(name.to_string());
            }
        } else if let Some(name) = l.strip_prefix('[').and_then(|r| r.split(']').next()) {
            section = Some(name.trim().to_string());
        }
    }
    section
}

/// The key assigned on the line that contains byte `offset` of the source.
fn key_at(text: &str, offset: usize) -> Option<String> {
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line = text[start..].lines().next()?;
    let (key, _) = line.split_once('=')?;
    let key = key.trim().trim_matches('"');
    (!key.is_empty() && !key.starts_with('[') && !key.starts_with('#')).then(|| key.to_string())
}

fn secret(field: &str, password: &Option<String>, hash: &Option<String>) -> Result<Option<Secret>, ConfigError> {
    match (password, hash) {
        (Some(_), Some(_)) => Err(ConfigError(format!(
            "configuration error: {field} has both password and password_hash; set exactly one"
        ))),
        (Some(p), None) => Ok(Some(Secret::Plain(p.clone()))),
        (None, Some(h)) => {
            if !h.starts_with("$argon2") {
                return Err(ConfigError(format!(
                    "configuration error: {field}.password_hash is not an Argon2 hash"
                )));
            }
            Ok(Some(Secret::Hash(h.clone())))
        }
        (None, None) => Ok(None),
    }
}

fn parse_ip(field: &str, value: &str) -> Result<IpAddr, ConfigError> {
    value.parse().map_err(|_| {
        ConfigError(format!(
            "configuration error: {field} = \"{value}\" is not a valid IP address"
        ))
    })
}

fn parse_port(field: &str, value: i64) -> Result<u16, ConfigError> {
    if (1..=65535).contains(&value) {
        Ok(value as u16)
    } else {
        Err(ConfigError(format!(
            "configuration error: {field} = {value} is not a valid port (1-65535)"
        )))
    }
}

/// An integer that must lie in `min..=max`; the error names the key.
fn in_range(field: &str, value: i64, min: i64, max: i64) -> Result<u64, ConfigError> {
    if (min..=max).contains(&value) {
        Ok(value as u64)
    } else {
        Err(ConfigError(format!(
            "configuration error: {field} = {value} must be between {min} and {max}"
        )))
    }
}

/// One of the five log levels, lower-cased; the error names where the value came from.
fn check_level(field: &str, value: &str) -> Result<String, ConfigError> {
    let level = value.to_ascii_lowercase();
    if ["error", "warn", "info", "debug", "trace"].contains(&level.as_str()) {
        Ok(level)
    } else {
        Err(ConfigError(format!(
            "configuration error: {field} = \"{value}\" must be error, warn, info, debug or trace"
        )))
    }
}

/// A file named in the configuration, resolved against the folder that holds it; an empty
/// name means the key was not set.
fn file_in(dir: &Option<PathBuf>, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let path = Path::new(name);
    match dir {
        Some(dir) if path.is_relative() => Some(dir.join(path)),
        _ => Some(path.to_path_buf()),
    }
}

/// A relative path is resolved against the folder of the executable, as `mqrust.toml` is.
fn resolve_from_exe(dir: &str) -> PathBuf {
    let path = Path::new(dir.trim());
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(path)))
        .unwrap_or_else(|| path.to_path_buf())
}

fn non_negative(field: &str, value: i64) -> Result<u64, ConfigError> {
    if value < 0 {
        Err(ConfigError(format!(
            "configuration error: {field} = {value} must not be negative"
        )))
    } else {
        Ok(value as u64)
    }
}

pub fn build(file: FileConfig, source: ConfigSource, overrides: &Overrides) -> Result<Config, ConfigError> {
    let b = &file.broker;
    let bind = parse_ip("broker.bind", overrides.bind.as_deref().unwrap_or(&b.bind))?;
    let port = match overrides.port {
        Some(p) => p,
        None => parse_port("broker.port", b.port)?,
    };
    if b.max_frame_size_mb < 1 {
        return Err(ConfigError(format!(
            "configuration error: broker.max_frame_size_mb = {} must be at least 1",
            b.max_frame_size_mb
        )));
    }
    let socket_buffer_kb = non_negative("broker.socket_buffer_kb", b.socket_buffer_kb)?;
    if socket_buffer_kb > 64 * 1024 {
        return Err(ConfigError(format!(
            "configuration error: broker.socket_buffer_kb = {socket_buffer_kb} must be at most 65536"
        )));
    }
    let processors = overrides.processors.unwrap_or(b.processors);
    if !(0..=1024).contains(&processors) {
        return Err(ConfigError(format!(
            "configuration error: broker.processors = {processors} must be between 0 and 1024"
        )));
    }
    let max_memory_mb = non_negative("broker.max_memory_mb", b.max_memory_mb)?;
    let auto_delete = non_negative("broker.auto_delete_empty_after_secs", b.auto_delete_empty_after_secs)?;
    let topic_pending = non_negative(
        "broker.topic_max_pending_per_consumer",
        b.topic_max_pending_per_consumer,
    )?;
    let threshold_kb = non_negative("broker.compress_threshold_kb", b.compress_threshold_kb)?;
    if !(0..=99).contains(&b.compress_min_saving_pct) {
        return Err(ConfigError(format!(
            "configuration error: broker.compress_min_saving_pct = {} must be between 0 and 99",
            b.compress_min_saving_pct
        )));
    }
    let e = &file.expiry;
    if e.check_interval_ms < 1 {
        return Err(ConfigError(format!(
            "configuration error: expiry.check_interval_ms = {} must be at least 1",
            e.check_interval_ms
        )));
    }
    let ttl_ceiling = non_negative("expiry.ttl_ceiling_ms", e.ttl_ceiling_ms)?;
    let default_ttl = non_negative("expiry.default_ttl_ms", e.default_ttl_ms)?;

    let a = &file.admin;
    let admin_bind = parse_ip("admin.bind", overrides.admin_bind.as_deref().unwrap_or(&a.bind))?;
    let admin_port = match overrides.admin_port {
        Some(p) => p,
        None => parse_port("admin.port", a.port)?,
    };
    let session_idle = in_range("admin.session_idle_minutes", a.session_idle_minutes, 1, 1440)?;
    let session_max = in_range("admin.session_max_hours", a.session_max_hours, 1, 168)?;
    let max_failures = in_range("admin.login_max_failures", a.login_max_failures, 0, 100)?;
    let lockout = in_range("admin.login_lockout_seconds", a.login_lockout_seconds, 1, 86_400)?;

    let l = &file.log;
    let level = match &overrides.log_level {
        Some(v) => check_level("--log-level", v)?,
        None => check_level("log.level", &l.level)?,
    };
    if l.dir.trim().is_empty() {
        return Err(ConfigError("configuration error: log.dir must not be empty".into()));
    }
    let retention = in_range("log.retention_days", l.retention_days, 0, 3650)?;

    let t = &file.tls;
    let tls_bind = parse_ip("tls.bind", if t.bind.is_empty() { &b.bind } else { &t.bind })?;
    let tls_port = parse_port("tls.port", t.port)?;
    if t.enabled {
        let pem = !t.cert.is_empty() || !t.key.is_empty();
        let p12 = !t.keystore.is_empty();
        match (pem, p12) {
            (true, true) => {
                return Err(ConfigError(
                    "configuration error: set either tls.cert and tls.key, or tls.keystore, not both".into(),
                ))
            }
            (false, false) => {
                return Err(ConfigError(
                    "configuration error: tls.enabled = true needs tls.cert and tls.key, or tls.keystore".into(),
                ))
            }
            (true, false) if t.cert.is_empty() => {
                return Err(ConfigError(
                    "configuration error: tls.cert is required with tls.key".into(),
                ))
            }
            (true, false) if t.key.is_empty() => {
                return Err(ConfigError(
                    "configuration error: tls.key is required with tls.cert".into(),
                ))
            }
            _ => {}
        }
        if !["1.2", "1.3"].contains(&t.min_version.as_str()) {
            return Err(ConfigError(format!(
                "configuration error: tls.min_version = \"{}\" must be 1.2 or 1.3",
                t.min_version
            )));
        }
        if !["none", "optional", "required"].contains(&t.client_auth.as_str()) {
            return Err(ConfigError(format!(
                "configuration error: tls.client_auth = \"{}\" must be none, optional or required",
                t.client_auth
            )));
        }
        if t.client_auth != "none" && t.client_ca.is_empty() {
            return Err(ConfigError(format!(
                "configuration error: tls.client_ca is required with tls.client_auth = \"{}\"",
                t.client_auth
            )));
        }
        if b.plain && tls_bind == bind && tls_port == port {
            return Err(ConfigError(format!(
                "configuration error: tls.port = {tls_port} is the address and port of the plain listener"
            )));
        }
    } else if !b.plain {
        return Err(ConfigError(
            "configuration error: broker.plain = false needs tls.enabled = true, or nothing would listen".into(),
        ));
    }

    let config_dir = match &source {
        ConfigSource::File(path) => path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(|d| d.to_path_buf()),
        ConfigSource::Defaults => std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf)),
    };
    let bans_path = match &source {
        ConfigSource::File(path) => path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from(crate::bans::FILE), |d| d.join(crate::bans::FILE)),
        ConfigSource::Defaults => resolve_from_exe(crate::bans::FILE),
    };
    let is_file = matches!(source, ConfigSource::File(_));
    let mut default_credentials = false;

    let admin_secret = secret("admin", &a.password, &a.password_hash)?;
    let admin_user = match admin_secret {
        Some(s) => User {
            username: a.username.clone(),
            secret: s,
        },
        None if !is_file => {
            default_credentials = true;
            User {
                username: DEFAULT_USER.into(),
                secret: Secret::Plain(DEFAULT_PASSWORD.into()),
            }
        }
        None => {
            return Err(ConfigError(
                "configuration error: admin.password or admin.password_hash is required".into(),
            ))
        }
    };

    let mut users = Vec::new();
    let mut seen = HashSet::new();
    for (i, u) in file.users.iter().enumerate() {
        let field = format!("users[{i}] ({})", u.username);
        if u.username.is_empty() {
            return Err(ConfigError(format!(
                "configuration error: users[{i}].username must not be empty"
            )));
        }
        if !seen.insert(u.username.clone()) {
            return Err(ConfigError(format!(
                "configuration error: duplicate username \"{}\"",
                u.username
            )));
        }
        let s = secret(&field, &u.password, &u.password_hash)?
            .ok_or_else(|| ConfigError(format!("configuration error: {field} needs password or password_hash")))?;
        users.push(User {
            username: u.username.clone(),
            secret: s,
        });
    }
    if users.is_empty() {
        if is_file && !b.allow_anonymous {
            return Err(ConfigError(
                "configuration error: at least one [[users]] entry is required unless broker.allow_anonymous = true"
                    .into(),
            ));
        }
        if !is_file {
            default_credentials = true;
            users.push(User {
                username: DEFAULT_USER.into(),
                secret: Secret::Plain(DEFAULT_PASSWORD.into()),
            });
        }
    }

    Ok(Config {
        source,
        broker_name: b.name.clone(),
        bind,
        port,
        max_frame_size: b.max_frame_size_mb * 1024 * 1024,
        socket_buffer_bytes: (socket_buffer_kb * 1024) as u32,
        processors: processors as usize,
        allow_anonymous: b.allow_anonymous,
        plain: b.plain,
        tls_enabled: t.enabled,
        tls_bind,
        tls_port,
        tls_cert: file_in(&config_dir, &t.cert),
        tls_key: file_in(&config_dir, &t.key),
        tls_keystore: file_in(&config_dir, &t.keystore),
        tls_keystore_password: t.keystore_password.clone(),
        tls_min_version: t.min_version.clone(),
        tls_client_auth: t.client_auth.clone(),
        tls_client_ca: file_in(&config_dir, &t.client_ca),
        max_memory_bytes: max_memory_mb * 1024 * 1024,
        auto_delete_empty_after_secs: auto_delete,
        topic_max_pending_per_consumer: topic_pending,
        compress_threshold_bytes: threshold_kb * 1024,
        compress_min_saving_pct: b.compress_min_saving_pct as u64,
        expiry_check_interval_ms: e.check_interval_ms as u64,
        use_broker_clock: e.use_broker_clock,
        ttl_ceiling_ms: ttl_ceiling,
        default_ttl_ms: default_ttl,
        admin_bind,
        admin_port,
        admin_user,
        admin_session_idle_minutes: session_idle,
        admin_session_max_hours: session_max,
        admin_login_max_failures: max_failures as u32,
        admin_login_lockout_seconds: lockout,
        admin_read_only: a.read_only,
        bans_path,
        users,
        log_level: level,
        log_dir: resolve_from_exe(&l.dir),
        log_to_file: l.file,
        log_retention_days: retention,
        log_compress: l.compress,
        default_credentials,
    })
}

impl Config {
    pub fn plain_text_in_use(&self) -> bool {
        matches!(self.admin_user.secret, Secret::Plain(_)) && !self.default_credentials
            || self.users.iter().any(|u| matches!(u.secret, Secret::Plain(_))) && !self.default_credentials
    }
}

/// Commented configuration written by `init-config`.
pub const TEMPLATE: &str = r#"# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# ActiveMQRust configuration. Every key is optional; missing keys take the default shown.
# Changes apply when the broker (or the Windows service) is restarted.
#
# Set the users with the command line instead of editing this file by hand:
#   mqrust.exe set-admin            admin console user ([admin])
#   mqrust.exe user add <name>      messaging users of the JMS/OpenWire clients ([[users]])
#   mqrust.exe check-config         validate this file

[broker]
# name = "ActiveMQRust"
# bind = "0.0.0.0"                     # OpenWire listen address
# port = 61616                         # OpenWire port
# max_frame_size_mb = 100
# socket_buffer_kb = 1024              # TCP send/receive buffer per connection; 0 = OS default
# processors = 0                       # 0 = processors in the process affinity (like the JVM)
# allow_anonymous = false
# max_memory_mb = 0                    # 0 = no limit
# auto_delete_empty_after_secs = 0     # 0 = keep empty queues
# topic_max_pending_per_consumer = 10000
# compress_threshold_kb = 0            # 0 = never (default, best throughput); e.g. 256 = compress bodies above 256 KB to save RAM
# compress_min_saving_pct = 10
# plain = true                         # false leaves only the TLS listener (needs [tls] enabled)

[expiry]
# check_interval_ms = 1000
# use_broker_clock = false
# ttl_ceiling_ms = 0                   # 0 = no ceiling
# default_ttl_ms = 0                   # 0 = messages without TTL never expire

[admin]
# bind = "127.0.0.1"
# port = 8161
username = "admin"
password = "admin"                     # replace with password_hash = "..." (mqrust.exe hash-password)
# session_idle_minutes = 30            # console session ends after this idle time (1-1440)
# session_max_hours = 8                # and in any case this long after login (1-168)
# login_max_failures = 5               # failed logins per IP in 15 minutes before a lockout; 0 = off
# login_lockout_seconds = 60           # lockout length (1-86400)
# read_only = false                    # true refuses every write operation of the console and API

# TLS for OpenWire (ssl://host:61617), off by default. Give either cert and key (PEM),
# or keystore and keystore_password (PKCS#12); paths are relative to this file.
[tls]
# enabled = false
# bind = "0.0.0.0"                     # default: the same as broker.bind
# port = 61617
# cert = "broker.crt"                  # PEM chain, leaf first
# key = "broker.key"                   # PEM private key, unencrypted
# keystore = "broker.p12"              # PKCS#12 keystore, instead of cert and key
# keystore_password = ""
# min_version = "1.2"                  # 1.2 | 1.3
# client_auth = "none"                 # none | optional | required
# client_ca = "clients-ca.crt"         # PEM bundle, required unless client_auth = "none"

[log]
# level = "info"                       # error | warn | info | debug | trace (--log-level overrides it)
# dir = "logs"                         # log folder; relative to mqrust.exe, or an absolute path
# file = true                          # in console mode write to the file as well as to stdout
# retention_days = 30                  # delete archives older than this; 0 = keep every archive
# compress = true                      # compress the archive of each day to .log.gz

# Messaging users: one [[users]] entry each, with password_hash (Argon2id) or password.
# Replace this default user: mqrust.exe user add <name>, then mqrust.exe user remove admin
[[users]]
username = "admin"
password = "admin"                     # replace with password_hash = "..." (mqrust.exe user passwd admin)
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> Result<Config, ConfigError> {
        from_toml(text, ConfigSource::File(PathBuf::from("t.toml")), &Overrides::default())
    }

    const MIN: &str = "[admin]\npassword = \"a\"\n[[users]]\nusername = \"u\"\npassword = \"p\"\n";

    #[test]
    fn defaults_without_file() {
        let c = build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap();
        assert_eq!(c.port, 61616);
        assert_eq!(c.bind.to_string(), "0.0.0.0");
        assert_eq!(c.admin_bind.to_string(), "127.0.0.1");
        assert_eq!(c.admin_port, 8161);
        assert_eq!(c.users[0].username, "admin");
        assert!(c.default_credentials);
        assert_eq!(c.broker_name, "ActiveMQRust");
        assert_eq!(c.admin_user.username, "admin");
        assert_eq!(c.admin_user.secret, Secret::Plain("admin".into()));
        assert_eq!(c.admin_session_idle_minutes, 30);
        assert_eq!(c.admin_session_max_hours, 8);
        assert_eq!(c.admin_login_max_failures, 5);
        assert_eq!(c.admin_login_lockout_seconds, 60);
    }

    #[test]
    fn admin_session_keys() {
        let keys =
            "session_idle_minutes = 5\nsession_max_hours = 1\nlogin_max_failures = 0\nlogin_lockout_seconds = 86400\n";
        let c = cfg(&MIN.replace("password = \"a\"\n", &format!("password = \"a\"\n{keys}"))).unwrap();
        assert_eq!(c.admin_session_idle_minutes, 5);
        assert_eq!(c.admin_session_max_hours, 1);
        assert_eq!(c.admin_login_max_failures, 0);
        assert_eq!(c.admin_login_lockout_seconds, 86_400);
        for (key, bad) in [
            ("session_idle_minutes", "0"),
            ("session_idle_minutes", "1441"),
            ("session_max_hours", "0"),
            ("session_max_hours", "169"),
            ("login_max_failures", "-1"),
            ("login_max_failures", "101"),
            ("login_lockout_seconds", "0"),
            ("login_lockout_seconds", "86401"),
        ] {
            let text = MIN.replace("password = \"a\"\n", &format!("password = \"a\"\n{key} = {bad}\n"));
            let e = cfg(&text).unwrap_err();
            assert!(e.0.contains(&format!("admin.{key}")), "{}", e.0);
        }
    }

    #[test]
    fn partial_file_takes_defaults() {
        let c = cfg(MIN).unwrap();
        assert_eq!(c.port, 61616);
        assert_eq!(c.compress_threshold_bytes, 0);
        assert_eq!(c.socket_buffer_bytes, 1024 * 1024);
        assert_eq!(c.processors, 0);
        assert_eq!(c.socket_buffer_bytes, 1024 * 1024);
        assert!(!c.default_credentials);
    }

    #[test]
    fn overrides_win() {
        let o = Overrides {
            port: Some(61620),
            ..Default::default()
        };
        let text = format!("[broker]\nport = 61617\n{MIN}");
        let c = from_toml(&text, ConfigSource::File("t".into()), &o).unwrap();
        assert_eq!(c.port, 61620);
    }

    #[test]
    fn unknown_key_is_named() {
        let e = cfg(&format!("[broker]\nprot = 1\n{MIN}")).unwrap_err();
        assert!(e.0.contains("unknown key broker.prot"), "{}", e.0);
        let e = cfg(&format!("{MIN}[expiry]\nttl = 5\n")).unwrap_err();
        assert!(e.0.contains("unknown key expiry.ttl"), "{}", e.0);
        let e = cfg(&format!("{MIN}[[users]]\nusername = \"v\"\npasswd = \"x\"\n")).unwrap_err();
        assert!(e.0.contains("unknown key users[1].passwd"), "{}", e.0);
        let e = cfg(&format!("colour = 1\n{MIN}")).unwrap_err();
        assert!(e.0.contains("unknown key colour"), "{}", e.0);
    }

    #[test]
    fn non_integer_value_names_the_key() {
        let e = cfg(&format!("[broker]\nmax_memory_mb = \"big\"\n{MIN}")).unwrap_err();
        assert!(e.0.contains("broker.max_memory_mb"), "{}", e.0);
        let e = cfg(&format!("[broker]\nport = 1\n{MIN}[expiry]\ncheck_interval_ms = 1.5\n")).unwrap_err();
        assert!(e.0.contains("expiry.check_interval_ms"), "{}", e.0);
    }

    /// Asserts that `text` is refused with an error mentioning `needle`.
    fn refused(text: &str, needle: &str) {
        let e = cfg(text).unwrap_err();
        assert!(e.0.starts_with("configuration error"), "{}", e.0);
        assert!(e.0.contains(needle), "expected '{needle}' in: {}", e.0);
    }

    #[test]
    fn every_validation_error_names_the_field() {
        refused(&format!("[broker]\nbind = \"1.2.3\"\n{MIN}"), "broker.bind");
        refused(
            "[admin]\nbind = \"localhost\"\npassword = \"a\"\n[[users]]\nusername = \"u\"\npassword = \"p\"\n",
            "admin.bind",
        );
        refused(&format!("[broker]\nport = 0\n{MIN}"), "broker.port");
        refused(
            "[admin]\nport = 70000\npassword = \"a\"\n[[users]]\nusername = \"u\"\npassword = \"p\"\n",
            "admin.port",
        );
        refused(&format!("[log]\nlevel = \"verbose\"\n{MIN}"), "log.level");
        refused("[admin]\npassword = \"a\"\n", "[[users]]");
        refused(
            "[admin]\npassword = \"a\"\n[[users]]\nusername = \"\"\npassword = \"p\"\n",
            "users[0].username",
        );
        refused(
            "[admin]\npassword = \"a\"\n[[users]]\nusername = \"u\"\n",
            "needs password or password_hash",
        );
        refused(
            "[admin]\npassword = \"a\"\n[[users]]\nusername = \"u\"\npassword_hash = \"plain\"\n",
            "password_hash is not an Argon2 hash",
        );
        refused(
            "[admin]\npassword_hash = \"md5\"\n[[users]]\nusername = \"u\"\npassword = \"p\"\n",
            "admin.password_hash",
        );
        refused("[[users]]\nusername = \"u\"\npassword = \"p\"\n", "admin.password");
        refused(
            &format!("[broker]\nmax_frame_size_mb = 0\n{MIN}"),
            "broker.max_frame_size_mb",
        );
        refused(
            &format!("[broker]\nauto_delete_empty_after_secs = -1\n{MIN}"),
            "broker.auto_delete_empty_after_secs",
        );
        refused(&format!("[broker]\nmax_memory_mb = -1\n{MIN}"), "broker.max_memory_mb");
        refused(
            &format!("[broker]\ntopic_max_pending_per_consumer = -5\n{MIN}"),
            "broker.topic_max_pending_per_consumer",
        );
        refused(
            &format!("[broker]\ncompress_threshold_kb = -1\n{MIN}"),
            "broker.compress_threshold_kb",
        );
        refused(
            &format!("[broker]\ncompress_min_saving_pct = 150\n{MIN}"),
            "broker.compress_min_saving_pct",
        );
    }

    #[test]
    fn anonymous_without_users_is_allowed() {
        let c = cfg("[broker]\nallow_anonymous = true\n[admin]\npassword = \"a\"\n").unwrap();
        assert!(c.users.is_empty());
        assert!(c.allow_anonymous);
    }

    #[test]
    fn memory_and_auto_delete_keys() {
        let c = cfg(MIN).unwrap();
        assert_eq!(c.max_memory_bytes, 0, "no memory limit by default");
        assert_eq!(c.auto_delete_empty_after_secs, 0);
        let c = cfg(&format!(
            "[broker]\nmax_memory_mb = 64\nauto_delete_empty_after_secs = 30\n{MIN}"
        ))
        .unwrap();
        assert_eq!(c.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(c.auto_delete_empty_after_secs, 30);
    }

    #[test]
    fn expiry_defaults_partial_section_and_invalid_values() {
        let c = cfg(MIN).unwrap();
        assert_eq!(c.expiry_check_interval_ms, 1000);
        assert!(!c.use_broker_clock);
        assert_eq!(c.ttl_ceiling_ms, 0);
        assert_eq!(c.default_ttl_ms, 0);
        let c = cfg(&format!("[expiry]\nttl_ceiling_ms = 60000\n{MIN}")).unwrap();
        assert_eq!(c.ttl_ceiling_ms, 60_000);
        assert_eq!(
            c.expiry_check_interval_ms, 1000,
            "missing keys of the section take their default"
        );
        assert_eq!(c.default_ttl_ms, 0);
        let c = cfg(&format!(
            "[expiry]\ncheck_interval_ms = 1\nuse_broker_clock = true\ndefault_ttl_ms = 5000\n{MIN}"
        ))
        .unwrap();
        assert_eq!(c.expiry_check_interval_ms, 1);
        assert!(c.use_broker_clock);
        assert_eq!(c.default_ttl_ms, 5000);
        refused(
            &format!("[expiry]\ncheck_interval_ms = 0\n{MIN}"),
            "expiry.check_interval_ms",
        );
        refused(
            &format!("[expiry]\nttl_ceiling_ms = -1\n{MIN}"),
            "expiry.ttl_ceiling_ms",
        );
        refused(
            &format!("[expiry]\ndefault_ttl_ms = -1\n{MIN}"),
            "expiry.default_ttl_ms",
        );
        refused(
            &format!("[expiry]\nuse_broker_clock = \"yes\"\n{MIN}"),
            "expiry.use_broker_clock",
        );
    }

    #[test]
    fn file_wins_over_defaults_and_command_line_over_file() {
        let text = "[broker]\nbind = \"127.0.0.1\"\nport = 61617\n[admin]\nport = 8200\npassword = \"a\"\n[[users]]\nusername = \"u\"\npassword = \"p\"\n";
        let c = cfg(text).unwrap();
        assert_eq!(
            (c.bind.to_string().as_str(), c.port, c.admin_port),
            ("127.0.0.1", 61617, 8200)
        );
        let o = Overrides {
            bind: Some("0.0.0.0".into()),
            admin_port: Some(8300),
            admin_bind: Some("127.0.0.2".into()),
            ..Default::default()
        };
        let c = from_toml(text, ConfigSource::File("t".into()), &o).unwrap();
        assert_eq!(
            (c.bind.to_string().as_str(), c.port, c.admin_port),
            ("0.0.0.0", 61617, 8300)
        );
        assert_eq!(c.admin_bind.to_string(), "127.0.0.2");
        // A bad command-line value is reported like a file value.
        let o = Overrides {
            bind: Some("nowhere".into()),
            ..Default::default()
        };
        assert!(from_toml(MIN, ConfigSource::File("t".into()), &o)
            .unwrap_err()
            .0
            .contains("broker.bind"));
    }

    #[test]
    fn duplicate_users_rejected() {
        let text = "[admin]\npassword = \"a\"\n[[users]]\nusername = \"u\"\npassword = \"p\"\n[[users]]\nusername = \"u\"\npassword = \"q\"\n";
        assert!(cfg(text).unwrap_err().0.contains("duplicate"));
    }

    #[test]
    fn both_password_forms_rejected() {
        let text = "[admin]\npassword = \"a\"\n[[users]]\nusername = \"u\"\npassword = \"p\"\npassword_hash = \"$argon2id$x\"\n";
        assert!(cfg(text).unwrap_err().0.contains("both"));
    }

    #[test]
    fn invalid_values_rejected() {
        assert!(cfg(&format!("[expiry]\ncheck_interval_ms = 0\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\ntopic_max_pending_per_consumer = -5\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\ncompress_min_saving_pct = 150\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\nmax_memory_mb = \"big\"\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\nport = 70000\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\nsocket_buffer_kb = -1\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\nsocket_buffer_kb = 65537\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\nprocessors = -1\n{MIN}")).is_err());
        assert!(cfg(&format!("[broker]\nprocessors = 1025\n{MIN}")).is_err());
        assert!(cfg("[admin]\npassword = \"a\"\n").is_err());
    }

    #[test]
    fn log_keys_take_defaults() {
        let c = cfg(MIN).unwrap();
        assert_eq!(c.log_level, "info");
        assert!(c.log_dir.ends_with("logs"));
        assert!(c.log_to_file);
        assert_eq!(c.log_retention_days, 30);
        assert!(c.log_compress);
    }

    #[test]
    fn log_keys_are_read() {
        let text = format!(
            "[log]\nlevel = \"debug\"\ndir = \"{}\"\nfile = false\nretention_days = 0\ncompress = false\n{MIN}",
            absolute()
        );
        let c = cfg(&text).unwrap();
        assert_eq!(c.log_level, "debug");
        assert_eq!(c.log_dir, PathBuf::from(absolute()));
        assert!(!c.log_to_file);
        assert_eq!(c.log_retention_days, 0);
        assert!(!c.log_compress);
    }

    /// An absolute folder that is left as it is, written for the platform running the test.
    fn absolute() -> &'static str {
        if cfg!(windows) {
            "C:\\\\logs\\\\mq"
        } else {
            "/var/log/mq"
        }
    }

    #[test]
    fn log_values_are_checked() {
        refused(&format!("[log]\nlevel = \"verbose\"\n{MIN}"), "log.level");
        refused(&format!("[log]\nretention_days = -1\n{MIN}"), "log.retention_days");
        refused(&format!("[log]\nretention_days = 3651\n{MIN}"), "log.retention_days");
        refused(&format!("[log]\ndir = \"\"\n{MIN}"), "log.dir");
        refused(&format!("[log]\nretention_days = \"30\"\n{MIN}"), "log.retention_days");
        refused(&format!("[log]\nfile = 1\n{MIN}"), "log.file");
    }

    #[test]
    fn log_level_override_wins_and_is_checked() {
        let o = Overrides {
            log_level: Some("DEBUG".into()),
            ..Default::default()
        };
        let text = format!("[log]\nlevel = \"error\"\n{MIN}");
        let c = from_toml(&text, ConfigSource::File("t".into()), &o).unwrap();
        assert_eq!(c.log_level, "debug");

        let bad = Overrides {
            log_level: Some("verbose".into()),
            ..Default::default()
        };
        let e = from_toml(MIN, ConfigSource::File("t".into()), &bad).unwrap_err();
        assert!(e.0.contains("--log-level"), "{}", e.0);
        assert!(e.0.contains("error, warn, info, debug or trace"), "{}", e.0);
    }

    #[test]
    fn tls_is_off_by_default() {
        let c = cfg(MIN).unwrap();
        assert!(!c.tls_enabled);
        assert!(c.plain);
        assert_eq!(c.tls_port, 61617);
        assert_eq!(c.tls_bind, c.bind, "tls.bind follows broker.bind");
        assert_eq!(c.tls_min_version, "1.2");
        assert_eq!(c.tls_client_auth, "none");
        assert!(c.tls_cert.is_none() && c.tls_keystore.is_none());
    }

    #[test]
    fn tls_files_are_resolved_next_to_the_configuration() {
        let text = format!("[tls]\nenabled = true\ncert = \"broker.crt\"\nkey = \"broker.key\"\n{MIN}");
        let c = from_toml(
            &text,
            ConfigSource::File(PathBuf::from("/etc/mq/mqrust.toml")),
            &Overrides::default(),
        )
        .unwrap();
        assert_eq!(c.tls_cert.unwrap(), PathBuf::from("/etc/mq/broker.crt"));
        assert_eq!(c.tls_key.unwrap(), PathBuf::from("/etc/mq/broker.key"));
    }

    #[test]
    fn tls_values_are_checked() {
        let on = "[tls]\nenabled = true\n";
        refused(&format!("{on}{MIN}"), "tls.cert");
        refused(&format!("{on}cert = \"a.crt\"\n{MIN}"), "tls.key");
        refused(&format!("{on}key = \"a.key\"\n{MIN}"), "tls.cert");
        refused(
            &format!("{on}cert = \"a.crt\"\nkey = \"a.key\"\nkeystore = \"a.p12\"\n{MIN}"),
            "not both",
        );
        let pem = format!("{on}cert = \"a.crt\"\nkey = \"a.key\"\n");
        refused(&format!("{pem}min_version = \"1.1\"\n{MIN}"), "tls.min_version");
        refused(&format!("{pem}client_auth = \"maybe\"\n{MIN}"), "tls.client_auth");
        refused(&format!("{pem}client_auth = \"required\"\n{MIN}"), "tls.client_ca");
        refused(&format!("{pem}port = 61616\n{MIN}"), "plain listener");
        refused(&format!("{pem}port = 0\n{MIN}"), "tls.port");
        // Turning the plain listener off without TLS would leave nothing listening.
        refused(&format!("[broker]\nplain = false\n{MIN}"), "broker.plain");
        // With TLS on it is allowed.
        let text = format!("[broker]\nplain = false\n{pem}{MIN}");
        let c = cfg(&text).unwrap();
        assert!(!c.plain && c.tls_enabled);
    }

    #[test]
    fn template_is_valid() {
        let c = cfg(TEMPLATE).unwrap();
        assert_eq!(c.port, 61616);
        let commented = TEMPLATE.replace("# session_", "session_").replace("# login_", "login_");
        let c = cfg(&commented).unwrap();
        assert_eq!((c.admin_session_idle_minutes, c.admin_session_max_hours), (30, 8));
        assert_eq!((c.admin_login_max_failures, c.admin_login_lockout_seconds), (5, 60));
        let tls_on = TEMPLATE
            .replace("# enabled = false", "enabled = true")
            .replace("# cert = ", "cert = ")
            .replace("# key = ", "key = ");
        let c = cfg(&tls_on).unwrap();
        assert!(c.tls_enabled && c.tls_port == 61617);
        assert!(c.tls_cert.unwrap().ends_with("broker.crt"));

        let commented = TEMPLATE
            .replace("# dir = ", "dir = ")
            .replace("# file = ", "file = ")
            .replace("# retention_days = ", "retention_days = ")
            .replace("# compress = ", "compress = ");
        let c = cfg(&commented).unwrap();
        assert!(c.log_dir.ends_with("logs"));
        assert!(c.log_to_file && c.log_compress);
        assert_eq!(c.log_retention_days, 30);
    }

    #[test]
    fn template_matches_the_example_file() {
        let example = include_str!("../mqrust.example.toml");
        assert_eq!(example.replace("\r\n", "\n"), TEMPLATE.replace("\r\n", "\n"));
    }
}
