use crate::network_monitor::check_network_connectivity;
use crate::service_manager::{ServiceInfo, ServiceManager};
use crate::system_info::SystemInfo;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct AppState {
    pub services: Vec<ServiceInfo>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub system_info: SystemInfo,
    pub last_refresh: Instant,
    pub status_message: Option<String>,
    pub status_message_time: Instant,
    pub network_protection_enabled: bool,
}

impl AppState {
    pub fn new() -> Self {
        let manager = ServiceManager::open().unwrap_or_else(|e| {
            eprintln!("Failed to open service manager: {}", e);
            std::process::exit(1);
        });

        let mut services = manager.list_services().unwrap_or_else(|e| {
            eprintln!("Failed to list services: {}", e);
            Vec::new()
        });

        // Fetch descriptions for all services
        for service in &mut services {
            if let Ok(desc) = manager.get_service_description(&service.name) {
                service.description = Some(desc);
            }
        }

        // Sort services: running first, then stopped
        services.sort_by(|a, b| {
            let a_running = a.status == "Running";
            let b_running = b.status == "Running";
            if a_running && !b_running {
                std::cmp::Ordering::Less
            } else if !a_running && b_running {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        Self {
            services,
            selected_index: 0,
            scroll_offset: 0,
            system_info: SystemInfo::new(),
            last_refresh: Instant::now(),
            status_message: None,
            status_message_time: Instant::now(),
            network_protection_enabled: true,
        }
    }

    pub fn refresh_services(&mut self) {
        let manager = ServiceManager::open().unwrap_or_else(|e| {
            eprintln!("Failed to open service manager: {}", e);
            std::process::exit(1);
        });

        if let Ok(mut new_services) = manager.list_services() {
            // Fetch descriptions for all services
            for service in &mut new_services {
                if let Ok(desc) = manager.get_service_description(&service.name) {
                    service.description = Some(desc);
                }
            }

            // Sort services
            new_services.sort_by(|a, b| {
                let a_running = a.status == "Running";
                let b_running = b.status == "Running";
                if a_running && !b_running {
                    std::cmp::Ordering::Less
                } else if !a_running && b_running {
                    std::cmp::Ordering::Greater
                } else {
                    a.name.cmp(&b.name)
                }
            });

            // Preserve selection if possible
            let selected_name = self.services.get(self.selected_index)
                .map(|s| s.name.clone());

            self.services = new_services;

            if let Some(name) = selected_name {
                if let Some(new_index) = self.services.iter().position(|s| s.name == name) {
                    self.selected_index = new_index;
                }
            }

            self.last_refresh = Instant::now();
        }
    }

    pub fn set_status_message(&mut self, message: String) {
        self.status_message = Some(message);
        self.status_message_time = Instant::now();
    }

    pub fn clear_status_message_if_expired(&mut self) {
        if self.status_message.is_some() && self.status_message_time.elapsed() > Duration::from_secs(5) {
            self.status_message = None;
        }
    }
}

pub async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_state = Arc::new(Mutex::new(AppState::new()));

    // Spawn a background task to refresh system info periodically
    let app_state_clone = app_state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut state = app_state_clone.lock().await;
            state.system_info.refresh();
            state.clear_status_message_if_expired();
        }
    });

    let result = run_tui_loop(&mut terminal, app_state).await;

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        io::stdout(),
        crossterm::terminal::LeaveAlternateScreen
    )?;

    result
}

async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app_state: Arc<Mutex<AppState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| {
            let state = app_state.blocking_lock();
            draw_ui(f, &state);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let mut state = app_state.lock().await;
                handle_key_event(&mut state, key, &app_state).await?;

                if should_quit(&state) {
                    return Ok(());
                }
            }
        }
    }
}

fn should_quit(state: &AppState) -> bool {
    state.status_message.as_ref()
        .map(|m| m.contains("Exiting"))
        .unwrap_or(false)
}

async fn handle_key_event(
    state: &mut AppState,
    key: KeyEvent,
    app_state: &Arc<Mutex<AppState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    match key.code {
        KeyCode::Up => {
            if state.selected_index > 0 {
                state.selected_index -= 1;
                if state.selected_index < state.scroll_offset {
                    state.scroll_offset = state.selected_index;
                }
            }
        }
        KeyCode::Down => {
            if !state.services.is_empty() && state.selected_index < state.services.len() - 1 {
                state.selected_index += 1;
                let visible_height = 20; // Approximate visible items
                if state.selected_index >= state.scroll_offset + visible_height {
                    state.scroll_offset = state.selected_index - visible_height + 1;
                }
            }
        }
        KeyCode::PageUp => {
            state.selected_index = state.selected_index.saturating_sub(10);
            state.scroll_offset = state.scroll_offset.saturating_sub(10);
        }
        KeyCode::PageDown => {
            state.selected_index = (state.selected_index + 10).min(state.services.len().saturating_sub(1));
            state.scroll_offset = (state.scroll_offset + 10).min(state.services.len().saturating_sub(20));
        }
        KeyCode::Enter => {
            if let Some(service) = state.services.get(state.selected_index) {
                let service_name = service.name.clone();
                let is_running = service.status == "Running";

                if is_running {
                    // Stop the service with network protection
                    stop_service_with_protection(state, &service_name, app_state).await?;
                } else {
                    // Start the service
                    start_service(state, &service_name).await?;
                }

                // Refresh services after action
                state.refresh_services();
            }
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            state.set_status_message("Exiting...".to_string());
        }
        KeyCode::Char('r') => {
            state.refresh_services();
            state.set_status_message("Services refreshed".to_string());
        }
        KeyCode::Char('p') => {
            state.network_protection_enabled = !state.network_protection_enabled;
            let status = if state.network_protection_enabled { "enabled" } else { "disabled" };
            state.set_status_message(format!("Network protection {}", status));
        }
        _ => {}
    }

    Ok(())
}

async fn stop_service_with_protection(
    state: &mut AppState,
    service_name: &str,
    app_state: &Arc<Mutex<AppState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ServiceManager::open()?;

    // Check network connectivity before stopping
    let network_before = check_network_connectivity();

    match manager.stop_service(service_name) {
        Ok(_) => {
            state.set_status_message(format!("Service '{}' stopped", service_name));

            // Network protection: monitor for 5 seconds
            if state.network_protection_enabled && network_before {
                let service_name_clone = service_name.to_string();
                let app_state_clone = app_state.clone();

                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;

                    for _ in 0..5 {
                        tokio::time::sleep(Duration::from_secs(1)).await;

                        if !check_network_connectivity() {
                            // Network lost! Restart the service
                            let manager = ServiceManager::open().unwrap();
                            if let Err(e) = manager.start_service(&service_name_clone) {
                                eprintln!("Failed to restart service '{}': {}", service_name_clone, e);
                            } else {
                                let mut state = app_state_clone.lock().await;
                                state.set_status_message(format!(
                                    "Network lost! Service '{}' restarted automatically",
                                    service_name_clone
                                ));
                                state.refresh_services();
                            }
                            return;
                        }
                    }
                });
            }
        }
        Err(e) => {
            state.set_status_message(format!("Failed to stop '{}': {}", service_name, e));
        }
    }

    Ok(())
}

async fn start_service(
    state: &mut AppState,
    service_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ServiceManager::open()?;

    match manager.start_service(service_name) {
        Ok(_) => {
            state.set_status_message(format!("Service '{}' started", service_name));
        }
        Err(e) => {
            state.set_status_message(format!("Failed to start '{}': {}", service_name, e));
        }
    }

    Ok(())
}

fn draw_ui(f: &mut Frame, state: &AppState) {
    let size = f.area();

    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),   // Services list
            Constraint::Length(3), // Footer
        ])
        .split(size);

    // Header
    let header_text = format!(
        "PC Booster - Services Manager    RAM: {} / {} ({:.1}%)    Network Protection: {}",
        SystemInfo::format_memory(state.system_info.used_memory()),
        SystemInfo::format_memory(state.system_info.total_memory()),
        state.system_info.memory_usage_percent(),
        if state.network_protection_enabled { "ON" } else { "OFF" }
    );

    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(header, chunks[0]);

    // Services list
    let services: Vec<ListItem> = state.services
        .iter()
        .enumerate()
        .map(|(i, service)| {
            let is_selected = i == state.selected_index;
            let status_color = match service.status.as_str() {
                "Running" => Color::Green,
                "Stopped" => Color::Red,
                "Starting" => Color::Yellow,
                "Stopping" => Color::Yellow,
                _ => Color::White,
            };

            let status_style = Style::default().fg(status_color);
            let selected_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let description = service.description.as_deref().unwrap_or("No description");

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{:<40}", service.name),
                        selected_style,
                    ),
                    Span::styled(
                        format!("{:<10}", service.status),
                        status_style,
                    ),
                    Span::styled(
                        format!("{:<50}", description),
                        selected_style,
                    ),
                ]),
            ])
        })
        .collect();

    let services_list = List::new(services)
        .block(Block::default()
            .borders(Borders::ALL)
            .title("Services (↑↓ navigate, Enter stop/start, q quit, r refresh, p toggle protection)"))
        .style(Style::default().fg(Color::White));

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(state.selected_index));

    f.render_stateful_widget(services_list, chunks[1], &mut list_state);

    // Footer with status message
    let footer_text = if let Some(msg) = &state.status_message {
        msg.clone()
    } else {
        format!("Total services: {} | Running: {} | Stopped: {}",
            state.services.len(),
            state.services.iter().filter(|s| s.status == "Running").count(),
            state.services.iter().filter(|s| s.status == "Stopped").count(),
        )
    };

    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, chunks[2]);
}
