// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Setup commands: the admin console user, the messaging users, password rules and safe
//! editing of the configuration file (comments and key order are kept, writes are atomic).

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Key, Table, TableLike, Value};

use crate::auth;
use crate::config::{self, Config, ConfigSource, Overrides};

/// Printed by every command that changes the file.
pub const RESTART_NOTICE: &str = "Restart the broker (or `mqrust.exe service stop` and `service start`) to apply.";

const ATTEMPTS: usize = 3;
const MIN_PASSWORD_CHARS: usize = 8;
const COMMON_PASSWORDS: [&str; 2] = ["admin", "password"];

/// A setup failure and its exit code: 2 for configuration or usage errors, 1 for runtime errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: u8,
    pub message: String,
}

impl Failure {
    pub fn usage(message: impl Into<String>) -> Self {
        Failure { code: 2, message: message.into() }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Failure { code: 1, message: message.into() }
    }
}

/// Checks a username: 1-64 letters, digits, `.`, `_`, `-` or `@`.
pub fn check_username(name: &str) -> Result<(), String> {
    let len = name.chars().count();
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@');
    if len == 0 || len > 64 || !name.chars().all(allowed) {
        return Err(format!(
            "invalid username \"{name}\": use 1-64 characters among letters, digits, '.', '_', '-' and '@'"
        ));
    }
    Ok(())
}

/// Checks a new password: at least 8 characters, different from the username, not a common one.
pub fn check_password(username: Option<&str>, password: &str) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(format!("password refused: it must have at least {MIN_PASSWORD_CHARS} characters"));
    }
    if username.is_some_and(|u| u == password) {
        return Err("password refused: it must differ from the username".into());
    }
    if COMMON_PASSWORDS.iter().any(|c| c.eq_ignore_ascii_case(password)) {
        return Err("password refused: \"admin\" and \"password\" are not allowed".into());
    }
    Ok(())
}

/// Asks for a new password twice, up to three attempts. `read` reads one hidden line after a prompt.
pub fn ask_new_password(
    username: Option<&str>,
    read: &mut dyn FnMut(&str) -> std::io::Result<String>,
    out: &mut dyn Write,
) -> Result<String, Failure> {
    let who = username.map(|u| format!(" for {u}")).unwrap_or_default();
    for _ in 0..ATTEMPTS {
        let first = read(&format!("New password{who}: "))
            .map_err(|e| Failure::runtime(format!("cannot read the password: {e}")))?;
        if let Err(reason) = check_password(username, &first) {
            let _ = writeln!(out, "{reason}");
            continue;
        }
        let second =
            read("Repeat the password: ").map_err(|e| Failure::runtime(format!("cannot read the password: {e}")))?;
        if first != second {
            let _ = writeln!(out, "Passwords do not match");
            continue;
        }
        return Ok(first);
    }
    Err(Failure::usage(format!("no valid password after {ATTEMPTS} attempts; nothing changed")))
}

/// Fails fast when a password must be typed but standard input is not a terminal.
pub fn require_terminal(password_stdin: bool) -> Result<(), Failure> {
    if password_stdin || std::io::stdin().is_terminal() {
        return Ok(());
    }
    Err(Failure::usage(
        "standard input is not a terminal: pass the password with --password-stdin, \
         e.g. Get-Content secret.txt | mqrust.exe ... --password-stdin",
    ))
}

/// Reads one password line from `input`, removing only the line ending.
pub fn read_password_line(input: &mut dyn BufRead) -> Result<String, Failure> {
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|e| Failure::runtime(format!("cannot read the password from standard input: {e}")))?;
    let trimmed = line.strip_suffix('\n').unwrap_or(&line);
    Ok(trimmed.strip_suffix('\r').unwrap_or(trimmed).to_string())
}

/// Gets a new password from standard input (`--password-stdin`) or from a hidden double prompt.
pub fn new_password(username: Option<&str>, password_stdin: bool) -> Result<String, Failure> {
    if password_stdin {
        let pw = read_password_line(&mut std::io::stdin().lock())?;
        check_password(username, &pw).map_err(Failure::usage)?;
        return Ok(pw);
    }
    require_terminal(false)?;
    ask_new_password(username, &mut |prompt| rpassword::prompt_password(prompt), &mut std::io::stderr())
}

/// Hashes a password with Argon2id.
pub fn hash(password: &str) -> Result<String, Failure> {
    auth::hash_password(password).map_err(|e| Failure::runtime(format!("cannot hash the password: {e}")))
}

/// File edited by the setup commands: `--config`, else `mqrust.toml` next to the executable.
pub fn target_path(explicit: Option<&Path>) -> Result<PathBuf, Failure> {
    match explicit {
        Some(p) => Ok(p.to_path_buf()),
        None => config::default_config_path().ok_or_else(|| Failure::runtime("cannot locate the executable folder")),
    }
}

/// Writes `text` to a temporary file in the same folder, then replaces `path` with it.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "mqrust.toml".into());
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// A configuration file being edited.
pub struct ConfigFile {
    pub path: PathBuf,
    /// True when the file did not exist and was started from the commented template.
    pub created: bool,
    doc: DocumentMut,
    /// The file used Windows line endings; they are kept when writing.
    crlf: bool,
}

impl ConfigFile {
    /// Opens the file, or starts from the template when it does not exist. Invalid TOML is refused.
    pub fn open(path: &Path) -> Result<Self, Failure> {
        if !path.exists() {
            return Self::parse(path, config::TEMPLATE, true);
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| Failure::runtime(format!("cannot read {}: {e}", path.display())))?;
        Self::parse(path, &text, false)
    }

    pub fn parse(path: &Path, text: &str, created: bool) -> Result<Self, Failure> {
        let crlf = text.contains("\r\n");
        let doc = text.replace("\r\n", "\n").parse::<DocumentMut>().map_err(|e| {
            Failure::usage(format!("cannot edit {}, it is not valid TOML; file unchanged\n{e}", path.display()))
        })?;
        Ok(ConfigFile { path: path.to_path_buf(), created, doc, crlf })
    }

    pub fn text(&self) -> String {
        let text = self.doc.to_string();
        if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        }
    }

    /// Current admin console username, if set in the file.
    pub fn admin_username(&self) -> Option<String> {
        self.doc.get("admin")?.get("username")?.as_str().map(str::to_string)
    }

    /// Usernames of the `[[users]]` entries, in file order.
    pub fn usernames(&self) -> Vec<String> {
        let Some(users) = self.doc.get("users").and_then(Item::as_array_of_tables) else {
            return Vec::new();
        };
        users.iter().filter_map(|t| t.get("username").and_then(Item::as_str).map(str::to_string)).collect()
    }

    fn allow_anonymous(&self) -> bool {
        self.doc.get("broker").and_then(|b| b.get("allow_anonymous")).and_then(Item::as_bool).unwrap_or(false)
    }

    /// True when the template's messaging user `admin` with password `admin` is still present.
    pub fn has_default_user(&self) -> bool {
        let Some(users) = self.doc.get("users").and_then(Item::as_array_of_tables) else {
            return false;
        };
        users.iter().any(|t| {
            t.get("username").and_then(Item::as_str) == Some(config::DEFAULT_USER)
                && t.get("password").and_then(Item::as_str) == Some(config::DEFAULT_PASSWORD)
        })
    }

    /// Sets `[admin] username` and `password_hash`, removing any plain `password`.
    pub fn set_admin(&mut self, username: &str, password_hash: &str) -> Result<(), Failure> {
        if self.doc.get("admin").is_none() {
            self.doc.insert("admin", Item::Table(Table::new()));
        }
        let admin = self.doc["admin"]
            .as_table_like_mut()
            .ok_or_else(|| Failure::usage("cannot edit [admin]: it is not a table"))?;
        set_string(admin, "username", username);
        set_secret(admin, password_hash);
        Ok(())
    }

    fn users_mut(&mut self) -> Result<&mut ArrayOfTables, Failure> {
        if self.doc.get("users").is_none() {
            self.doc.insert("users", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        self.doc["users"]
            .as_array_of_tables_mut()
            .ok_or_else(|| Failure::usage("cannot edit users: write them as [[users]] tables"))
    }

    fn user_index(&mut self, name: &str) -> Result<Option<usize>, Failure> {
        Ok(self.users_mut()?.iter().position(|t| t.get("username").and_then(Item::as_str) == Some(name)))
    }

    /// Adds a `[[users]]` entry; refuses a name that already exists.
    pub fn add_user(&mut self, name: &str, password_hash: &str) -> Result<(), Failure> {
        if self.user_index(name)?.is_some() {
            return Err(duplicate_user(name));
        }
        let mut entry = Table::new();
        entry.decor_mut().set_prefix("\n");
        entry.insert("username", toml_edit::value(name));
        entry.insert("password_hash", toml_edit::value(password_hash));
        self.users_mut()?.push(entry);
        Ok(())
    }

    /// Replaces the password of an existing user.
    pub fn set_user_password(&mut self, name: &str, password_hash: &str) -> Result<(), Failure> {
        let i = self.user_index(name)?.ok_or_else(|| unknown_user(name))?;
        let entry = self.users_mut()?.get_mut(i).expect("index from position");
        set_secret(entry, password_hash);
        Ok(())
    }

    /// Removes a user, refusing to remove the last one unless anonymous access is on.
    pub fn remove_user(&mut self, name: &str) -> Result<(), Failure> {
        let anonymous = self.allow_anonymous();
        let i = self.user_index(name)?.ok_or_else(|| unknown_user(name))?;
        let users = self.users_mut()?;
        if users.len() == 1 && !anonymous {
            return Err(Failure::usage(format!(
                "cannot remove {name}: it is the last messaging user and broker.allow_anonymous is false, \
                 so clients could no longer connect; add another user first (mqrust.exe user add <name>)"
            )));
        }
        users.remove(i);
        Ok(())
    }

    /// Validates the edited document as the broker would read it.
    pub fn validate(&self) -> Result<Config, Failure> {
        config::from_toml(&self.text(), ConfigSource::File(self.path.clone()), &Overrides::default())
            .map_err(|e| Failure::usage(format!("{}: {e}; file unchanged", self.path.display())))
    }

    /// Validates, then writes the file atomically. Returns the configuration it now holds.
    pub fn save(&self) -> Result<Config, Failure> {
        let cfg = self.validate()?;
        write_atomic(&self.path, &self.text())
            .map_err(|e| Failure::runtime(format!("cannot write {}: {e}; file unchanged", self.path.display())))?;
        Ok(cfg)
    }
}

fn duplicate_user(name: &str) -> Failure {
    Failure::usage(format!("user {name} already exists; to change its password run: mqrust.exe user passwd {name}"))
}

fn unknown_user(name: &str) -> Failure {
    Failure::usage(format!("user {name} does not exist; see mqrust.exe user list"))
}

/// Sets a string value, keeping the comments around an existing one.
fn set_string(table: &mut dyn TableLike, key: &str, text: &str) {
    match table.get_mut(key).and_then(Item::as_value_mut) {
        Some(old) => {
            let decor = old.decor().clone();
            let mut new = Value::from(text);
            *new.decor_mut() = decor;
            *old = new;
        }
        None => {
            table.insert(key, toml_edit::value(text));
        }
    }
}

/// Stores `password_hash`, removing `password`. Comments above `password` move to `password_hash`;
/// the comment after it, which describes the plain value, goes with it.
fn set_secret(table: &mut dyn TableLike, password_hash: &str) {
    let leading = table.key("password").map(|k| k.leaf_decor().clone());
    table.remove("password");
    if table.contains_key("password_hash") {
        set_string(table, "password_hash", password_hash);
        return;
    }
    let mut key = Key::new("password_hash");
    if let Some(decor) = leading {
        *key.leaf_decor_mut() = decor;
    }
    if let toml_edit::Entry::Vacant(v) = table.entry_format(&key) {
        v.insert(toml_edit::value(password_hash));
    }
}

/// Console URL for messages: an unspecified bind address is shown as 127.0.0.1.
pub fn console_url(cfg: &Config) -> String {
    let ip = if cfg.admin_bind.is_unspecified() { "127.0.0.1".parse().unwrap() } else { cfg.admin_bind };
    format!("http://{}", std::net::SocketAddr::new(ip, cfg.admin_port))
}

fn print_created(file: &ConfigFile) {
    if file.created {
        println!("Created {} from the commented template.", file.path.display());
    }
}

/// `set-admin`: sets the admin console user.
pub fn set_admin(path: &Path, username: Option<String>, password_stdin: bool) -> Result<(), Failure> {
    require_terminal(password_stdin)?;
    let mut file = ConfigFile::open(path)?;
    let current = file.admin_username().unwrap_or_else(|| config::DEFAULT_USER.to_string());
    let username = match username {
        Some(u) => u,
        None if password_stdin => current,
        None => prompt_line(&format!("Admin console username [{current}]: "))?
            .filter(|s| !s.is_empty())
            .unwrap_or(current),
    };
    check_username(&username).map_err(Failure::usage)?;
    let password = new_password(Some(&username), password_stdin)?;
    file.set_admin(&username, &hash(&password)?)?;
    let cfg = file.save()?;
    print_created(&file);
    println!("Admin console user \"{username}\" written to {}", file.path.display());
    println!("Console: {} (log in as {username})", console_url(&cfg));
    if file.has_default_user() {
        println!("Next: create a messaging user with `mqrust.exe user add <name>`, then `mqrust.exe user remove admin`.");
    }
    println!("{RESTART_NOTICE}");
    Ok(())
}

/// `user add`.
pub fn user_add(path: &Path, name: &str, password_stdin: bool) -> Result<(), Failure> {
    check_username(name).map_err(Failure::usage)?;
    require_terminal(password_stdin)?;
    let mut file = ConfigFile::open(path)?;
    if file.usernames().iter().any(|u| u == name) {
        return Err(duplicate_user(name));
    }
    let password = new_password(Some(name), password_stdin)?;
    file.add_user(name, &hash(&password)?)?;
    file.save()?;
    print_created(&file);
    println!("User \"{name}\" added to {}", file.path.display());
    if file.has_default_user() && name != config::DEFAULT_USER {
        println!("The template user admin/admin still exists: remove it with `mqrust.exe user remove admin`.");
    }
    println!("{RESTART_NOTICE}");
    Ok(())
}

/// `user passwd`.
pub fn user_passwd(path: &Path, name: &str, password_stdin: bool) -> Result<(), Failure> {
    require_terminal(password_stdin)?;
    let mut file = ConfigFile::open(path)?;
    if !file.usernames().iter().any(|u| u == name) {
        return Err(unknown_user(name));
    }
    let password = new_password(Some(name), password_stdin)?;
    file.set_user_password(name, &hash(&password)?)?;
    file.save()?;
    print_created(&file);
    println!("Password of \"{name}\" changed in {}", file.path.display());
    println!("{RESTART_NOTICE}");
    Ok(())
}

/// `user remove`.
pub fn user_remove(path: &Path, name: &str) -> Result<(), Failure> {
    if !path.exists() {
        return Err(Failure::usage(format!(
            "{} does not exist; create it with mqrust.exe init-config",
            path.display()
        )));
    }
    let mut file = ConfigFile::open(path)?;
    file.remove_user(name)?;
    file.save()?;
    println!("User \"{name}\" removed from {}", file.path.display());
    println!("{RESTART_NOTICE}");
    Ok(())
}

/// `user list`: usernames only, one per line.
pub fn user_list(path: &Path) -> Result<(), Failure> {
    if !path.exists() {
        eprintln!("{} does not exist: the built-in default user is in use", path.display());
        println!("{}", config::DEFAULT_USER);
        return Ok(());
    }
    let file = ConfigFile::open(path)?;
    let names = file.usernames();
    if names.is_empty() {
        eprintln!("no messaging users in {}", path.display());
    }
    for n in names {
        println!("{n}");
    }
    Ok(())
}

/// `hash-password`.
pub fn hash_password(password_stdin: bool) -> Result<(), Failure> {
    require_terminal(password_stdin)?;
    let password = new_password(None, password_stdin)?;
    println!("{}", hash(&password)?);
    Ok(())
}

/// `init-config`: writes the commented template; never overwrites.
pub fn init_config(path: &Path) -> Result<(), Failure> {
    if path.exists() {
        println!("{} already exists; left unchanged", path.display());
        return Ok(());
    }
    write_atomic(path, config::TEMPLATE)
        .map_err(|e| Failure::runtime(format!("cannot write {}: {e}", path.display())))?;
    println!("Written {}", path.display());
    println!("Next: `mqrust.exe set-admin` for the admin console, `mqrust.exe user add <name>` for the clients.");
    Ok(())
}

/// Reads one visible line from the terminal; `None` at end of input.
fn prompt_line(prompt: &str) -> Result<Option<String>, Failure> {
    let mut out = std::io::stderr();
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
    let mut line = String::new();
    let n = std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Failure::runtime(format!("cannot read standard input: {e}")))?;
    Ok((n > 0).then(|| line.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA";

    const CUSTOM: &str = "# my broker\n\n[broker]\nport = 61620   # custom port\nname = \"prod\"\n\n\
        [admin]\nusername = \"admin\"\n# the old secret\npassword = \"admin\" # change me\n\n\
        [[users]]\nusername = \"app1\"\npassword = \"app1-secret\"\n";

    fn file(text: &str) -> ConfigFile {
        ConfigFile::parse(Path::new("t.toml"), text, false).unwrap()
    }

    fn scripted(lines: &[&str]) -> impl FnMut(&str) -> std::io::Result<String> {
        let mut it = lines.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter();
        move |_| Ok(it.next().unwrap_or_default())
    }

    #[test]
    fn set_admin_keeps_comments_and_other_keys() {
        let mut f = file(CUSTOM);
        f.set_admin("ops", HASH).unwrap();
        let text = f.text();
        let broker_part = "# my broker\n\n[broker]\nport = 61620   # custom port\nname = \"prod\"\n\n";
        assert!(text.starts_with(broker_part), "{text}");
        assert!(text.contains("username = \"ops\""), "{text}");
        assert!(text.contains("# the old secret\npassword_hash = "), "{text}");
        assert!(!text.contains("password = \"admin\""), "{text}");
        assert!(!text.contains("change me"), "{text}");
        assert!(text.contains("[[users]]\nusername = \"app1\"\npassword = \"app1-secret\"\n"), "{text}");
        let cfg = f.validate().unwrap();
        assert_eq!(cfg.admin_user.username, "ops");
        assert_eq!(cfg.port, 61620);
    }

    #[test]
    fn template_becomes_valid_after_edits() {
        let mut f = ConfigFile::parse(Path::new("t.toml"), config::TEMPLATE, true).unwrap();
        f.set_admin("ops", HASH).unwrap();
        f.add_user("app1", HASH).unwrap();
        assert!(f.has_default_user());
        f.remove_user("admin").unwrap();
        assert!(!f.has_default_user());
        let cfg = f.validate().unwrap();
        assert_eq!(cfg.users.len(), 1);
        assert_eq!(cfg.users[0].username, "app1");
        assert!(f.text().contains("# [broker]") || f.text().contains("[broker]"));
        assert!(f.text().contains("# compress_threshold_kb = 0 "));
    }

    #[test]
    fn users_add_passwd_remove() {
        let mut f = file(CUSTOM);
        assert_eq!(f.add_user("app1", HASH).unwrap_err().code, 2);
        f.add_user("app2", HASH).unwrap();
        assert_eq!(f.usernames(), vec!["app1", "app2"]);
        f.set_user_password("app1", HASH).unwrap();
        let text = f.text();
        assert!(!text.contains("app1-secret"), "{text}");
        assert!(f.set_user_password("nobody", HASH).is_err());
        f.remove_user("app2").unwrap();
        let last = f.remove_user("app1").unwrap_err();
        assert!(last.message.contains("could no longer connect"), "{}", last.message);
        assert_eq!(f.usernames(), vec!["app1"]);
    }

    #[test]
    fn last_user_removable_with_anonymous_access() {
        let mut f = file("[broker]\nallow_anonymous = true\n[admin]\npassword_hash = \"$argon2id$x\"\n[[users]]\nusername = \"a\"\npassword = \"b\"\n");
        f.remove_user("a").unwrap();
        assert!(f.usernames().is_empty());
        f.validate().unwrap();
    }

    #[test]
    fn invalid_toml_refused_with_line() {
        let e = ConfigFile::parse(Path::new("t.toml"), "[broker]\nport = = 1\n", false).err().unwrap();
        assert_eq!(e.code, 2);
        assert!(e.message.contains("line 2"), "{}", e.message);
    }

    #[test]
    fn result_failing_validation_is_refused() {
        let mut f = file(&format!("{CUSTOM}[log]\nlevel = \"loud\"\n"));
        f.set_admin("ops", HASH).unwrap();
        assert!(f.validate().unwrap_err().message.contains("log.level"));
    }

    #[test]
    fn save_writes_atomically_and_failed_write_keeps_original() {
        let dir = std::env::temp_dir().join(format!("mqrust-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mqrust.toml");
        std::fs::write(&path, CUSTOM).unwrap();

        // A folder in place of the temporary file makes the write fail before the original is touched.
        std::fs::create_dir(dir.join("mqrust.toml.tmp")).unwrap();
        let mut f = ConfigFile::open(&path).unwrap();
        f.set_admin("ops", HASH).unwrap();
        assert_eq!(f.save().unwrap_err().code, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), CUSTOM);

        std::fs::remove_dir(dir.join("mqrust.toml.tmp")).unwrap();
        f.save().unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("username = \"ops\""));
        assert!(!dir.join("mqrust.toml.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn password_rules() {
        assert!(check_password(Some("ops"), "S3cure-pass").is_ok());
        assert!(check_password(Some("ops"), "short").is_err());
        assert!(check_password(Some("admin"), "admin").is_err());
        assert!(check_password(None, "password").is_err());
        assert!(check_password(None, "PASSWORD").is_err());
        assert!(check_password(Some("longusername"), "longusername").is_err());
        assert!(check_password(None, "").is_err());
    }

    #[test]
    fn username_rules() {
        for ok in ["ops", "a", "app.1_x-y@corp", &"u".repeat(64)] {
            assert!(check_username(ok).is_ok(), "{ok}");
        }
        for bad in ["", "bad name", "a/b", "é", &"u".repeat(65)] {
            let e = check_username(bad).unwrap_err();
            assert!(e.contains("letters, digits"), "{e}");
        }
    }

    #[test]
    fn prompt_retries_then_gives_up() {
        let mut out = Vec::new();
        let pw = ask_new_password(Some("ops"), &mut scripted(&["admin", "S3cure-pass", "other-pass", "S3cure-pass", "S3cure-pass"]), &mut out).unwrap();
        assert_eq!(pw, "S3cure-pass");
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("at least 8"), "{text}");
        assert!(text.contains("Passwords do not match"), "{text}");

        let mut out = Vec::new();
        let e = ask_new_password(Some("ops"), &mut scripted(&["S3cure-pass", "x1", "S3cure-pass", "x2", "S3cure-pass", "x3"]), &mut out)
            .unwrap_err();
        assert_eq!(e.code, 2);
        assert_eq!(String::from_utf8(out).unwrap().matches("Passwords do not match").count(), 3);
    }

    #[test]
    fn stdin_line_keeps_inner_spaces() {
        let mut input: &[u8] = b"  pass word \r\nnext\n";
        assert_eq!(read_password_line(&mut input).unwrap(), "  pass word ");
    }
}
