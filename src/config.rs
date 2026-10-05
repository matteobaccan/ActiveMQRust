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
            compress_threshold_kb: 32,
            compress_min_saving_pct: 10,
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
        ExpirySection { check_interval_ms: 1000, use_broker_clock: false, ttl_ceiling_ms: 0, default_ttl_ms: 0 }
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
}

impl Default for AdminSection {
    fn default() -> Self {
        AdminSection {
            bind: "127.0.0.1".into(),
            port: 8161,
            username: DEFAULT_USER.into(),
            password: None,
            password_hash: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogSection {
    pub level: String,
}

impl Default for LogSection {
    fn default() -> Self {
        LogSection { level: "info".into() }
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
    pub users: Vec<User>,
    pub log_level: String,
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
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("mqrust.toml")))
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
    let file: FileConfig = toml::from_str(text).map_err(|e| ConfigError(describe_toml_error(&e)))?;
    build(file, source, overrides)
}

fn describe_toml_error(e: &toml::de::Error) -> String {
    let msg = e.message().to_string();
    // Unknown keys: make the full key path explicit, e.g. "unknown key broker.prot".
    if let Some(rest) = msg.strip_prefix("unknown field `") {
        let field = rest.split('`').next().unwrap_or(rest);
        let section = section_of_error(e);
        return match section {
            Some(s) => format!("configuration error: unknown key {s}.{field}"),
            None => format!("configuration error: unknown key {field}"),
        };
    }
    format!("configuration error: {}", msg.trim())
}

fn section_of_error(e: &toml::de::Error) -> Option<String> {
    // The span points into the source; the toml crate does not expose the table path,
    // so derive it from the message context when available.
    let text = e.to_string();
    for s in ["broker", "expiry", "admin", "log", "users"] {
        if text.contains(&format!("[{s}]")) {
            return Some(s.to_string());
        }
    }
    None
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
    value
        .parse()
        .map_err(|_| ConfigError(format!("configuration error: {field} = \"{value}\" is not a valid IP address")))
}

fn parse_port(field: &str, value: i64) -> Result<u16, ConfigError> {
    if (1..=65535).contains(&value) {
        Ok(value as u16)
    } else {
        Err(ConfigError(format!("configuration error: {field} = {value} is not a valid port (1-65535)")))
    }
}

fn non_negative(field: &str, value: i64) -> Result<u64, ConfigError> {
    if value < 0 {
        Err(ConfigError(format!("configuration error: {field} = {value} must not be negative")))
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
    let topic_pending = non_negative("broker.topic_max_pending_per_consumer", b.topic_max_pending_per_consumer)?;
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

    let level = file.log.level.to_ascii_lowercase();
    if !["error", "warn", "info", "debug", "trace"].contains(&level.as_str()) {
        return Err(ConfigError(format!(
            "configuration error: log.level = \"{}\" must be error, warn, info, debug or trace",
            file.log.level
        )));
    }

    let is_file = matches!(source, ConfigSource::File(_));
    let mut default_credentials = false;

    let admin_secret = secret("admin", &a.password, &a.password_hash)?;
    let admin_user = match admin_secret {
        Some(s) => User { username: a.username.clone(), secret: s },
        None if !is_file => {
            default_credentials = true;
            User { username: DEFAULT_USER.into(), secret: Secret::Plain(DEFAULT_PASSWORD.into()) }
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
            return Err(ConfigError(format!("configuration error: users[{i}].username must not be empty")));
        }
        if !seen.insert(u.username.clone()) {
            return Err(ConfigError(format!("configuration error: duplicate username \"{}\"", u.username)));
        }
        let s = secret(&field, &u.password, &u.password_hash)?
            .ok_or_else(|| ConfigError(format!("configuration error: {field} needs password or password_hash")))?;
        users.push(User { username: u.username.clone(), secret: s });
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
            users.push(User { username: DEFAULT_USER.into(), secret: Secret::Plain(DEFAULT_PASSWORD.into()) });
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
        users,
        log_level: level,
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
# compress_threshold_kb = 32           # broker compresses larger bodies; 0 = never
# compress_min_saving_pct = 10

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

[log]
# level = "info"                       # error | warn | info | debug | trace

[[users]]
username = "admin"
password = "admin"                     # replace with password_hash = "..." (mqrust.exe hash-password)
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
    }

    #[test]
    fn partial_file_takes_defaults() {
        let c = cfg(MIN).unwrap();
        assert_eq!(c.port, 61616);
        assert_eq!(c.compress_threshold_bytes, 32 * 1024);
        assert_eq!(c.socket_buffer_bytes, 1024 * 1024);
        assert_eq!(c.processors, 0);
        assert_eq!(c.socket_buffer_bytes, 1024 * 1024);
        assert!(!c.default_credentials);
    }

    #[test]
    fn overrides_win() {
        let o = Overrides { port: Some(61620), ..Default::default() };
        let text = format!("[broker]\nport = 61617\n{MIN}");
        let c = from_toml(&text, ConfigSource::File("t".into()), &o).unwrap();
        assert_eq!(c.port, 61620);
    }

    #[test]
    fn unknown_key_is_named() {
        let e = cfg(&format!("[broker]\nprot = 1\n{MIN}")).unwrap_err();
        assert!(e.0.contains("prot"), "{}", e.0);
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
    fn template_is_valid() {
        let c = cfg(TEMPLATE).unwrap();
        assert_eq!(c.port, 61616);
    }
}
