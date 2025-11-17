use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ethers::core::types::Address;
use nilav::contract_client::{ContractConfig, NilAVClient};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

/// Format an address or hash to a shortened format (0x1234...5678)
fn format_short_hex(hex: &str) -> String {
    if hex.len() > 12 {
        format!("{}...{}", &hex[..6], &hex[hex.len() - 4..])
    } else {
        hex.to_string()
    }
}

/// NilAV Contract Monitor - Interactive TUI for monitoring the NilAV smart contract
#[derive(Parser)]
#[command(name = "monitor")]
#[command(about = "NilAV Contract Monitor - Interactive TUI", long_about = None)]
struct Cli {
    /// Ethereum RPC endpoint
    #[arg(long, env = "RPC_URL", default_value = "http://localhost:8545")]
    rpc_url: String,

    /// NilAV contract address
    #[arg(
        long,
        env = "CONTRACT_ADDRESS",
        default_value = "0x5FbDB2315678afecb367f032d93F642f64180aa3"
    )]
    contract_address: String,

    /// Private key for signing transactions
    #[arg(
        long,
        env = "PRIVATE_KEY",
        default_value = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    )]
    private_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Nodes,
    HTXSubmitted,
    HTXAssigned,
    HTXResponded,
}

impl Tab {
    fn title(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Nodes => "Nodes",
            Tab::HTXSubmitted => "HTX Submitted",
            Tab::HTXAssigned => "HTX Assigned",
            Tab::HTXResponded => "HTX Responded",
        }
    }

    fn next(&self) -> Self {
        match self {
            Tab::Overview => Tab::Nodes,
            Tab::Nodes => Tab::HTXSubmitted,
            Tab::HTXSubmitted => Tab::HTXAssigned,
            Tab::HTXAssigned => Tab::HTXResponded,
            Tab::HTXResponded => Tab::Overview,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Tab::Overview => Tab::HTXResponded,
            Tab::Nodes => Tab::Overview,
            Tab::HTXSubmitted => Tab::Nodes,
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
    status_message: String,
    selected_node_index: Option<usize>,
    show_confirm_deregister: bool,
    htx_submitted_state: ListState,
    htx_assigned_state: ListState,
    htx_responded_state: ListState,
}

impl MonitorState {
    fn new() -> Self {
        Self {
            current_tab: Tab::Overview,
            should_quit: false,
            last_update: std::time::Instant::now(),
            node_count: 0,
            nodes: Vec::new(),
            htx_submitted: Vec::new(),
            htx_assigned: Vec::new(),
            htx_responded: Vec::new(),
            status_message: "Press 'q' to quit, 'r' to refresh, Tab/Shift+Tab to navigate"
                .to_string(),
            selected_node_index: None,
            show_confirm_deregister: false,
            htx_submitted_state: ListState::default(),
            htx_assigned_state: ListState::default(),
            htx_responded_state: ListState::default(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let contract_address = cli.contract_address.parse::<Address>()?;
    let contract_config = ContractConfig::new(cli.rpc_url, contract_address);
    let client = NilAVClient::new(contract_config, cli.private_key).await?;

    run_monitor(client).await
}

async fn run_monitor(client: NilAVClient) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_monitor_loop(&mut terminal, client).await;

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
    client: NilAVClient,
) -> Result<()> {
    let mut state = MonitorState::new();

    // Initial data fetch
    fetch_data(&client, &mut state).await?;

    loop {
        terminal.draw(|f| {
            ui(f, &mut state);
            if state.show_confirm_deregister {
                render_confirm_dialog(f, &state);
            }
        })?;

        // Auto-refresh every 5 seconds
        if state.last_update.elapsed() > Duration::from_secs(5) {
            fetch_data(&client, &mut state).await?;
        }

        // Handle input with timeout
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Handle confirmation dialog first
                    if state.show_confirm_deregister {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(idx) = state.selected_node_index {
                                    if idx < state.nodes.len() {
                                        let node_addr = state.nodes[idx];
                                        state.status_message =
                                            format!("Deregistering node {:?}...", node_addr);
                                        match client.deregister_node(node_addr).await {
                                            Ok(tx_hash) => {
                                                state.status_message =
                                                    format!("Node deregistered! TX: {:?}", tx_hash);
                                                fetch_data(&client, &mut state).await?;
                                                state.selected_node_index = None;
                                            }
                                            Err(e) => {
                                                state.status_message = format!("Error: {}", e);
                                            }
                                        }
                                    }
                                }
                                state.show_confirm_deregister = false;
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                state.show_confirm_deregister = false;
                            }
                            _ => {}
                        }
                    } else {
                        // Normal key handling
                        match key.code {
                            KeyCode::Char('q') => {
                                state.should_quit = true;
                                break;
                            }
                            KeyCode::Char('r') => {
                                fetch_data(&client, &mut state).await?;
                            }
                            KeyCode::Tab => {
                                state.current_tab = state.current_tab.next();
                                state.selected_node_index = None;
                            }
                            KeyCode::BackTab => {
                                state.current_tab = state.current_tab.prev();
                                state.selected_node_index = None;
                            }
                            KeyCode::Right => {
                                state.current_tab = state.current_tab.next();
                                state.selected_node_index = None;
                            }
                            KeyCode::Left => {
                                state.current_tab = state.current_tab.prev();
                                state.selected_node_index = None;
                            }
                            KeyCode::Down => match state.current_tab {
                                Tab::Nodes if !state.nodes.is_empty() => {
                                    state.selected_node_index = Some(
                                        state
                                            .selected_node_index
                                            .map(|idx| (idx + 1).min(state.nodes.len() - 1))
                                            .unwrap_or(0),
                                    );
                                }
                                Tab::HTXSubmitted if !state.htx_submitted.is_empty() => {
                                    let i = state.htx_submitted_state.selected().unwrap_or(0);
                                    let next = (i + 1).min(state.htx_submitted.len() - 1);
                                    state.htx_submitted_state.select(Some(next));
                                }
                                Tab::HTXAssigned if !state.htx_assigned.is_empty() => {
                                    let i = state.htx_assigned_state.selected().unwrap_or(0);
                                    let next = (i + 1).min(state.htx_assigned.len() - 1);
                                    state.htx_assigned_state.select(Some(next));
                                }
                                Tab::HTXResponded if !state.htx_responded.is_empty() => {
                                    let i = state.htx_responded_state.selected().unwrap_or(0);
                                    let next = (i + 1).min(state.htx_responded.len() - 1);
                                    state.htx_responded_state.select(Some(next));
                                }
                                _ => {}
                            },
                            KeyCode::Up => match state.current_tab {
                                Tab::Nodes if !state.nodes.is_empty() => {
                                    state.selected_node_index = Some(
                                        state
                                            .selected_node_index
                                            .map(|idx| idx.saturating_sub(1))
                                            .unwrap_or(0),
                                    );
                                }
                                Tab::HTXSubmitted if !state.htx_submitted.is_empty() => {
                                    let i = state.htx_submitted_state.selected().unwrap_or(0);
                                    state.htx_submitted_state.select(Some(i.saturating_sub(1)));
                                }
                                Tab::HTXAssigned if !state.htx_assigned.is_empty() => {
                                    let i = state.htx_assigned_state.selected().unwrap_or(0);
                                    state.htx_assigned_state.select(Some(i.saturating_sub(1)));
                                }
                                Tab::HTXResponded if !state.htx_responded.is_empty() => {
                                    let i = state.htx_responded_state.selected().unwrap_or(0);
                                    state.htx_responded_state.select(Some(i.saturating_sub(1)));
                                }
                                _ => {}
                            },
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                // Only allow deregistration in Nodes tab with selection
                                if state.current_tab == Tab::Nodes
                                    && state.selected_node_index.is_some()
                                {
                                    state.show_confirm_deregister = true;
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

async fn fetch_data(client: &NilAVClient, state: &mut MonitorState) -> Result<()> {
    // Fetch node count and list
    let count = client.node_count().await?;
    state.node_count = count.as_usize();
    state.nodes = client.get_nodes().await?;

    // Fetch HTX submitted events
    let submitted_events = client.get_htx_submitted_events().await?;
    state.htx_submitted = submitted_events
        .iter()
        .map(|e| {
            let htx_id = format!("{:?}", e.htx_id);
            let sender = format!("{:?}", e.sender);
            format!(
                "HTX: {} | Sender: {}",
                format_short_hex(&htx_id),
                format_short_hex(&sender)
            )
        })
        .collect();

    // Fetch HTX assigned events
    let assigned_events = client.get_htx_assigned_events().await?;
    state.htx_assigned = assigned_events
        .iter()
        .map(|e| {
            let htx_id = format!("{:?}", e.htx_id);
            let node = format!("{:?}", e.node);
            format!(
                "HTX: {} | Node: {}",
                format_short_hex(&htx_id),
                format_short_hex(&node)
            )
        })
        .collect();

    // Fetch HTX responded events
    let responded_events = client.get_htx_responded_events().await?;
    state.htx_responded = responded_events
        .iter()
        .map(|e| {
            let htx_id = format!("{:?}", e.htx_id);
            let node = format!("{:?}", e.node);
            format!(
                "HTX: {} | Node: {} | Result: {}",
                format_short_hex(&htx_id),
                format_short_hex(&node),
                e.result
            )
        })
        .collect();

    state.last_update = std::time::Instant::now();

    Ok(())
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
            Constraint::Length(7), // Stats
            Constraint::Min(0),    // Details
        ])
        .split(area);

    // Stats
    let stats_text = vec![
        Line::from(vec![
            Span::styled("Contract Address: ", Style::default().fg(Color::Cyan)),
            Span::raw("Connected"),
        ]),
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

    f.render_widget(stats, chunks[0]);

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

    f.render_widget(activity, chunks[1]);
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
                .title_style(
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )
                .style(Style::default().bg(Color::Black)),
        )
        .alignment(Alignment::Center);

    f.render_widget(dialog, popup_area);
}
