// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Windows service: install, uninstall, control and Service Control Manager entry point.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceInfo,
    ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

pub const DEFAULT_NAME: &str = "ActiveMQRust";
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_EXISTS: i32 = 1073;
const ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: i32 = 1063;

fn describe(e: &windows_service::Error, name: &str) -> String {
    if let windows_service::Error::Winapi(io) = e {
        match io.raw_os_error() {
            Some(ERROR_ACCESS_DENIED) => return "administrator rights are required (run the console as administrator)".into(),
            Some(ERROR_SERVICE_DOES_NOT_EXIST) => return format!("service {name} is not installed"),
            Some(ERROR_SERVICE_EXISTS) => return format!("service {name} already exists"),
            _ => {}
        }
    }
    format!("{e}")
}

fn manager(access: ServiceManagerAccess, name: &str) -> Result<ServiceManager, String> {
    ServiceManager::local_computer(None::<&str>, access).map_err(|e| describe(&e, name))
}

pub fn install(name: &str, config: Option<PathBuf>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut args: Vec<OsString> = vec!["service".into(), "run".into(), "--name".into(), name.into()];
    if let Some(c) = config {
        let abs = std::fs::canonicalize(&c).map_err(|e| format!("cannot resolve {}: {e}", c.display()))?;
        // canonicalize returns a \\?\ path; keep it readable for the Services console.
        let text = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        args.push("--config".into());
        args.push(text.into());
    }
    let display = if name == DEFAULT_NAME { DEFAULT_NAME.to_string() } else { format!("{DEFAULT_NAME} {name}") };
    let info = ServiceInfo {
        name: OsString::from(name),
        display_name: OsString::from(display),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: args,
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let m = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE, name)?;
    let s = m
        .create_service(&info, ServiceAccess::CHANGE_CONFIG)
        .map_err(|e| describe(&e, name))?;
    let _ = s.set_description("ActiveMQRust OpenWire message broker");
    Ok(())
}

pub fn uninstall(name: &str) -> Result<(), String> {
    let m = manager(ServiceManagerAccess::CONNECT, name)?;
    let s = m
        .open_service(name, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE)
        .map_err(|e| describe(&e, name))?;
    let status = s.query_status().map_err(|e| describe(&e, name))?;
    if status.current_state != ServiceState::Stopped {
        let _ = s.stop();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match s.query_status() {
                Ok(st) if st.current_state == ServiceState::Stopped => break,
                _ => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }
    s.delete().map_err(|e| describe(&e, name))
}

pub fn start(name: &str) -> Result<(), String> {
    let m = manager(ServiceManagerAccess::CONNECT, name)?;
    let s = m.open_service(name, ServiceAccess::START).map_err(|e| describe(&e, name))?;
    s.start(&[] as &[&OsStr]).map_err(|e| describe(&e, name))
}

pub fn stop(name: &str) -> Result<(), String> {
    let m = manager(ServiceManagerAccess::CONNECT, name)?;
    let s = m
        .open_service(name, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)
        .map_err(|e| describe(&e, name))?;
    s.stop().map(|_| ()).map_err(|e| describe(&e, name))
}

pub fn status(name: &str) -> Result<&'static str, String> {
    let m = manager(ServiceManagerAccess::CONNECT, name)?;
    let s = match m.open_service(name, ServiceAccess::QUERY_STATUS) {
        Ok(s) => s,
        Err(windows_service::Error::Winapi(io)) if io.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST) => {
            return Ok("not installed")
        }
        Err(e) => return Err(describe(&e, name)),
    };
    let st = s.query_status().map_err(|e| describe(&e, name))?;
    Ok(match st.current_state {
        ServiceState::Running => "running",
        ServiceState::Stopped => "stopped",
        ServiceState::StartPending => "start pending",
        ServiceState::StopPending => "stop pending",
        ServiceState::Paused => "paused",
        ServiceState::PausePending => "pause pending",
        ServiceState::ContinuePending => "continue pending",
    })
}

/// Arguments handed from `main` to the service entry point.
static RUN_ARGS: Mutex<Option<(String, Option<PathBuf>)>> = Mutex::new(None);

define_windows_service!(ffi_service_main, service_main);

/// `mqrust.exe service run`: connects to the Service Control Manager.
pub fn run(name: String, config: Option<PathBuf>) -> Result<(), String> {
    *RUN_ARGS.lock().unwrap() = Some((name.clone(), config));
    service_dispatcher::start(&name, ffi_service_main).map_err(|e| match &e {
        windows_service::Error::Winapi(io) if io.raw_os_error() == Some(ERROR_FAILED_SERVICE_CONTROLLER_CONNECT) => {
            "`service run` must be started by the Windows Service Control Manager (use `service start`)".to_string()
        }
        _ => format!("{e}"),
    })
}

fn service_main(_args: Vec<OsString>) {
    let (name, config) = RUN_ARGS.lock().unwrap().clone().unwrap_or((DEFAULT_NAME.into(), None));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let stop_tx = Mutex::new(Some(stop_tx));
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown | ServiceControl::Preshutdown => {
            if let Some(tx) = stop_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(status) = service_control_handler::register(&name, handler) else { return };
    let set = |state: ServiceState, code: u32, accept: ServiceControlAccept, wait: Duration| {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(code),
            checkpoint: 0,
            wait_hint: wait,
            process_id: None,
        });
    };
    set(ServiceState::StartPending, 0, ServiceControlAccept::empty(), Duration::from_secs(10));

    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mqrust.log")))
        .unwrap_or_else(|| PathBuf::from("mqrust.log"));

    let code = match crate::config::load(config.as_deref(), &Default::default()) {
        Err(e) => {
            let _ = crate::logging::init_file(&log_path, "info");
            tracing::error!("{e}");
            2
        }
        Ok(cfg) => {
            let _ = crate::logging::init_file(&log_path, &cfg.log_level);
            let rt = crate::cpu::runtime(cfg.processors).expect("runtime");
            let status_ref = &status;
            let result = rt.block_on(crate::server::run(
                cfg,
                Box::pin(async move {
                    let _ = stop_rx.await;
                }),
                move || {
                    let _ = status_ref.set_service_status(ServiceStatus {
                        service_type: ServiceType::OWN_PROCESS,
                        current_state: ServiceState::Running,
                        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                        exit_code: ServiceExitCode::Win32(0),
                        checkpoint: 0,
                        wait_hint: Duration::default(),
                        process_id: None,
                    });
                },
            ));
            match result {
                Ok(()) => 0,
                Err(e) => {
                    tracing::error!("{e}");
                    1
                }
            }
        }
    };
    set(ServiceState::Stopped, code, ServiceControlAccept::empty(), Duration::default());
}
