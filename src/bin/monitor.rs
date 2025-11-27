use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ethers::core::types::Address;
use nilav::config::{MonitorCliArgs, MonitorConfig};
use nilav::contract_client::{ContractConfig, NilAVWsClient};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, TableState, Wrap},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Convert a byte array to a 0x-prefixed hex string
fn bytes_to_hex(bytes: &[u8]) -> String {
    format!(
        "0x{}",
        bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    )
}

/// Format an address or hash to a shortened format (0x1234...5678)
fn format_short_hex(hex: &str) -> String {
    if hex.len() > 12 {
        format!("{}...{}", &hex[..6], &hex[hex.len() - 4..])
    } else {
        hex.to_string()
    }
}

/// Tracks the state of an HTX transaction across all stages
#[derive(Debug, Clone)]
struct HTXTransaction {
    htx_id: String,
    submitted_sender: Option<String>,
    assigned_node: Option<String>,
    responded: Option<bool>,
    timestamp: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Nodes,
    HTXTracking,
    HTXSubmitted,
    HTXAssigned,
    HTXResponded,
}

impl Tab {
    fn title(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Nodes => "Nodes",
            Tab::HTXTracking => "HTX Tracking",
            Tab::HTXSubmitted => "HTX Submitted",
            Tab::HTXAssigned => "HTX Assigned",
            Tab::HTXResponded => "HTX Responded",
        }
    }

    fn next(&self) -> Self {
        match self {
            Tab::Overview => Tab::Nodes,
            Tab::Nodes => Tab::HTXTracking,
            Tab::HTXTracking => Tab::HTXSubmitted,
            Tab::HTXSubmitted => Tab::HTXAssigned,
            Tab::HTXAssigned => Tab::HTXResponded,
            Tab::HTXResponded => Tab::Overview,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Tab::Overview => Tab::HTXResponded,
            Tab::Nodes => Tab::Overview,
            Tab::HTXTracking => Tab::Nodes,
            Tab::HTXSubmitted => Tab::HTXTracking,
            Tab::HTXAssigned => Tab::HTXSubmitted,
            Tab::HTXResponded => Tab::HTXAssigned,
        }
    }
}

struct MonitorState {
    current_tab: Tab,
    should_quit: bool,
    last_update: std::time::Instant,
    node_count: usize,
    nodes: Vec<Address>,
    htx_submitted: Vec<String>,
    htx_assigned: Vec<String>,
    htx_responded: Vec<String>,
    htx_tracking: HashMap<String, HTXTransaction>,
    status_message: String,
    selected_node_index: Option<usize>,
    show_confirm_deregister: bool,
    htx_submitted_state: ListState,
    htx_assigned_state: ListState,
    htx_responded_state: ListState,
    htx_tracking_state: TableState,
    rpc_url: String,
    contract_address: Address,
    public_key: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = MonitorCliArgs::parse();
    let config = MonitorConfig::load(cli_args)?;

    // Store values before they're moved
    let rpc_url = config.rpc_url.clone();
    let contract_address = config.contract_address;

    let contract_config = ContractConfig::new(config.rpc_url, config.contract_address);
    let client = NilAVWsClient::new(contract_config, config.private_key).await?;

    // Initial data fetch for node count and list
    let node_count = client.node_count().await?.as_usize();
    let nodes = client.get_nodes().await?;

    // Fetch historical events to populate initial state
    let htx_submitted = if config.all_htxs {
        match client.get_htx_submitted_events().await {
            Ok(events) => events
                .iter()
                .map(|e| {
                    let htx_id = bytes_to_hex(&e.htx_id);
                    let sender = format!("{:?}", e.sender);
                    format!(
                        "HTX: {} | Sender: {}",
                        format_short_hex(&htx_id),
                        format_short_hex(&sender)
                    )
                })
                .collect(),
            Err(e) => {
                eprintln!("Failed to fetch historical HTX submitted events: {}", e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let htx_assigned = if config.all_htxs {
        match client.get_htx_assigned_events().await {
            Ok(events) => events
                .iter()
                .map(|e| {
                    let htx_id = bytes_to_hex(&e.htx_id);
                    let node = format!("{:?}", e.node);
                    format!(
                        "HTX: {} | Node: {}",
                        format_short_hex(&htx_id),
                        format_short_hex(&node)
                    )
                })
                .collect(),
            Err(e) => {
                eprintln!("Failed to fetch historical HTX assigned events: {}", e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let htx_responded = if config.all_htxs {
        match client.get_htx_responded_events().await {
            Ok(events) => events
                .iter()
                .map(|e| {
                    let htx_id = bytes_to_hex(&e.htx_id);
                    let node = format!("{:?}", e.node);
                    format!(
                        "HTX: {} | Node: {} | Result: {}",
                        format_short_hex(&htx_id),
                        format_short_hex(&node),
                        e.result
                    )
                })
                .collect(),
            Err(e) => {
                eprintln!("Failed to fetch historical HTX responded events: {}", e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let total_events = htx_submitted.len() + htx_assigned.len() + htx_responded.len();

    // Build HTX tracking map from historical events
    let mut htx_tracking: HashMap<String, HTXTransaction> = HashMap::new();

    // Add submitted events
    if config.all_htxs {
        if let Ok(events) = client.get_htx_submitted_events().await {
            for e in events {
                let htx_id = bytes_to_hex(&e.htx_id);
                let sender = format!("{:?}", e.sender);
                htx_tracking.insert(
                    htx_id.clone(),
                    HTXTransaction {
                        htx_id,
                        submitted_sender: Some(sender),
                        assigned_node: None,
                        responded: None,
                        timestamp: SystemTime::now(),
                    },
                );
            }
        }

        // Add assigned events
        if let Ok(events) = client.get_htx_assigned_events().await {
            for e in events {
                let htx_id = bytes_to_hex(&e.htx_id);
                let node = format!("{:?}", e.node);
                htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| tx.assigned_node = Some(node.clone()))
                    .or_insert(HTXTransaction {
                        htx_id,
                        submitted_sender: None,
                        assigned_node: Some(node),
                        responded: None,
                        timestamp: SystemTime::now(),
                    });
            }
        }

        // Add responded events
        if let Ok(events) = client.get_htx_responded_events().await {
            for e in events {
                let htx_id = bytes_to_hex(&e.htx_id);
                htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| tx.responded = Some(e.result))
                    .or_insert(HTXTransaction {
                        htx_id,
                        submitted_sender: None,
                        assigned_node: None,
                        responded: Some(e.result),
                        timestamp: SystemTime::now(),
                    });
            }
        }
    }

    let initial_state = MonitorState {
        current_tab: Tab::Overview,
        should_quit: false,
        last_update: std::time::Instant::now(),
        node_count,
        nodes,
        htx_submitted,
        htx_assigned,
        htx_responded,
        htx_tracking,
        status_message: format!(
            "Press 'q' to quit, 'r' to refresh, Tab/Shift+Tab to navigate - Live WebSocket mode (Loaded {} historical events)",
            total_events
        ),
        selected_node_index: None,
        show_confirm_deregister: false,
        htx_submitted_state: ListState::default(),
        htx_assigned_state: ListState::default(),
        htx_responded_state: ListState::default(),
        htx_tracking_state: TableState::default(),
        rpc_url,
        contract_address,
        public_key: format!("{:?}", client.signer_address()),
    };

    run_monitor(client, initial_state).await
}

async fn run_monitor(client: NilAVWsClient, initial_state: MonitorState) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Wrap state in Arc<Mutex> for thread-safe sharing
    let state = Arc::new(Mutex::new(initial_state));
    let client_arc = Arc::new(client);

    // Spawn WebSocket event listeners
    let state_clone = state.clone();
    let client_clone = client_arc.clone();
    tokio::spawn(async move {
        let _ = listen_htx_submitted(client_clone, state_clone).await;
    });

    let state_clone = state.clone();
    let client_clone = client_arc.clone();
    tokio::spawn(async move {
        let _ = listen_htx_assigned(client_clone, state_clone).await;
    });

    let state_clone = state.clone();
    let client_clone = client_arc.clone();
    tokio::spawn(async move {
        let _ = listen_htx_responded(client_clone, state_clone).await;
    });

    let result = run_monitor_loop(&mut terminal, client_arc, state).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_monitor_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: Arc<NilAVWsClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    loop {
        // Draw UI with current state
        {
            let mut state_guard = state.lock().unwrap();
            terminal.draw(|f| {
                ui(f, &mut state_guard);
                if state_guard.show_confirm_deregister {
                    render_confirm_dialog(f, &state_guard);
                }
            })?;
        }

        // Handle input with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let mut state_guard = state.lock().unwrap();

                    // Handle confirmation dialog first
                    if state_guard.show_confirm_deregister {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(idx) = state_guard.selected_node_index {
                                    if idx < state_guard.nodes.len() {
                                        let node_addr = state_guard.nodes[idx];
                                        state_guard.status_message =
                                            format!("Deregistering node {:?}...", node_addr);
                                        drop(state_guard); // Release lock before async call

                                        match client.deregister_node(node_addr).await {
                                            Ok(tx_hash) => {
                                                let mut state_guard = state.lock().unwrap();
                                                state_guard.status_message =
                                                    format!("Node deregistered! TX: {:?}", tx_hash);
                                                // Refresh node list
                                                match client.get_nodes().await {
                                                    Ok(nodes) => {
                                                        state_guard.nodes = nodes;
                                                        state_guard.node_count =
                                                            state_guard.nodes.len();
                                                    }
                                                    Err(e) => {
                                                        state_guard.status_message =
                                                            format!("Error refreshing: {}", e);
                                                    }
                                                }
                                                state_guard.selected_node_index = None;
                                            }
                                            Err(e) => {
                                                let mut state_guard = state.lock().unwrap();
                                                state_guard.status_message =
                                                    format!("Error: {}", e);
                                            }
                                        }
                                        continue; // Skip to next iteration since we dropped the lock
                                    }
                                }
                                state_guard.show_confirm_deregister = false;
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                state_guard.show_confirm_deregister = false;
                            }
                            _ => {}
                        }
                    } else {
                        // Normal key handling
                        match key.code {
                            KeyCode::Char('q') => {
                                state_guard.should_quit = true;
                                break;
                            }
                            KeyCode::Char('r') => {
                                // Manual refresh - update node list
                                state_guard.status_message = "Refreshing...".to_string();
                                drop(state_guard);

                                match client.get_nodes().await {
                                    Ok(nodes) => {
                                        let mut state_guard = state.lock().unwrap();
                                        state_guard.nodes = nodes;
                                        state_guard.node_count = state_guard.nodes.len();
                                        state_guard.status_message = "Refreshed!".to_string();
                                    }
                                    Err(e) => {
                                        let mut state_guard = state.lock().unwrap();
                                        state_guard.status_message = format!("Error: {}", e);
                                    }
                                }
                                continue;
                            }
                            KeyCode::Tab => {
                                state_guard.current_tab = state_guard.current_tab.next();
                                state_guard.selected_node_index = None;
                            }
                            KeyCode::BackTab => {
                                state_guard.current_tab = state_guard.current_tab.prev();
                                state_guard.selected_node_index = None;
                            }
                            KeyCode::Right => {
                                state_guard.current_tab = state_guard.current_tab.next();
                                state_guard.selected_node_index = None;
                            }
                            KeyCode::Left => {
                                state_guard.current_tab = state_guard.current_tab.prev();
                                state_guard.selected_node_index = None;
                            }
                            KeyCode::Down => match state_guard.current_tab {
                                Tab::Nodes if !state_guard.nodes.is_empty() => {
                                    state_guard.selected_node_index = Some(
                                        state_guard
                                            .selected_node_index
                                            .map(|idx| (idx + 1).min(state_guard.nodes.len() - 1))
                                            .unwrap_or(0),
                                    );
                                }
                                Tab::HTXTracking if !state_guard.htx_tracking.is_empty() => {
                                    let i = state_guard.htx_tracking_state.selected().unwrap_or(0);
                                    let next = (i + 1).min(state_guard.htx_tracking.len() - 1);
                                    state_guard.htx_tracking_state.select(Some(next));
                                }
                                Tab::HTXSubmitted if !state_guard.htx_submitted.is_empty() => {
                                    let i = state_guard.htx_submitted_state.selected().unwrap_or(0);
                                    let next = (i + 1).min(state_guard.htx_submitted.len() - 1);
                                    state_guard.htx_submitted_state.select(Some(next));
                                }
                                Tab::HTXAssigned if !state_guard.htx_assigned.is_empty() => {
                                    let i = state_guard.htx_assigned_state.selected().unwrap_or(0);
                                    let next = (i + 1).min(state_guard.htx_assigned.len() - 1);
                                    state_guard.htx_assigned_state.select(Some(next));
                                }
                                Tab::HTXResponded if !state_guard.htx_responded.is_empty() => {
                                    let i = state_guard.htx_responded_state.selected().unwrap_or(0);
                                    let next = (i + 1).min(state_guard.htx_responded.len() - 1);
                                    state_guard.htx_responded_state.select(Some(next));
                                }
                                _ => {}
                            },
                            KeyCode::Up => match state_guard.current_tab {
                                Tab::Nodes if !state_guard.nodes.is_empty() => {
                                    state_guard.selected_node_index = Some(
                                        state_guard
                                            .selected_node_index
                                            .map(|idx| idx.saturating_sub(1))
                                            .unwrap_or(0),
                                    );
                                }
                                Tab::HTXTracking if !state_guard.htx_tracking.is_empty() => {
                                    let i = state_guard.htx_tracking_state.selected().unwrap_or(0);
                                    state_guard
                                        .htx_tracking_state
                                        .select(Some(i.saturating_sub(1)));
                                }
                                Tab::HTXSubmitted if !state_guard.htx_submitted.is_empty() => {
                                    let i = state_guard.htx_submitted_state.selected().unwrap_or(0);
                                    state_guard
                                        .htx_submitted_state
                                        .select(Some(i.saturating_sub(1)));
                                }
                                Tab::HTXAssigned if !state_guard.htx_assigned.is_empty() => {
                                    let i = state_guard.htx_assigned_state.selected().unwrap_or(0);
                                    state_guard
                                        .htx_assigned_state
                                        .select(Some(i.saturating_sub(1)));
                                }
                                Tab::HTXResponded if !state_guard.htx_responded.is_empty() => {
                                    let i = state_guard.htx_responded_state.selected().unwrap_or(0);
                                    state_guard
                                        .htx_responded_state
                                        .select(Some(i.saturating_sub(1)));
                                }
                                _ => {}
                            },
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                // Only allow deregistration in Nodes tab with selection
                                if state_guard.current_tab == Tab::Nodes
                                    && state_guard.selected_node_index.is_some()
                                {
                                    state_guard.show_confirm_deregister = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// WebSocket event listener for HTX submitted events
async fn listen_htx_submitted(
    client: Arc<NilAVWsClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    client
        .listen_htx_submitted_events(move |event| {
            let state = state.clone();
            async move {
                let htx_id = bytes_to_hex(&event.htx_id);
                let sender = format!("{:?}", event.sender);
                let entry = format!(
                    "HTX: {} | Sender: {}",
                    format_short_hex(&htx_id),
                    format_short_hex(&sender)
                );

                let mut state_guard = state.lock().unwrap();
                state_guard.htx_submitted.push(entry);

                // Update tracking map
                state_guard
                    .htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| tx.submitted_sender = Some(sender.clone()))
                    .or_insert(HTXTransaction {
                        htx_id,
                        submitted_sender: Some(sender),
                        assigned_node: None,
                        responded: None,
                        timestamp: SystemTime::now(),
                    });

                state_guard.last_update = std::time::Instant::now();
                Ok(())
            }
        })
        .await
}

// WebSocket event listener for HTX assigned events
async fn listen_htx_assigned(
    client: Arc<NilAVWsClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    client
        .listen_htx_assigned_events(move |event| {
            let state = state.clone();
            async move {
                let htx_id = bytes_to_hex(&event.htx_id);
                let node = format!("{:?}", event.node);
                let entry = format!(
                    "HTX: {} | Node: {}",
                    format_short_hex(&htx_id),
                    format_short_hex(&node)
                );

                let mut state_guard = state.lock().unwrap();
                state_guard.htx_assigned.push(entry);

                // Update tracking map
                state_guard
                    .htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| tx.assigned_node = Some(node.clone()))
                    .or_insert(HTXTransaction {
                        htx_id,
                        submitted_sender: None,
                        assigned_node: Some(node),
                        responded: None,
                        timestamp: SystemTime::now(),
                    });

                state_guard.last_update = std::time::Instant::now();
                Ok(())
            }
        })
        .await
}

// WebSocket event listener for HTX responded events
async fn listen_htx_responded(
    client: Arc<NilAVWsClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    client
        .listen_htx_responded_events(move |event| {
            let state = state.clone();
            async move {
                let htx_id = bytes_to_hex(&event.htx_id);
                let node = format!("{:?}", event.node);
                let entry = format!(
                    "HTX: {} | Node: {} | Result: {}",
                    format_short_hex(&htx_id),
                    format_short_hex(&node),
                    event.result
                );

                let mut state_guard = state.lock().unwrap();
                state_guard.htx_responded.push(entry);

                // Update tracking map
                state_guard
                    .htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| tx.responded = Some(event.result))
                    .or_insert(HTXTransaction {
                        htx_id,
                        submitted_sender: None,
                        assigned_node: None,
                        responded: Some(event.result),
                        timestamp: SystemTime::now(),
                    });

                state_guard.last_update = std::time::Instant::now();
                Ok(())
            }
        })
        .await
}

fn ui(f: &mut Frame, state: &mut MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Content
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    // Header
    render_header(f, chunks[0], state);

    // Content
    render_content(f, chunks[1], state);

    // Footer
    render_footer(f, chunks[2], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &MonitorState) {
    let tab_spans: Vec<Span> = [
        Tab::Overview,
        Tab::Nodes,
        Tab::HTXTracking,
        Tab::HTXSubmitted,
        Tab::HTXAssigned,
        Tab::HTXResponded,
    ]
    .iter()
    .flat_map(|tab| {
        let style = if *tab == state.current_tab {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        vec![
            Span::raw(" "),
            Span::styled(tab.title(), style),
            Span::raw(" │"),
        ]
    })
    .collect();

    let header = Paragraph::new(Line::from(tab_spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("NilAV Contract Monitor")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(header, area);
}

fn render_content(f: &mut Frame, area: Rect, state: &mut MonitorState) {
    match state.current_tab {
        Tab::Overview => render_overview(f, area, state),
        Tab::Nodes => render_nodes(f, area, state),
        Tab::HTXTracking => render_htx_tracking(f, area, state),
        Tab::HTXSubmitted => render_htx_list(
            f,
            area,
            &state.htx_submitted,
            "HTX Submitted Events",
            &mut state.htx_submitted_state,
        ),
        Tab::HTXAssigned => render_htx_list(
            f,
            area,
            &state.htx_assigned,
            "HTX Assigned Events",
            &mut state.htx_assigned_state,
        ),
        Tab::HTXResponded => render_htx_list(
            f,
            area,
            &state.htx_responded,
            "HTX Responded Events",
            &mut state.htx_responded_state,
        ),
    }
}

fn render_overview(f: &mut Frame, area: Rect, state: &MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // Connection info
            Constraint::Length(7), // Stats
            Constraint::Min(0),    // Details
        ])
        .split(area);

    // Connection Info
    let info_text = vec![
        Line::from(vec![
            Span::styled("RPC URL: ", Style::default().fg(Color::Cyan)),
            Span::styled(state.rpc_url.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Contract: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{:?}", state.contract_address),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Signer: ", Style::default().fg(Color::Cyan)),
            Span::styled(state.public_key.clone(), Style::default().fg(Color::White)),
        ]),
    ];

    let info = Paragraph::new(info_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Connection Info")
            .title_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(info, chunks[0]);

    // Stats
    let stats_text = vec![
        Line::from(vec![
            Span::styled("Total Nodes: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                state.node_count.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("HTX Submitted: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                state.htx_submitted.len().to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("HTX Assigned: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                state.htx_assigned.len().to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("HTX Responded: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                state.htx_responded.len().to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
    ];

    let stats = Paragraph::new(stats_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Contract Statistics")
            .title_style(Style::default().fg(Color::Green)),
    );

    f.render_widget(stats, chunks[1]);

    // Recent activity
    let mut activity_lines = vec![];

    activity_lines.push(Line::from(vec![Span::styled(
        "Recent Activity:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    activity_lines.push(Line::from(""));

    // Show last 3 of each
    if !state.htx_responded.is_empty() {
        activity_lines.push(Line::from(vec![Span::styled(
            "Latest Responses:",
            Style::default().fg(Color::Yellow),
        )]));
        for item in state.htx_responded.iter().rev().take(3) {
            activity_lines.push(Line::from(vec![Span::raw("  • "), Span::raw(item)]));
        }
        activity_lines.push(Line::from(""));
    }

    if !state.htx_assigned.is_empty() {
        activity_lines.push(Line::from(vec![Span::styled(
            "Latest Assignments:",
            Style::default().fg(Color::Yellow),
        )]));
        for item in state.htx_assigned.iter().rev().take(3) {
            activity_lines.push(Line::from(vec![Span::raw("  • "), Span::raw(item)]));
        }
    }

    let activity = Paragraph::new(activity_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recent Activity")
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(activity, chunks[2]);
}

fn render_nodes(f: &mut Frame, area: Rect, state: &MonitorState) {
    let items: Vec<ListItem> = state
        .nodes
        .iter()
        .enumerate()
        .map(|(idx, addr)| {
            let content = format!("{}. {:?}", idx + 1, addr);
            let style = if Some(idx) == state.selected_node_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(content).style(style)
        })
        .collect();

    let title = if state.selected_node_index.is_some() {
        format!(
            "Registered Nodes ({}) - Press 'd' to deregister",
            state.nodes.len()
        )
    } else {
        format!(
            "Registered Nodes ({}) - Use ↑↓ to select",
            state.nodes.len()
        )
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(Style::default().fg(Color::Green)),
    );

    f.render_widget(list, area);
}

fn render_htx_tracking(f: &mut Frame, area: Rect, state: &mut MonitorState) {
    use ratatui::widgets::{Row, Table};

    // Convert HashMap to sorted Vec for display (reverse order - newest first)
    let mut transactions: Vec<&HTXTransaction> = state.htx_tracking.values().collect();
    transactions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let total = transactions.len();

    // Create table rows with numbering from n to 1
    let rows: Vec<Row> = transactions
        .iter()
        .enumerate()
        .map(|(idx, tx)| {
            let row_num = total - idx; // Count from n down to 1

            let submitted = match &tx.submitted_sender {
                Some(sender) => format_short_hex(sender),
                None => "-".to_string(),
            };

            let assigned = match &tx.assigned_node {
                Some(node) => format_short_hex(node),
                None => "-".to_string(),
            };

            let responded = match tx.responded {
                Some(true) => "✓ True",
                Some(false) => "✗ False",
                None => "-",
            };

            Row::new(vec![
                row_num.to_string(),
                format_short_hex(&tx.htx_id),
                submitted,
                assigned,
                responded.to_string(),
            ])
            .style(Style::default().fg(Color::White))
        })
        .collect();

    // Create table header
    let header = Row::new(vec![
        "#",
        "HTX ID",
        "Submitted (Sender)",
        "Assigned (Node)",
        "Responded",
    ])
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let scroll_help = if !transactions.is_empty() {
        " - Use ↑↓ to scroll"
    } else {
        ""
    };

    // Create table
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),      // # column
            Constraint::Percentage(18), // HTX ID
            Constraint::Percentage(27), // Submitted
            Constraint::Percentage(27), // Assigned
            Constraint::Percentage(20), // Responded
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                "HTX Transaction Tracking ({}){}",
                transactions.len(),
                scroll_help
            ))
            .title_style(Style::default().fg(Color::Green)),
    )
    .row_highlight_style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ");

    // Initialize selection if not set and items exist
    if state.htx_tracking_state.selected().is_none() && !transactions.is_empty() {
        state.htx_tracking_state.select(Some(0));
    }

    f.render_stateful_widget(table, area, &mut state.htx_tracking_state);
}

fn render_htx_list(
    f: &mut Frame,
    area: Rect,
    items: &[String],
    title: &str,
    list_state: &mut ListState,
) {
    // Initialize selection if not set and items exist
    if list_state.selected().is_none() && !items.is_empty() {
        list_state.select(Some(0));
    }

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(_, item)| ListItem::new(item.clone()))
        .collect();

    let scroll_help = if !items.is_empty() {
        " - Use ↑↓ to scroll"
    } else {
        ""
    };

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("{} ({}){}", title, items.len(), scroll_help))
                .title_style(Style::default().fg(Color::Green)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, list_state);
}

fn render_footer(f: &mut Frame, area: Rect, state: &MonitorState) {
    let mut help_spans = vec![
        Span::styled(
            "q",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Quit  "),
        Span::styled(
            "r",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Refresh  "),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("/"),
        Span::styled(
            "←→",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Navigate  "),
    ];

    // Add node-specific controls when in Nodes tab
    if state.current_tab == Tab::Nodes {
        help_spans.extend(vec![
            Span::styled(
                "↑↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Select  "),
            Span::styled(
                "d",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Deregister  "),
        ]);
    }

    help_spans.push(Span::styled(
        "Auto-refresh: 5s",
        Style::default().fg(Color::Gray),
    ));

    let help_text = vec![Line::from(help_spans)];

    let footer = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Controls")
            .title_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(footer, area);
}

fn render_confirm_dialog(f: &mut Frame, state: &MonitorState) {
    // Create a centered popup area
    let area = f.area();
    let popup_width = 60.min(area.width - 4);
    let popup_height = 12.min(area.height - 4);
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    let node_text = if let Some(idx) = state.selected_node_index {
        if idx < state.nodes.len() {
            format!("{:?}", state.nodes[idx])
        } else {
            "Unknown".to_string()
        }
    } else {
        "Unknown".to_string()
    };

    let text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "⚠ Confirm Deregistration",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::raw("Are you sure you want to deregister node:")]),
        Line::from(""),
        Line::from(vec![Span::styled(
            node_text,
            Style::default().fg(Color::Cyan),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Y",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Yes  "),
            Span::styled(
                "N",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw("/"),
            Span::styled(
                "ESC",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": No"),
        ]),
    ];

    let dialog = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirm Action")
                .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                .style(Style::default().bg(Color::Black)),
        )
        .alignment(Alignment::Center);

    f.render_widget(dialog, popup_area);
}
