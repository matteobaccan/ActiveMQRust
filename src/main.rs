// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! ActiveMQRust: an in-memory message broker speaking Apache ActiveMQ's OpenWire protocol.

use mqrust::{config, cpu, logging, server, service, setup};

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Top of both helps: product line, usage, getting started and the grouped commands.
const HELP_TEMPLATE: &str = "\
ActiveMQRust {version}: in-memory OpenWire message broker compatible with Apache ActiveMQ

{usage-heading} {usage}

Getting started:
  1. mqrust.exe init-config       create mqrust.toml next to the executable
  2. mqrust.exe set-admin         choose the admin console user and password
  3. mqrust.exe user add <name>   create a user for the JMS/OpenWire clients
  4. mqrust.exe                   start the broker
                                  (or mqrust.exe service install to run it as a Windows service)

Setup:
  init-config    Write a commented mqrust.toml (never overwrites)
  set-admin      Set the admin console user and password
  user           Manage the messaging users: add, passwd, remove, list
  hash-password  Print the Argon2id hash of a password
  check-config   Validate the configuration and exit

Windows service:
  service        Install, uninstall, start, stop the Windows service or show its state

{all-args}{after-help}";

const AFTER_HELP: &str = "Run 'mqrust.exe --help' for details, examples and exit codes.";

const AFTER_LONG_HELP: &str = "\
Two kinds of users:
  admin console user  [admin], one user, for the web console at http://127.0.0.1:8161
                      set it with: mqrust.exe set-admin
  messaging users     [[users]], one or more, for JMS/OpenWire clients (tcp://host:61616)
                      manage them with: mqrust.exe user add | passwd | remove | list
  Without a configuration file both use admin/admin. Anyone who can reach the ports can then
  log in: change them before exposing the broker on a network.

Configuration file, searched in this order:
  1. --config <FILE>
  2. mqrust.toml next to mqrust.exe
  3. built-in defaults (no file)
  Changes to the file apply when the broker (or the Windows service) is restarted.

Examples:
  mqrust.exe init-config
  mqrust.exe set-admin --username ops
  mqrust.exe user add app1
  Get-Content secret.txt | mqrust.exe user add app2 --password-stdin
  mqrust.exe --config D:\\mq\\mqrust.toml --port 61617
  mqrust.exe service install --config D:\\mq\\mqrust.toml

Exit codes:
  0  success
  1  runtime error (for example the port is already in use)
  2  configuration or usage error";

#[derive(Parser)]
#[command(
    name = "ActiveMQRust",
    bin_name = "mqrust.exe",
    version,
    about = "ActiveMQRust: in-memory OpenWire message broker compatible with Apache ActiveMQ",
    help_template = HELP_TEMPLATE,
    override_usage = "mqrust.exe [OPTIONS] [COMMAND]",
    after_help = AFTER_HELP,
    after_long_help = AFTER_LONG_HELP,
    disable_help_subcommand = true
)]
struct Cli {
    /// Configuration file [default: mqrust.toml next to mqrust.exe]
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help_heading = "Configuration",
        long_help = "Configuration file [default: mqrust.toml next to mqrust.exe, else built-in defaults]"
    )]
    config: Option<PathBuf>,
    /// OpenWire listen address [default: 0.0.0.0]
    #[arg(long, value_name = "IP", help_heading = "Network")]
    bind: Option<String>,
    /// OpenWire port [default: 61616]
    #[arg(long, value_name = "PORT", help_heading = "Network")]
    port: Option<u16>,
    /// Admin console listen address [default: 127.0.0.1]
    #[arg(long, value_name = "IP", help_heading = "Network")]
    admin_bind: Option<String>,
    /// Admin console port [default: 8161]
    #[arg(long, value_name = "PORT", help_heading = "Network")]
    admin_port: Option<u16>,
    /// Processors to use [default: 0 = all available to the process]
    #[arg(
        long,
        value_name = "N",
        help_heading = "Performance",
        long_help = "Processors to use [default: 0 = all available to the process], like -XX:ActiveProcessorCount"
    )]
    processors: Option<i64>,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write a commented mqrust.toml next to the executable, or to --config (never overwrites)
    #[command(hide = true, after_help = "Examples:\n  mqrust.exe init-config\n  mqrust.exe init-config --config D:\\mq\\mqrust.toml")]
    InitConfig,
    /// Set the admin console user and password (hidden input, asked twice)
    #[command(
        hide = true,
        long_about = SET_ADMIN_ABOUT,
        after_help = "Examples:\n  mqrust.exe set-admin\n  mqrust.exe set-admin --username ops\n  \
                      Get-Content secret.txt | mqrust.exe set-admin --username ops --password-stdin"
    )]
    SetAdmin {
        /// Console username (asked when omitted; default: the current one, else admin)
        #[arg(long)]
        username: Option<String>,
        /// Read the password from the first line of standard input instead of asking
        #[arg(long)]
        password_stdin: bool,
    },
    /// Manage the messaging users of the JMS/OpenWire clients
    #[command(
        hide = true,
        arg_required_else_help = true,
        subcommand_required = true,
        after_help = "Examples:\n  mqrust.exe user add app1\n  mqrust.exe user passwd app1\n  \
                      mqrust.exe user remove admin\n  mqrust.exe user list"
    )]
    User {
        #[command(subcommand)]
        action: UserCmd,
    },
    /// Read a password and print its Argon2id hash
    #[command(
        hide = true,
        after_help = "Examples:\n  mqrust.exe hash-password\n  Get-Content secret.txt | mqrust.exe hash-password --password-stdin"
    )]
    HashPassword {
        /// Read the password from the first line of standard input instead of asking
        #[arg(long)]
        password_stdin: bool,
    },
    /// Validate the configuration and exit
    #[command(
        hide = true,
        after_help = "Examples:\n  mqrust.exe check-config\n  mqrust.exe check-config --config mqrust.example.toml"
    )]
    CheckConfig,
    /// Manage the Windows service
    #[command(
        hide = true,
        arg_required_else_help = true,
        subcommand_required = true,
        after_help = "Examples:\n  mqrust.exe service install --config D:\\mq\\mqrust.toml\n  \
                      mqrust.exe service start\n  mqrust.exe service status"
    )]
    Service {
        #[command(subcommand)]
        action: ServiceCmd,
    },
}

const SET_ADMIN_ABOUT: &str = "Set the admin console user and password (hidden input, asked twice).

Writes [admin] username and password_hash (Argon2id) and removes any plain password.
Creates the configuration file from the commented template when it does not exist.
Passwords: at least 8 characters, different from the username, not \"admin\" or \"password\".
Usernames: 1-64 letters, digits, '.', '_', '-' or '@'.";

const USER_ADD_ABOUT: &str = "Add a messaging user (password asked twice, hidden).

Creates the configuration file from the commented template when it does not exist.
Passwords: at least 8 characters, different from the username, not \"admin\" or \"password\".
Usernames: 1-64 letters, digits, '.', '_', '-' or '@'.";

const USER_PASSWD_ABOUT: &str = "Change the password of a messaging user (asked twice, hidden).

Passwords: at least 8 characters, different from the username, not \"admin\" or \"password\".";

#[derive(Subcommand)]
enum UserCmd {
    /// Add a messaging user (password asked twice, hidden)
    #[command(long_about = USER_ADD_ABOUT, after_help = "Examples:\n  mqrust.exe user add app1\n  \
        Get-Content secret.txt | mqrust.exe user add app1 --password-stdin")]
    Add {
        /// Username
        name: String,
        /// Read the password from the first line of standard input instead of asking
        #[arg(long)]
        password_stdin: bool,
    },
    /// Change the password of a messaging user
    #[command(long_about = USER_PASSWD_ABOUT, after_help = "Example:\n  mqrust.exe user passwd app1")]
    Passwd {
        /// Username
        name: String,
        /// Read the password from the first line of standard input instead of asking
        #[arg(long)]
        password_stdin: bool,
    },
    /// Remove a messaging user (the last one only when broker.allow_anonymous = true)
    #[command(after_help = "Example:\n  mqrust.exe user remove admin")]
    Remove {
        /// Username
        name: String,
    },
    /// List the messaging usernames (no passwords)
    #[command(after_help = "Example:\n  mqrust.exe user list")]
    List,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Install the broker as a Windows service (administrator)
    #[command(after_help = "Example:\n  mqrust.exe service install --config D:\\mq\\mqrust.toml")]
    Install {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Stop and remove the Windows service (administrator)
    #[command(after_help = "Example:\n  mqrust.exe service uninstall")]
    Uninstall {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Start the installed service
    #[command(after_help = "Example:\n  mqrust.exe service start")]
    Start {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Stop the installed service
    #[command(after_help = "Example:\n  mqrust.exe service stop")]
    Stop {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Print the service state
    #[command(after_help = "Example:\n  mqrust.exe service status")]
    Status {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Entry point used by the Service Control Manager
    #[command(hide = true)]
    Run {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
}

fn overrides(cli: &Cli) -> config::Overrides {
    config::Overrides {
        bind: cli.bind.clone(),
        port: cli.port,
        admin_bind: cli.admin_bind.clone(),
        admin_port: cli.admin_port,
        processors: cli.processors,
    }
}

fn report(result: Result<(), String>, ok: &str) -> ExitCode {
    match result {
        Ok(()) => {
            println!("{ok}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

/// Runs a setup command against the target configuration file.
fn setup_command(cli: &Cli, run: impl FnOnce(&std::path::Path) -> Result<(), setup::Failure>) -> ExitCode {
    let result = setup::target_path(cli.config.as_deref()).and_then(|path| run(&path));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            eprintln!("error: {}", f.message);
            ExitCode::from(f.code)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match &cli.command {
        Some(Cmd::HashPassword { password_stdin }) => setup_command(&cli, |_| setup::hash_password(*password_stdin)),
        Some(Cmd::SetAdmin { username, password_stdin }) => {
            setup_command(&cli, |p| setup::set_admin(p, username.clone(), *password_stdin))
        }
        Some(Cmd::User { action }) => setup_command(&cli, |p| match action {
            UserCmd::Add { name, password_stdin } => setup::user_add(p, name, *password_stdin),
            UserCmd::Passwd { name, password_stdin } => setup::user_passwd(p, name, *password_stdin),
            UserCmd::Remove { name } => setup::user_remove(p, name),
            UserCmd::List => setup::user_list(p),
        }),
        Some(Cmd::InitConfig) => setup_command(&cli, setup::init_config),
        Some(Cmd::CheckConfig) => match config::load(cli.config.as_deref(), &overrides(&cli)) {
            Ok(c) => {
                match c.source {
                    config::ConfigSource::File(p) => println!("configuration OK: {}", p.display()),
                    config::ConfigSource::Defaults => println!("configuration OK: no file, built-in defaults"),
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(2)
            }
        },
        Some(Cmd::Service { action }) => match action {
            ServiceCmd::Install { name } => report(
                service::install(name, cli.config.clone()),
                &format!("service {name} installed (automatic start)"),
            ),
            ServiceCmd::Uninstall { name } => report(service::uninstall(name), &format!("service {name} removed")),
            ServiceCmd::Start { name } => report(service::start(name), &format!("service {name} started")),
            ServiceCmd::Stop { name } => report(service::stop(name), &format!("service {name} stopping")),
            ServiceCmd::Status { name } => match service::status(name) {
                Ok(s) => {
                    println!("{s}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            },
            ServiceCmd::Run { name } => match service::run(name.clone(), cli.config.clone()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            },
        },
        None => {
            let cfg = match config::load(cli.config.as_deref(), &overrides(&cli)) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(2);
                }
            };
            logging::init_stdout(&cfg.log_level);
            let rt = match cpu::runtime(cfg.processors) {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("error: cannot start the runtime: {e}");
                    return ExitCode::from(1);
                }
            };
            match rt.block_on(server::run(cfg, server::console_stop_signal(), || {})) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::from(1)
                }
            }
        }
    }
}
