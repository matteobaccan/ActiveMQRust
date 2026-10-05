// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! ActiveMQRust: an in-memory message broker speaking Apache ActiveMQ's OpenWire protocol.

use mqrust::{auth, config, cpu, logging, server, service};

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(
    name = "ActiveMQRust",
    bin_name = "mqrust.exe",
    version,
    about = "ActiveMQRust: in-memory OpenWire message broker compatible with Apache ActiveMQ"
)]
struct Cli {
    /// Configuration file (default: mqrust.toml next to the executable, else built-in defaults)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// OpenWire listen address
    #[arg(long)]
    bind: Option<String>,
    /// OpenWire port
    #[arg(long)]
    port: Option<u16>,
    /// Admin console listen address
    #[arg(long)]
    admin_bind: Option<String>,
    /// Admin console port
    #[arg(long)]
    admin_port: Option<u16>,
    /// Processors to use (0 = those available to the process), like -XX:ActiveProcessorCount
    #[arg(long)]
    processors: Option<i64>,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Read a password (hidden input) and print its Argon2id hash
    HashPassword,
    /// Validate the configuration and exit
    CheckConfig,
    /// Write a commented mqrust.toml next to the executable (never overwrites)
    InitConfig,
    /// Manage the Windows service
    Service {
        #[command(subcommand)]
        action: ServiceCmd,
    },
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Install the broker as a Windows service (administrator)
    Install {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Stop and remove the Windows service (administrator)
    Uninstall {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Start the installed service
    Start {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Stop the installed service
    Stop {
        #[arg(long, default_value = service::DEFAULT_NAME)]
        name: String,
    },
    /// Print the service state
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    match &cli.command {
        Some(Cmd::HashPassword) => {
            let pw = match rpassword::prompt_password("Password: ") {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: cannot read the password: {e}");
                    return ExitCode::from(1);
                }
            };
            match auth::hash_password(&pw) {
                Ok(h) => {
                    println!("{h}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
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
        Some(Cmd::InitConfig) => {
            let Some(path) = config::default_config_path() else {
                eprintln!("error: cannot locate the executable folder");
                return ExitCode::from(1);
            };
            if path.exists() {
                println!("{} already exists; left unchanged", path.display());
                return ExitCode::SUCCESS;
            }
            report(
                std::fs::write(&path, config::TEMPLATE).map_err(|e| e.to_string()),
                &format!("written {}", path.display()),
            )
        }
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
