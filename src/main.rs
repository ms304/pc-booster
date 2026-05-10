mod config;
mod network_monitor;
mod service_manager;
mod system_info;
mod tui;

use clap::{Parser, Subcommand};
use colored::*;
use config::Config;
use service_manager::ServiceManager;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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
    /// Launch interactive TUI interface
    Tui,
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
        Commands::Tui => {
            run_tui();
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

fn run_tui() {
    println!("{} Starting TUI interface...", "Info:".blue());

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    if let Err(e) = rt.block_on(tui::run_tui()) {
        eprintln!("{} TUI error: {}", "Error:".red(), e);
        std::process::exit(1);
    }
}
