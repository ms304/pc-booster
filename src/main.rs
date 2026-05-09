use clap::{Parser, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, EnumServicesStatusExW,
    OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, StartServiceW,
    SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SC_MANAGER_ALL_ACCESS,
    SERVICE_ACCEPT_STOP, SERVICE_ALL_ACCESS, SERVICE_CONTROL_STOP,
    SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_PROCESS, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, SC_HANDLE, SC_ENUM_PROCESS_INFO, SERVICE_QUERY_STATUS,
    ENUM_SERVICE_TYPE, ENUM_SERVICE_STATE, SC_STATUS_TYPE,
};
use windows::core::{PCWSTR, PWSTR};

#[derive(Parser)]
#[command(name = "pc-booster")]
#[command(about = "Windows service manager and optimizer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all services with their status
    List {
        /// Show only running services
        #[arg(short, long)]
        running: bool,
        /// Show only stopped services
        #[arg(short, long)]
        stopped: bool,
    },
    /// Stop a service
    Stop {
        /// Service name
        name: String,
    },
    /// Start a service
    Start {
        /// Service name
        name: String,
    },
    /// Monitor and keep services stopped
    Monitor {
        /// Services to monitor (comma-separated)
        #[arg(short, long)]
        services: String,
        /// Check interval in seconds
        #[arg(short, long, default_value = "5")]
        interval: u64,
    },
    /// Add service to blacklist (auto-stop list)
    Blacklist {
        /// Service name
        name: String,
    },
    /// Remove service from blacklist
    Unblacklist {
        /// Service name
        name: String,
    },
    /// List blacklisted services
    ListBlacklist,
    /// Apply blacklist (stop all blacklisted services)
    Apply,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceInfo {
    name: String,
    display_name: String,
    status: String,
    process_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    blacklisted_services: HashSet<String>,
}

impl Config {
    fn load() -> Self {
        let path = get_config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<Config>(&content) {
                    return config;
                }
            }
        }
        Config {
            blacklisted_services: HashSet::new(),
        }
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = get_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }
}

fn get_config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("pc-booster");
    path.push("config.json");
    path
}

fn to_pcwstr(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_pcwstr(ptr: PWSTR) -> String {
    unsafe {
        let mut len = 0;
        while *ptr.0.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr.0, len))
    }
}

struct ServiceManager {
    sc_manager: SC_HANDLE,
}

impl ServiceManager {
    fn open() -> Result<Self, String> {
        unsafe {
            let sc_manager = OpenSCManagerW(
                PCWSTR::null(),
                PCWSTR::null(),
                SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE | SC_MANAGER_ALL_ACCESS,
            )
            .map_err(|e| format!("Failed to open service manager: {:?}", e))?;

            Ok(Self { sc_manager })
        }
    }

    fn list_services(&self) -> Result<Vec<ServiceInfo>, String> {
        unsafe {
            let mut services = Vec::new();
            let mut bytes_needed = 0u32;
            let mut services_returned = 0u32;
            let mut resume_handle: Option<*mut u32> = None;

            let mut buffer = Vec::new();

            loop {
                let result = EnumServicesStatusExW(
                    self.sc_manager,
                    SC_ENUM_PROCESS_INFO,
                    ENUM_SERVICE_TYPE(0x00000030), // SERVICE_WIN32
                    ENUM_SERVICE_STATE(0x00000003), // SERVICE_STATE_ALL
                    Some(&mut buffer),
                    &mut bytes_needed,
                    &mut services_returned,
                    resume_handle,
                    None,
                );

                if result.is_ok() {
                    break;
                }

                let err = GetLastError();
                if err.0 == 234 { // ERROR_MORE_DATA
                    buffer.resize(bytes_needed as usize, 0);
                    continue;
                }

                return Err(format!("Failed to enumerate services: {:?}", err));
            }

            let service_array = buffer.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW;
            for i in 0..services_returned {
                let service = &*service_array.add(i as usize);
                let name = from_pcwstr(service.lpServiceName);
                let display_name = from_pcwstr(service.lpDisplayName);

                let status = match service.ServiceStatusProcess.dwCurrentState {
                    SERVICE_RUNNING => "Running".to_string(),
                    SERVICE_STOPPED => "Stopped".to_string(),
                    SERVICE_START_PENDING => "Starting".to_string(),
                    SERVICE_STOP_PENDING => "Stopping".to_string(),
                    _ => "Unknown".to_string(),
                };

                let process_id = if service.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING {
                    Some(service.ServiceStatusProcess.dwProcessId)
                } else {
                    None
                };

                services.push(ServiceInfo {
                    name,
                    display_name,
                    status,
                    process_id,
                });
            }

            Ok(services)
        }
    }

    fn stop_service(&self, name: &str) -> Result<(), String> {
        unsafe {
            let service_name = to_pcwstr(name);
            let service = OpenServiceW(
                self.sc_manager,
                PCWSTR(service_name.as_ptr()),
                SERVICE_ALL_ACCESS,
            )
            .map_err(|e| format!("Failed to open service '{}': {:?}", name, e))?;

            let mut status = SERVICE_STATUS_PROCESS::default();
            let mut bytes_needed = 0u32;
            let result = QueryServiceStatusEx(
                service,
                SC_STATUS_TYPE(0),
                Some(std::slice::from_raw_parts_mut(
                    &mut status as *mut _ as *mut u8,
                    std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
                )),
                &mut bytes_needed,
            );

            if result.is_err() {
                CloseServiceHandle(service);
                return Err(format!("Failed to query service status: {:?}", GetLastError()));
            }

            if status.dwCurrentState == SERVICE_STOPPED {
                CloseServiceHandle(service);
                return Ok(());
            }

            if status.dwControlsAccepted & SERVICE_ACCEPT_STOP == 0 {
                CloseServiceHandle(service);
                return Err(format!("Service '{}' does not accept stop commands", name));
            }

            let mut service_status = SERVICE_STATUS::default();
            let result = ControlService(service, SERVICE_CONTROL_STOP, &mut service_status);
            if result.is_err() {
                CloseServiceHandle(service);
                return Err(format!("Failed to stop service '{}': {:?}", name, GetLastError()));
            }

            CloseServiceHandle(service);
            Ok(())
        }
    }

    fn start_service(&self, name: &str) -> Result<(), String> {
        unsafe {
            let service_name = to_pcwstr(name);
            let service = OpenServiceW(
                self.sc_manager,
                PCWSTR(service_name.as_ptr()),
                SERVICE_ALL_ACCESS,
            )
            .map_err(|e| format!("Failed to open service '{}': {:?}", name, e))?;

            let result = StartServiceW(service, None);
            CloseServiceHandle(service);

            if result.is_err() {
                Err(format!("Failed to start service '{}': {:?}", name, GetLastError()))
            } else {
                Ok(())
            }
        }
    }

    fn is_service_running(&self, name: &str) -> Result<bool, String> {
        unsafe {
            let service_name = to_pcwstr(name);
            let service = OpenServiceW(
                self.sc_manager,
                PCWSTR(service_name.as_ptr()),
                SERVICE_QUERY_STATUS,
            )
            .map_err(|e| format!("Failed to open service '{}': {:?}", name, e))?;

            let mut status = SERVICE_STATUS_PROCESS::default();
            let mut bytes_needed = 0u32;
            let result = QueryServiceStatusEx(
                service,
                SC_STATUS_TYPE(0),
                Some(std::slice::from_raw_parts_mut(
                    &mut status as *mut _ as *mut u8,
                    std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
                )),
                &mut bytes_needed,
            );

            CloseServiceHandle(service);

            if result.is_err() {
                Err(format!("Failed to query service status: {:?}", GetLastError()))
            } else {
                Ok(status.dwCurrentState == SERVICE_RUNNING)
            }
        }
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.sc_manager);
        }
    }
}

// Windows API structures
#[repr(C)]
struct ENUM_SERVICE_STATUS_PROCESSW {
    lpServiceName: PWSTR,
    lpDisplayName: PWSTR,
    ServiceStatusProcess: SERVICE_STATUS_PROCESS,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::List { running, stopped } => {
            list_services(running, stopped);
        }
        Commands::Stop { name } => {
            stop_service(&name);
        }
        Commands::Start { name } => {
            start_service(&name);
        }
        Commands::Monitor { services, interval } => {
            monitor_services(&services, interval);
        }
        Commands::Blacklist { name } => {
            blacklist_service(&name);
        }
        Commands::Unblacklist { name } => {
            unblacklist_service(&name);
        }
        Commands::ListBlacklist => {
            list_blacklist();
        }
        Commands::Apply => {
            apply_blacklist();
        }
    }
}

fn list_services(running_only: bool, stopped_only: bool) {
    let manager = match ServiceManager::open() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            return;
        }
    };

    let services = match manager.list_services() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            return;
        }
    };

    println!("\n{:<40} {:<30} {:<10} {}", "Service Name", "Display Name", "Status", "PID");
    println!("{}", "-".repeat(90));

    for service in services {
        let should_show = if running_only {
            service.status == "Running"
        } else if stopped_only {
            service.status == "Stopped"
        } else {
            true
        };

        if should_show {
            let status_colored = match service.status.as_str() {
                "Running" => service.status.green(),
                "Stopped" => service.status.red(),
                "Starting" => service.status.yellow(),
                "Stopping" => service.status.yellow(),
                _ => service.status.white(),
            };

            let pid = service.process_id.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string());
            println!("{:<40} {:<30} {:<10} {}", service.name, service.display_name, status_colored, pid);
        }
    }
}

fn stop_service(name: &str) {
    let manager = match ServiceManager::open() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            return;
        }
    };

    match manager.stop_service(name) {
        Ok(_) => {
            println!("{} {}", "Success:".green(), format!("Service '{}' stopped", name));
        }
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
        }
    }
}

fn start_service(name: &str) {
    let manager = match ServiceManager::open() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            return;
        }
    };

    match manager.start_service(name) {
        Ok(_) => {
            println!("{} {}", "Success:".green(), format!("Service '{}' started", name));
        }
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
        }
    }
}

fn monitor_services(services_str: &str, interval: u64) {
    let services: Vec<String> = services_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if services.is_empty() {
        eprintln!("{} No services to monitor", "Error:".red());
        return;
    }

    println!("\n{} Monitoring services: {}", "Info:".blue(), services.join(", "));
    println!("{} Check interval: {} seconds", "Info:".blue(), interval);
    println!("{} Press Ctrl+C to stop\n", "Info:".blue());

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
        println!("\n{} Stopping monitor...", "Info:".blue());
    })
    .expect("Error setting Ctrl+C handler");

    let manager = match ServiceManager::open() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            return;
        }
    };

    while running.load(Ordering::SeqCst) {
        for service_name in &services {
            match manager.is_service_running(service_name) {
                Ok(true) => {
                    println!("{} Service '{}' is running, stopping it...", "Monitor:".yellow(), service_name);
                    if let Err(e) = manager.stop_service(service_name) {
                        eprintln!("{} Failed to stop '{}': {}", "Error:".red(), service_name, e);
                    } else {
                        println!("{} Service '{}' stopped", "Monitor:".green(), service_name);
                    }
                }
                Ok(false) => {
                    // Service is stopped, no action needed
                }
                Err(e) => {
                    eprintln!("{} Error checking '{}': {}", "Error:".red(), service_name, e);
                }
            }
        }

        std::thread::sleep(Duration::from_secs(interval));
    }

    println!("{} Monitor stopped", "Info:".blue());
}

fn blacklist_service(name: &str) {
    let mut config = Config::load();
    config.blacklisted_services.insert(name.to_string());

    match config.save() {
        Ok(_) => {
            println!("{} Service '{}' added to blacklist", "Success:".green(), name);
        }
        Err(e) => {
            eprintln!("{} Failed to save config: {}", "Error:".red(), e);
        }
    }
}

fn unblacklist_service(name: &str) {
    let mut config = Config::load();
    if config.blacklisted_services.remove(name) {
        match config.save() {
            Ok(_) => {
                println!("{} Service '{}' removed from blacklist", "Success:".green(), name);
            }
            Err(e) => {
                eprintln!("{} Failed to save config: {}", "Error:".red(), e);
            }
        }
    } else {
        println!("{} Service '{}' not in blacklist", "Info:".blue(), name);
    }
}

fn list_blacklist() {
    let config = Config::load();

    if config.blacklisted_services.is_empty() {
        println!("{} No services in blacklist", "Info:".blue());
    } else {
        println!("\n{} Blacklisted services:", "Info:".blue());
        for service in &config.blacklisted_services {
            println!("  - {}", service);
        }
    }
}

fn apply_blacklist() {
    let config = Config::load();

    if config.blacklisted_services.is_empty() {
        println!("{} No services in blacklist", "Info:".blue());
        return;
    }

    let manager = match ServiceManager::open() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            return;
        }
    };

    println!("\n{} Applying blacklist...", "Info:".blue());

    for service_name in &config.blacklisted_services {
        match manager.stop_service(service_name) {
            Ok(_) => {
                println!("{} Service '{}' stopped", "Success:".green(), service_name);
            }
            Err(e) => {
                eprintln!("{} Failed to stop '{}': {}", "Error:".red(), service_name, e);
            }
        }
    }
}
