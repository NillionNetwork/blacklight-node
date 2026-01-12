use alloy::primitives::{utils::format_units, Address, U256};
use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use niluv::config::{MonitorCliArgs, MonitorConfig};
use niluv::contract_client::{ContractConfig, NilUVClient};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, TableState, Wrap},
    Frame, Terminal,
};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

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
    assigned_nodes: HashSet<String>,
    responded_nodes: HashSet<String>,
    timestamp: SystemTime,
}

#[derive(Debug, Clone)]
struct NodeInfo {
    address: Address,
    stake: U256,
    is_registered: bool,
    eth_balance: U256,
}

#[derive(Debug, Clone)]
struct TokenHolder {
    address: Address,
    balance: U256,
}

/// Validation status for an HTX - polled directly from contract (not streaming)
#[derive(Debug, Clone)]
struct HTXValidationStatus {
    htx_id: String,
    expected_validators: Vec<Address>,
    actual_validators: Vec<Address>,
    missing_validators: Vec<Address>,
    is_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StakingInputField {
    PrivateKey,
    TargetAddress,
    Amount,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MintingInputField {
    PrivateKey,
    TargetAddress,
    Amount,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TransferInputField {
    PrivateKey,
    TargetAddress,
    Amount,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Nodes,
    HTXTracking,
    ValidationStatus,
    Staking,
    Minting,
    TokenHolders,
    TransferETH,
}

impl Tab {
    fn title(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Nodes => "Nodes",
            Tab::HTXTracking => "HTX Tracking",
            Tab::ValidationStatus => "Validation Status",
            Tab::Staking => "Staking",
            Tab::Minting => "Mint Tokens",
            Tab::TokenHolders => "Token Holders",
            Tab::TransferETH => "Transfer ETH",
        }
    }

    fn next(&self) -> Self {
        match self {
            Tab::Overview => Tab::Nodes,
            Tab::Nodes => Tab::HTXTracking,
            Tab::HTXTracking => Tab::ValidationStatus,
            Tab::ValidationStatus => Tab::Staking,
            Tab::Staking => Tab::Minting,
            Tab::Minting => Tab::TokenHolders,
            Tab::TokenHolders => Tab::TransferETH,
            Tab::TransferETH => Tab::Overview,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Tab::Overview => Tab::TransferETH,
            Tab::Nodes => Tab::Overview,
            Tab::HTXTracking => Tab::Nodes,
            Tab::ValidationStatus => Tab::HTXTracking,
            Tab::Staking => Tab::ValidationStatus,
            Tab::Minting => Tab::Staking,
            Tab::TokenHolders => Tab::Minting,
            Tab::TransferETH => Tab::TokenHolders,
        }
    }
}

struct MonitorState {
    current_tab: Tab,
    should_quit: bool,
    last_update: std::time::Instant,
    node_count: usize,
    nodes: Vec<NodeInfo>,
    htx_tracking: HashMap<String, HTXTransaction>,
    status_message: String,
    selected_node_index: Option<usize>,
    htx_tracking_state: TableState,
    rpc_url: String,
    manager_contract_address: Address,
    staking_contract_address: Address,
    token_contract_address: Address,
    public_key: String,
    token_balance: U256,
    eth_balance: U256,
    // Staking Tab State
    staking_private_key: String,
    staking_target_address: String,
    staking_amount: String,
    staking_active_input: StakingInputField,
    // Minting Tab State
    minting_private_key: String,
    minting_target_address: String,
    minting_amount: String,
    minting_active_input: MintingInputField,
    // Token Holders Tab State
    token_holders: Vec<TokenHolder>,
    token_holder_addresses: HashSet<Address>, // Track all addresses that have interacted with the token
    token_holders_state: ratatui::widgets::ListState,
    // Transfer ETH Tab State
    transfer_private_key: String,
    transfer_target_address: String,
    transfer_amount: String,
    transfer_active_input: TransferInputField,
    // Validation Status Tab State (polled, not streamed)
    validation_statuses: Vec<HTXValidationStatus>,
    validation_status_state: ratatui::widgets::ListState,
    validation_last_refresh: std::time::Instant,
    validation_is_loading: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = MonitorCliArgs::parse();
    let config = MonitorConfig::load(cli_args)?;

    // Store values before they're moved
    let rpc_url = config.rpc_url.clone();
    let manager_contract_address = config.manager_contract_address;
    let staking_contract_address = config.staking_contract_address;
    let token_contract_address = config.token_contract_address;

    let contract_config = ContractConfig::new(
        config.rpc_url,
        config.manager_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );
    let client = NilUVClient::new(contract_config, config.private_key).await?;

    // Initial data fetch for node count and list
    let node_count = client.manager.node_count().await?.to::<usize>();

    // Get all registered nodes from manager
    let registered_nodes = client.manager.get_nodes().await?;
    let registered_set: HashSet<Address> = registered_nodes.into_iter().collect();

    // Get all operators with stake (efficient: queries contract state directly)
    let mut nodes = Vec::new();
    if let Ok(staked_operators) = client.staking.get_operators_with_stake().await {
        for addr in staked_operators {
            if let Ok(stake) = client.staking.stake_of(addr).await {
                let is_registered = registered_set.contains(&addr);
                let eth_balance = client.get_balance_of(addr).await.unwrap_or(U256::ZERO);
                nodes.push(NodeInfo {
                    address: addr,
                    stake,
                    is_registered,
                    eth_balance,
                });
            }
        }
    }

    // Sort by stake descending
    nodes.sort_by(|a, b| b.stake.cmp(&a.stake));

    // Fetch initial token balance and ETH balance
    let signer_address = client.signer_address();
    let token_balance = client
        .token
        .balance_of(signer_address)
        .await
        .unwrap_or(U256::ZERO);
    let eth_balance = client.get_balance().await.unwrap_or(U256::ZERO);

    // Build token holders list from system addresses (nodes and operators)
    // Collect all relevant addresses: registered nodes + all staked operators
    let mut token_holder_addresses = HashSet::new();

    // Add all registered nodes
    if let Ok(registered_nodes) = client.manager.get_nodes().await {
        for addr in registered_nodes {
            token_holder_addresses.insert(addr);
        }
    }

    // Add all operators with stake (efficient: direct contract query)
    if let Ok(staked_operators) = client.staking.get_operators_with_stake().await {
        for operator in staked_operators {
            token_holder_addresses.insert(operator);
        }
    }

    // Fetch current token balances for all system addresses
    let mut token_holders = Vec::new();
    for addr in &token_holder_addresses {
        if let Ok(balance) = client.token.balance_of(*addr).await {
            if balance > U256::ZERO {
                token_holders.push(TokenHolder {
                    address: *addr,
                    balance,
                });
            }
        }
    }
    // Sort by balance descending
    token_holders.sort_by(|a, b| b.balance.cmp(&a.balance));

    // Build HTX tracking map from historical events
    let mut htx_tracking: HashMap<String, HTXTransaction> = HashMap::new();
    let mut total_events = 0;

    // Add submitted events
    if config.all_htxs {
        if let Ok(events) = client.manager.get_htx_submitted_events().await {
            total_events += events.len();
            for e in events {
                let htx_id = bytes_to_hex(e.heartbeatKey.as_slice());
                let sender = format!("{:?}", e.submitter);
                htx_tracking.insert(
                    htx_id.clone(),
                    HTXTransaction {
                        htx_id,
                        submitted_sender: Some(sender),
                        assigned_nodes: HashSet::new(),
                        responded_nodes: HashSet::new(),
                        timestamp: SystemTime::now(),
                    },
                );
            }
        }

        // Add assigned events
        if let Ok(events) = client.manager.get_htx_assigned_events().await {
            total_events += events.len();
            for e in events {
                let htx_id = bytes_to_hex(e.heartbeatKey.as_slice());
                let nodes: Vec<_> = e.members.iter().map(|n| format!("{n:?}")).collect();
                htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| {
                        tx.assigned_nodes.extend(nodes.iter().cloned());
                    })
                    .or_insert_with(|| {
                        let mut assigned = HashSet::new();
                        assigned.extend(nodes);
                        HTXTransaction {
                            htx_id,
                            submitted_sender: None,
                            assigned_nodes: assigned,
                            responded_nodes: HashSet::new(),
                            timestamp: SystemTime::now(),
                        }
                    });
            }
        }

        // Add responded events
        if let Ok(events) = client.manager.get_htx_responded_events().await {
            total_events += events.len();
            for e in events {
                let htx_id = bytes_to_hex(e.heartbeatKey.as_slice());
                let node = format!("{:?}", e.operator);
                htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| {
                        tx.responded_nodes.insert(node.clone());
                    })
                    .or_insert_with(|| {
                        let mut responded = HashSet::new();
                        responded.insert(node.clone());
                        HTXTransaction {
                            htx_id,
                            submitted_sender: None,
                            assigned_nodes: HashSet::new(),
                            responded_nodes: responded,
                            timestamp: SystemTime::now(),
                        }
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
        htx_tracking,
        status_message: format!(
            "Press 'q' to quit, 'r' to refresh, Tab/Shift+Tab to navigate - Live WebSocket mode (Loaded {} historical events)",
            total_events
        ),
        selected_node_index: None,
        htx_tracking_state: TableState::default(),
        rpc_url,
        manager_contract_address,
        staking_contract_address,
        token_contract_address,
        public_key: format!("{:?}", client.signer_address()),
        token_balance,
        eth_balance,
        staking_private_key: String::new(),
        staking_target_address: String::new(),
        staking_amount: String::new(),
        staking_active_input: StakingInputField::None,
        minting_private_key: String::new(),
        minting_target_address: String::new(),
        minting_amount: String::new(),
        minting_active_input: MintingInputField::None,
        token_holders,
        token_holder_addresses,
        token_holders_state: ratatui::widgets::ListState::default(),
        transfer_private_key: String::new(),
        transfer_target_address: String::new(),
        transfer_amount: String::new(),
        transfer_active_input: TransferInputField::None,
        validation_statuses: Vec::new(),
        validation_status_state: ratatui::widgets::ListState::default(),
        validation_last_refresh: std::time::Instant::now(),
        validation_is_loading: false,
    };

    run_monitor(client, initial_state).await
}

async fn run_monitor(client: NilUVClient, initial_state: MonitorState) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Wrap state in Arc<Mutex> for thread-safe sharing
    let state = Arc::new(Mutex::new(initial_state));
    let client_arc = Arc::new(client);

    // Spawn WebSocket event listeners with reconnection logic
    let state_clone = state.clone();
    let client_clone = client_arc.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = listen_htx_submitted(client_clone.clone(), state_clone.clone()).await {
                eprintln!("HTX submitted listener error: {}, reconnecting...", e);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let state_clone = state.clone();
    let client_clone = client_arc.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = listen_htx_assigned(client_clone.clone(), state_clone.clone()).await {
                eprintln!("HTX assigned listener error: {}, reconnecting...", e);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let state_clone = state.clone();
    let client_clone = client_arc.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = listen_htx_responded(client_clone.clone(), state_clone.clone()).await {
                eprintln!("HTX responded listener error: {}, reconnecting...", e);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let state_clone = state.clone();
    let client_clone = client_arc.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = listen_token_transfers(client_clone.clone(), state_clone.clone()).await
            {
                eprintln!("Token transfer listener error: {}, reconnecting...", e);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let result = run_monitor_loop(&mut terminal, client_arc, state).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_monitor_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: Arc<NilUVClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    loop {
        // Draw UI with current state
        {
            let mut state_guard = state.lock().await;
            terminal.draw(|f| {
                ui(f, &mut state_guard);
            })?;
        }

        // Handle input with timeout
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Paste(pasted_text) => {
                    // Handle paste events (CMD+V)
                    let mut state_guard = state.lock().await;

                    // Add pasted text to the active input field
                    if state_guard.current_tab == Tab::Staking
                        && state_guard.staking_active_input != StakingInputField::None
                    {
                        match state_guard.staking_active_input {
                            StakingInputField::PrivateKey => {
                                state_guard.staking_private_key.push_str(&pasted_text)
                            }
                            StakingInputField::TargetAddress => {
                                state_guard.staking_target_address.push_str(&pasted_text)
                            }
                            StakingInputField::Amount => {
                                state_guard.staking_amount.push_str(&pasted_text)
                            }
                            _ => {}
                        }
                    } else if state_guard.current_tab == Tab::Minting
                        && state_guard.minting_active_input != MintingInputField::None
                    {
                        match state_guard.minting_active_input {
                            MintingInputField::PrivateKey => {
                                state_guard.minting_private_key.push_str(&pasted_text)
                            }
                            MintingInputField::TargetAddress => {
                                state_guard.minting_target_address.push_str(&pasted_text)
                            }
                            MintingInputField::Amount => {
                                state_guard.minting_amount.push_str(&pasted_text)
                            }
                            _ => {}
                        }
                    } else if state_guard.current_tab == Tab::TransferETH
                        && state_guard.transfer_active_input != TransferInputField::None
                    {
                        match state_guard.transfer_active_input {
                            TransferInputField::PrivateKey => {
                                state_guard.transfer_private_key.push_str(&pasted_text)
                            }
                            TransferInputField::TargetAddress => {
                                state_guard.transfer_target_address.push_str(&pasted_text)
                            }
                            TransferInputField::Amount => {
                                state_guard.transfer_amount.push_str(&pasted_text)
                            }
                            _ => {}
                        }
                    }
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let mut state_guard = state.lock().await;

                    // Normal key handling
                    match key.code {
                        KeyCode::Char('q') => {
                            state_guard.should_quit = true;
                            break;
                        }
                        KeyCode::Char('r') => {
                            // Manual refresh - update node list and balance
                            state_guard.status_message = "Refreshing...".to_string();
                            drop(state_guard);

                            // Get all registered nodes from manager
                            let registered_result = client.manager.get_nodes().await;
                            // Get all operators with stake (efficient: direct contract query)
                            let staked_operators_result =
                                client.staking.get_operators_with_stake().await;

                            match (registered_result, staked_operators_result) {
                                (Ok(registered_nodes), Ok(staked_operators)) => {
                                    let registered_set: HashSet<Address> =
                                        registered_nodes.into_iter().collect();

                                    // Fetch current stake for all operators with stake
                                    let mut nodes = Vec::new();
                                    for addr in staked_operators {
                                        if let Ok(stake) = client.staking.stake_of(addr).await {
                                            let is_registered = registered_set.contains(&addr);
                                            let eth_balance = client
                                                .get_balance_of(addr)
                                                .await
                                                .unwrap_or(U256::ZERO);
                                            nodes.push(NodeInfo {
                                                address: addr,
                                                stake,
                                                is_registered,
                                                eth_balance,
                                            });
                                        }
                                    }

                                    // Sort by stake descending
                                    nodes.sort_by(|a, b| b.stake.cmp(&a.stake));

                                    // Fetch updated balances
                                    let signer_address = client.signer_address();
                                    let token_balance = client
                                        .token
                                        .balance_of(signer_address)
                                        .await
                                        .unwrap_or(U256::ZERO);
                                    let eth_balance =
                                        client.get_balance().await.unwrap_or(U256::ZERO);

                                    // Fetch updated token holder balances from system addresses
                                    // Collect addresses from registered nodes and staked operators
                                    let mut token_holder_addresses = HashSet::new();

                                    // Add all registered nodes
                                    if let Ok(registered_nodes) = client.manager.get_nodes().await {
                                        for addr in registered_nodes {
                                            token_holder_addresses.insert(addr);
                                        }
                                    }

                                    // Add all operators with stake (efficient: direct contract query)
                                    if let Ok(staked_operators) =
                                        client.staking.get_operators_with_stake().await
                                    {
                                        for operator in staked_operators {
                                            token_holder_addresses.insert(operator);
                                        }
                                    }

                                    let mut token_holders = Vec::new();
                                    for addr in token_holder_addresses.iter() {
                                        if let Ok(balance) = client.token.balance_of(*addr).await {
                                            if balance > U256::ZERO {
                                                token_holders.push(TokenHolder {
                                                    address: *addr,
                                                    balance,
                                                });
                                            }
                                        }
                                    }
                                    token_holders.sort_by(|a, b| b.balance.cmp(&a.balance));

                                    let mut state_guard = state.lock().await;
                                    state_guard.nodes = nodes;
                                    state_guard.node_count = registered_set.len();
                                    state_guard.token_balance = token_balance;
                                    state_guard.eth_balance = eth_balance;
                                    state_guard.token_holders = token_holders;
                                    state_guard.status_message = "Refreshed!".to_string();
                                }
                                (Err(e), _) | (_, Err(e)) => {
                                    let mut state_guard = state.lock().await;
                                    state_guard.status_message = format!("Error: {}", e);
                                }
                            }
                            continue;
                        }
                        KeyCode::Char('v') => {
                            // Load validation status from contract (polling, not streaming)
                            state_guard.validation_is_loading = true;
                            state_guard.status_message =
                                "Loading validation status from contract...".to_string();
                            drop(state_guard);

                            let result = load_validation_status(&client).await;

                            let mut state_guard = state.lock().await;
                            state_guard.validation_is_loading = false;
                            state_guard.validation_last_refresh = std::time::Instant::now();

                            match result {
                                Ok(statuses) => {
                                    let complete = statuses.iter().filter(|s| s.is_complete).count();
                                    let total = statuses.len();
                                    state_guard.validation_statuses = statuses;
                                    state_guard.validation_status_state =
                                        ratatui::widgets::ListState::default();
                                    state_guard.status_message = format!(
                                        "Loaded {} HTXs ({} complete, {} incomplete)",
                                        total,
                                        complete,
                                        total - complete
                                    );
                                }
                                Err(e) => {
                                    state_guard.status_message =
                                        format!("Error loading validation status: {}", e);
                                }
                            }
                            continue;
                        }
                        KeyCode::Tab => {
                            state_guard.current_tab = state_guard.current_tab.next();
                            state_guard.selected_node_index = None;
                            if state_guard.current_tab == Tab::Staking
                                && state_guard.staking_active_input == StakingInputField::None
                            {
                                state_guard.staking_active_input = StakingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to stake".to_string();
                            }
                            if state_guard.current_tab == Tab::Minting
                                && state_guard.minting_active_input == MintingInputField::None
                            {
                                state_guard.minting_active_input = MintingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to mint".to_string();
                            }
                            if state_guard.current_tab == Tab::TransferETH
                                && state_guard.transfer_active_input == TransferInputField::None
                            {
                                state_guard.transfer_active_input =
                                    TransferInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to transfer ETH".to_string();
                            }
                        }
                        KeyCode::BackTab => {
                            state_guard.current_tab = state_guard.current_tab.prev();
                            state_guard.selected_node_index = None;
                            if state_guard.current_tab == Tab::Staking
                                && state_guard.staking_active_input == StakingInputField::None
                            {
                                state_guard.staking_active_input = StakingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to stake".to_string();
                            }
                            if state_guard.current_tab == Tab::Minting
                                && state_guard.minting_active_input == MintingInputField::None
                            {
                                state_guard.minting_active_input = MintingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to mint".to_string();
                            }
                            if state_guard.current_tab == Tab::TransferETH
                                && state_guard.transfer_active_input == TransferInputField::None
                            {
                                state_guard.transfer_active_input =
                                    TransferInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to transfer ETH".to_string();
                            }
                        }
                        KeyCode::Right => {
                            state_guard.current_tab = state_guard.current_tab.next();
                            state_guard.selected_node_index = None;
                            if state_guard.current_tab == Tab::Staking
                                && state_guard.staking_active_input == StakingInputField::None
                            {
                                state_guard.staking_active_input = StakingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to stake".to_string();
                            }
                            if state_guard.current_tab == Tab::Minting
                                && state_guard.minting_active_input == MintingInputField::None
                            {
                                state_guard.minting_active_input = MintingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to mint".to_string();
                            }
                            if state_guard.current_tab == Tab::TransferETH
                                && state_guard.transfer_active_input == TransferInputField::None
                            {
                                state_guard.transfer_active_input =
                                    TransferInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to transfer ETH".to_string();
                            }
                        }
                        KeyCode::Left => {
                            state_guard.current_tab = state_guard.current_tab.prev();
                            state_guard.selected_node_index = None;
                            if state_guard.current_tab == Tab::Staking
                                && state_guard.staking_active_input == StakingInputField::None
                            {
                                state_guard.staking_active_input = StakingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to stake".to_string();
                            }
                            if state_guard.current_tab == Tab::Minting
                                && state_guard.minting_active_input == MintingInputField::None
                            {
                                state_guard.minting_active_input = MintingInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to mint".to_string();
                            }
                            if state_guard.current_tab == Tab::TransferETH
                                && state_guard.transfer_active_input == TransferInputField::None
                            {
                                state_guard.transfer_active_input =
                                    TransferInputField::TargetAddress;
                                state_guard.status_message =
                                    "Fill in the form and press Enter to transfer ETH".to_string();
                            }
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
                            Tab::Staking => {
                                state_guard.staking_active_input = match state_guard
                                    .staking_active_input
                                {
                                    StakingInputField::None | StakingInputField::PrivateKey => {
                                        StakingInputField::TargetAddress
                                    }
                                    StakingInputField::TargetAddress => StakingInputField::Amount,
                                    StakingInputField::Amount => StakingInputField::Amount,
                                };
                            }
                            Tab::Minting => {
                                state_guard.minting_active_input = match state_guard
                                    .minting_active_input
                                {
                                    MintingInputField::None | MintingInputField::PrivateKey => {
                                        MintingInputField::TargetAddress
                                    }
                                    MintingInputField::TargetAddress => MintingInputField::Amount,
                                    MintingInputField::Amount => MintingInputField::Amount,
                                };
                            }
                            Tab::TransferETH => {
                                state_guard.transfer_active_input = match state_guard
                                    .transfer_active_input
                                {
                                    TransferInputField::None | TransferInputField::PrivateKey => {
                                        TransferInputField::TargetAddress
                                    }
                                    TransferInputField::TargetAddress => TransferInputField::Amount,
                                    TransferInputField::Amount => TransferInputField::Amount,
                                };
                            }
                            Tab::TokenHolders if !state_guard.token_holders.is_empty() => {
                                let i = state_guard.token_holders_state.selected().unwrap_or(0);
                                let next = (i + 1).min(state_guard.token_holders.len() - 1);
                                state_guard.token_holders_state.select(Some(next));
                            }
                            Tab::ValidationStatus
                                if !state_guard.validation_statuses.is_empty() =>
                            {
                                let i =
                                    state_guard.validation_status_state.selected().unwrap_or(0);
                                let next =
                                    (i + 1).min(state_guard.validation_statuses.len() - 1);
                                state_guard.validation_status_state.select(Some(next));
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
                            Tab::Staking => {
                                state_guard.staking_active_input = match state_guard
                                    .staking_active_input
                                {
                                    StakingInputField::None | StakingInputField::Amount => {
                                        StakingInputField::TargetAddress
                                    }
                                    StakingInputField::TargetAddress => {
                                        StakingInputField::PrivateKey
                                    }
                                    StakingInputField::PrivateKey => StakingInputField::PrivateKey,
                                };
                            }
                            Tab::Minting => {
                                state_guard.minting_active_input = match state_guard
                                    .minting_active_input
                                {
                                    MintingInputField::None | MintingInputField::Amount => {
                                        MintingInputField::TargetAddress
                                    }
                                    MintingInputField::TargetAddress => {
                                        MintingInputField::PrivateKey
                                    }
                                    MintingInputField::PrivateKey => MintingInputField::PrivateKey,
                                };
                            }
                            Tab::TransferETH => {
                                state_guard.transfer_active_input =
                                    match state_guard.transfer_active_input {
                                        TransferInputField::None | TransferInputField::Amount => {
                                            TransferInputField::TargetAddress
                                        }
                                        TransferInputField::TargetAddress => {
                                            TransferInputField::PrivateKey
                                        }
                                        TransferInputField::PrivateKey => {
                                            TransferInputField::PrivateKey
                                        }
                                    };
                            }
                            Tab::TokenHolders if !state_guard.token_holders.is_empty() => {
                                let i = state_guard.token_holders_state.selected().unwrap_or(0);
                                state_guard
                                    .token_holders_state
                                    .select(Some(i.saturating_sub(1)));
                            }
                            Tab::ValidationStatus
                                if !state_guard.validation_statuses.is_empty() =>
                            {
                                let i =
                                    state_guard.validation_status_state.selected().unwrap_or(0);
                                state_guard
                                    .validation_status_state
                                    .select(Some(i.saturating_sub(1)));
                            }
                            _ => {}
                        },
                        KeyCode::Char('d') | KeyCode::Char('D') => {
                            // Check if we're actively typing in an input field (Staking or Minting tabs)
                            let is_typing_staking = state_guard.current_tab == Tab::Staking
                                && state_guard.staking_active_input != StakingInputField::None;
                            let is_typing_minting = state_guard.current_tab == Tab::Minting
                                && state_guard.minting_active_input != MintingInputField::None;

                            if is_typing_staking {
                                // Add 'd' to the active staking input field
                                match state_guard.staking_active_input {
                                    StakingInputField::PrivateKey => {
                                        state_guard.staking_private_key.push('d')
                                    }
                                    StakingInputField::TargetAddress => {
                                        state_guard.staking_target_address.push('d')
                                    }
                                    StakingInputField::Amount => {
                                        state_guard.staking_amount.push('d')
                                    }
                                    _ => {}
                                }
                            } else if is_typing_minting {
                                // Add 'd' to the active minting input field
                                match state_guard.minting_active_input {
                                    MintingInputField::PrivateKey => {
                                        state_guard.minting_private_key.push('d')
                                    }
                                    MintingInputField::TargetAddress => {
                                        state_guard.minting_target_address.push('d')
                                    }
                                    MintingInputField::Amount => {
                                        state_guard.minting_amount.push('d')
                                    }
                                    _ => {}
                                }
                            }
                        }
                        // Staking Tab Input Handling
                        code if state_guard.current_tab == Tab::Staking => {
                            match code {
                                KeyCode::Enter => {
                                    // Submit Staking Transaction
                                    let private_key =
                                        state_guard.staking_private_key.trim().to_string();
                                    let target_addr_str =
                                        state_guard.staking_target_address.trim().to_string();
                                    let amount_str = state_guard.staking_amount.trim().to_string();

                                    // Validate inputs
                                    if target_addr_str.is_empty() {
                                        state_guard.status_message =
                                            "Error: Target address is required".to_string();
                                    } else if amount_str.is_empty() {
                                        state_guard.status_message =
                                            "Error: Amount is required".to_string();
                                    } else {
                                        // Validate amount first
                                        let amount_eth = match amount_str.parse::<f64>() {
                                            Ok(a) if a > 0.0 => a,
                                            Ok(_) => {
                                                state_guard.status_message =
                                                    "Error: Amount must be greater than 0"
                                                        .to_string();
                                                continue;
                                            }
                                            Err(_) => {
                                                state_guard.status_message = format!(
                                                    "Error: Invalid amount format: {}",
                                                    amount_str
                                                )
                                                .to_string();
                                                continue;
                                            }
                                        };

                                        // Validate address (show length for debugging)
                                        let target_addr = match target_addr_str.parse::<Address>() {
                                            Ok(addr) => addr,
                                            Err(e) => {
                                                state_guard.status_message = format!(
                                                    "Error: Invalid address '{}' (len: {}) - {}",
                                                    target_addr_str,
                                                    target_addr_str.len(),
                                                    e
                                                );
                                                continue;
                                            }
                                        };

                                        state_guard.status_message =
                                            "Approving tokens and submitting stake...".to_string();

                                        // Clone necessary data to drop lock
                                        let rpc_url = state_guard.rpc_url.clone();
                                        let manager_addr = state_guard.manager_contract_address;
                                        let staking_addr = state_guard.staking_contract_address;
                                        let token_addr = state_guard.token_contract_address;

                                        let use_custom_key = !private_key.is_empty();

                                        drop(state_guard); // Drop lock

                                        let result = async {
                                            let (staking_client, token_client) = if use_custom_key {
                                                // Create a new client with the provided private key
                                                let config = ContractConfig::new(
                                                    rpc_url,
                                                    manager_addr,
                                                    staking_addr,
                                                    token_addr,
                                                );

                                                let new_client =
                                                    NilUVClient::new(config, private_key).await?;
                                                (
                                                    new_client.staking.clone(),
                                                    new_client.token.clone(),
                                                )
                                            } else {
                                                // Use the existing client's staking and token contracts
                                                (client.staking.clone(), client.token.clone())
                                            };
                                            let amount_wei =
                                                U256::from((amount_eth * 1e6) as u128);
                                            // First, approve the staking contract to spend tokens
                                            token_client.approve(staking_addr, amount_wei).await?;

                                            // Then stake
                                            staking_client.stake_to(target_addr, amount_wei).await
                                        }
                                        .await;

                                        match result {
                                            Ok(tx) => {
                                                let mut state_guard = state.lock().await;
                                                state_guard.status_message =
                                                    format!("Stake submitted! TX: {:?}", tx);
                                                state_guard.staking_amount = String::new();
                                                // Clear amount
                                            }
                                            Err(e) => {
                                                let mut state_guard = state.lock().await;
                                                state_guard.status_message =
                                                    format!("Error staking: {}", e);
                                            }
                                        }
                                        continue;
                                    }
                                }
                                KeyCode::Char(c) => match state_guard.staking_active_input {
                                    StakingInputField::PrivateKey => {
                                        state_guard.staking_private_key.push(c)
                                    }
                                    StakingInputField::TargetAddress => {
                                        state_guard.staking_target_address.push(c)
                                    }
                                    StakingInputField::Amount => state_guard.staking_amount.push(c),
                                    _ => {}
                                },
                                KeyCode::Backspace => match state_guard.staking_active_input {
                                    StakingInputField::PrivateKey => {
                                        state_guard.staking_private_key.pop();
                                    }
                                    StakingInputField::TargetAddress => {
                                        state_guard.staking_target_address.pop();
                                    }
                                    StakingInputField::Amount => {
                                        state_guard.staking_amount.pop();
                                    }
                                    _ => {}
                                },
                                _ => {}
                            }
                        }
                        // Minting Tab Input Handling
                        code if state_guard.current_tab == Tab::Minting => {
                            match code {
                                KeyCode::Enter => {
                                    // Submit Minting Transaction
                                    let private_key =
                                        state_guard.minting_private_key.trim().to_string();
                                    let target_addr_str =
                                        state_guard.minting_target_address.trim().to_string();
                                    let amount_str = state_guard.minting_amount.trim().to_string();

                                    // Validate inputs
                                    if target_addr_str.is_empty() {
                                        state_guard.status_message =
                                            "Error: Target address is required".to_string();
                                    } else if amount_str.is_empty() {
                                        state_guard.status_message =
                                            "Error: Amount is required".to_string();
                                    } else {
                                        // Validate amount first
                                        let amount_eth = match amount_str.parse::<f64>() {
                                            Ok(a) if a > 0.0 => a,
                                            Ok(_) => {
                                                state_guard.status_message =
                                                    "Error: Amount must be greater than 0"
                                                        .to_string();
                                                continue;
                                            }
                                            Err(_) => {
                                                state_guard.status_message = format!(
                                                    "Error: Invalid amount format: {}",
                                                    amount_str
                                                )
                                                .to_string();
                                                continue;
                                            }
                                        };

                                        // Validate address (show length for debugging)
                                        let target_addr = match target_addr_str.parse::<Address>() {
                                            Ok(addr) => addr,
                                            Err(e) => {
                                                state_guard.status_message = format!(
                                                    "Error: Invalid address '{}' (len: {}) - {}",
                                                    target_addr_str,
                                                    target_addr_str.len(),
                                                    e
                                                );
                                                continue;
                                            }
                                        };

                                        state_guard.status_message =
                                            "Minting tokens...".to_string();

                                        // Clone necessary data to drop lock
                                        let rpc_url = state_guard.rpc_url.clone();
                                        let manager_addr = state_guard.manager_contract_address;
                                        let staking_addr = state_guard.staking_contract_address;
                                        let token_addr = state_guard.token_contract_address;

                                        let use_custom_key = !private_key.is_empty();

                                        drop(state_guard); // Drop lock

                                        let result = async {
                                            let token_client = if use_custom_key {
                                                // Create a new client with the provided private key
                                                let config = ContractConfig::new(
                                                    rpc_url,
                                                    manager_addr,
                                                    staking_addr,
                                                    token_addr,
                                                );

                                                let new_client =
                                                    NilUVClient::new(config, private_key).await?;
                                                new_client.token
                                            } else {
                                                // Use the existing client's token contract
                                                client.token.clone()
                                            };
                                            let amount_wei =
                                                U256::from((amount_eth * 1e6) as u128);

                                            token_client.mint(target_addr, amount_wei).await
                                        }
                                        .await;

                                        match result {
                                            Ok(tx) => {
                                                let mut state_guard = state.lock().await;
                                                state_guard.status_message =
                                                    format!("Tokens minted! TX: {:?}", tx);
                                                state_guard.minting_amount = String::new();
                                                // Clear amount
                                            }
                                            Err(e) => {
                                                let mut state_guard = state.lock().await;
                                                state_guard.status_message =
                                                    format!("Error minting: {}", e);
                                            }
                                        }
                                        continue;
                                    }
                                }
                                KeyCode::Char(c) => match state_guard.minting_active_input {
                                    MintingInputField::PrivateKey => {
                                        state_guard.minting_private_key.push(c)
                                    }
                                    MintingInputField::TargetAddress => {
                                        state_guard.minting_target_address.push(c)
                                    }
                                    MintingInputField::Amount => state_guard.minting_amount.push(c),
                                    _ => {}
                                },
                                KeyCode::Backspace => match state_guard.minting_active_input {
                                    MintingInputField::PrivateKey => {
                                        state_guard.minting_private_key.pop();
                                    }
                                    MintingInputField::TargetAddress => {
                                        state_guard.minting_target_address.pop();
                                    }
                                    MintingInputField::Amount => {
                                        state_guard.minting_amount.pop();
                                    }
                                    _ => {}
                                },
                                _ => {}
                            }
                        }
                        // Transfer ETH Tab Input Handling
                        code if state_guard.current_tab == Tab::TransferETH => {
                            match code {
                                KeyCode::Enter => {
                                    // Submit ETH Transfer Transaction
                                    let private_key =
                                        state_guard.transfer_private_key.trim().to_string();
                                    let target_addr_str =
                                        state_guard.transfer_target_address.trim().to_string();
                                    let amount_str = state_guard.transfer_amount.trim().to_string();

                                    // Validate inputs
                                    if target_addr_str.is_empty() {
                                        state_guard.status_message =
                                            "Error: Target address is required".to_string();
                                    } else if amount_str.is_empty() {
                                        state_guard.status_message =
                                            "Error: Amount is required".to_string();
                                    } else {
                                        // Validate amount (ETH amount as decimal string)
                                        let amount_eth: f64 = match amount_str.parse() {
                                            Ok(a) if a > 0.0 => a,
                                            Ok(_) => {
                                                state_guard.status_message =
                                                    "Error: Amount must be greater than 0"
                                                        .to_string();
                                                continue;
                                            }
                                            Err(_) => {
                                                state_guard.status_message = format!(
                                                    "Error: Invalid amount format: {}",
                                                    amount_str
                                                )
                                                .to_string();
                                                continue;
                                            }
                                        };

                                        // Convert ETH to Wei (1 ETH = 10^18 Wei)
                                        let amount_wei = U256::from((amount_eth * 1e18) as u128);

                                        // Validate address
                                        let target_addr = match target_addr_str.parse::<Address>() {
                                            Ok(addr) => addr,
                                            Err(e) => {
                                                state_guard.status_message = format!(
                                                    "Error: Invalid address '{}' (len: {}) - {}",
                                                    target_addr_str,
                                                    target_addr_str.len(),
                                                    e
                                                );
                                                continue;
                                            }
                                        };

                                        state_guard.status_message =
                                            "Transferring ETH...".to_string();

                                        // Clone necessary data to drop lock
                                        let rpc_url = state_guard.rpc_url.clone();
                                        let manager_addr = state_guard.manager_contract_address;
                                        let staking_addr = state_guard.staking_contract_address;
                                        let token_addr = state_guard.token_contract_address;

                                        let use_custom_key = !private_key.is_empty();

                                        drop(state_guard); // Drop lock

                                        let result = async {
                                            if use_custom_key {
                                                // Create a new client with the provided private key
                                                let config = ContractConfig::new(
                                                    rpc_url,
                                                    manager_addr,
                                                    staking_addr,
                                                    token_addr,
                                                );

                                                let new_client =
                                                    NilUVClient::new(config, private_key).await?;
                                                new_client.send_eth(target_addr, amount_wei).await
                                            } else {
                                                // Use the existing client
                                                client.send_eth(target_addr, amount_wei).await
                                            }
                                        }
                                        .await;

                                        match result {
                                            Ok(tx) => {
                                                let mut state_guard = state.lock().await;
                                                state_guard.status_message =
                                                    format!("ETH transferred! TX: {:?}", tx);
                                                state_guard.transfer_amount = String::new();
                                                // Clear amount
                                            }
                                            Err(e) => {
                                                let mut state_guard = state.lock().await;
                                                state_guard.status_message =
                                                    format!("Error transferring: {}", e);
                                            }
                                        }
                                        continue;
                                    }
                                }
                                KeyCode::Char(c) => match state_guard.transfer_active_input {
                                    TransferInputField::PrivateKey => {
                                        state_guard.transfer_private_key.push(c)
                                    }
                                    TransferInputField::TargetAddress => {
                                        state_guard.transfer_target_address.push(c)
                                    }
                                    TransferInputField::Amount => {
                                        state_guard.transfer_amount.push(c)
                                    }
                                    _ => {}
                                },
                                KeyCode::Backspace => match state_guard.transfer_active_input {
                                    TransferInputField::PrivateKey => {
                                        state_guard.transfer_private_key.pop();
                                    }
                                    TransferInputField::TargetAddress => {
                                        state_guard.transfer_target_address.pop();
                                    }
                                    TransferInputField::Amount => {
                                        state_guard.transfer_amount.pop();
                                    }
                                    _ => {}
                                },
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                _ => {} // Ignore other events
            }
        }
    }

    Ok(())
}

// WebSocket event listener for HTX submitted events (parallel processing)
async fn listen_htx_submitted(
    client: Arc<NilUVClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    let manager = Arc::new(client.manager.clone());
    manager
        .listen_htx_submitted_events(move |event| {
            let state = state.clone();
            async move {
                let htx_id = bytes_to_hex(event.heartbeatKey.as_slice());
                let sender = format!("{:?}", event.submitter);

                let mut state_guard = state.lock().await;

                // Update tracking map
                state_guard
                    .htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| tx.submitted_sender = Some(sender.clone()))
                    .or_insert(HTXTransaction {
                        htx_id,
                        submitted_sender: Some(sender),
                        assigned_nodes: HashSet::new(),
                        responded_nodes: HashSet::new(),
                        timestamp: SystemTime::now(),
                    });

                state_guard.last_update = std::time::Instant::now();
                Ok(())
            }
        })
        .await
}

// WebSocket event listener for HTX assigned events (parallel processing)
async fn listen_htx_assigned(
    client: Arc<NilUVClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    let manager = Arc::new(client.manager.clone());
    manager
        .listen_htx_assigned_events(move |event| {
            let state = state.clone();
            async move {
                let htx_id = bytes_to_hex(event.heartbeatKey.as_slice());
                let nodes: Vec<_> = event.members.iter().map(|n| format!("{n:?}")).collect();

                let mut state_guard = state.lock().await;

                // Update tracking map
                state_guard
                    .htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| {
                        tx.assigned_nodes.extend(nodes.iter().cloned());
                    })
                    .or_insert_with(|| {
                        let mut assigned = HashSet::new();
                        assigned.extend(nodes);
                        HTXTransaction {
                            htx_id,
                            submitted_sender: None,
                            assigned_nodes: assigned,
                            responded_nodes: HashSet::new(),
                            timestamp: SystemTime::now(),
                        }
                    });

                state_guard.last_update = std::time::Instant::now();
                Ok(())
            }
        })
        .await
}

// WebSocket event listener for HTX responded events (parallel processing)
async fn listen_htx_responded(
    client: Arc<NilUVClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    let manager = Arc::new(client.manager.clone());
    manager
        .listen_htx_responded_events(move |event| {
            let state = state.clone();
            async move {
                let htx_id = bytes_to_hex(event.heartbeatKey.as_slice());
                let node = format!("{:?}", event.operator);

                let mut state_guard = state.lock().await;

                // Update tracking map
                state_guard
                    .htx_tracking
                    .entry(htx_id.clone())
                    .and_modify(|tx| {
                        tx.responded_nodes.insert(node.clone());
                    })
                    .or_insert_with(|| {
                        let mut responded = HashSet::new();
                        responded.insert(node.clone());
                        HTXTransaction {
                            htx_id,
                            submitted_sender: None,
                            assigned_nodes: HashSet::new(),
                            responded_nodes: responded,
                            timestamp: SystemTime::now(),
                        }
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
        Tab::ValidationStatus,
        Tab::Staking,
        Tab::Minting,
        Tab::TokenHolders,
        Tab::TransferETH,
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
            .title("NilUV Contract Monitor")
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
        Tab::ValidationStatus => render_validation_status(f, area, state),
        Tab::Staking => render_staking(f, area, state),
        Tab::Minting => render_minting(f, area, state),
        Tab::TokenHolders => render_token_holders(f, area, state),
        Tab::TransferETH => render_transfer_eth(f, area, state),
    }
}

fn render_overview(f: &mut Frame, area: Rect, state: &MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Connection info
            Constraint::Length(8), // Stats (includes ETH and TEST balances)
            Constraint::Min(5),    // Details
        ])
        .split(area);

    // Connection Info
    let info_text = vec![
        Line::from(vec![
            Span::styled("RPC URL: ", Style::default().fg(Color::Cyan)),
            Span::styled(state.rpc_url.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(
                "Manager Contract Address: ",
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("{:?}", state.manager_contract_address),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Staking Contract Address: ",
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("{:?}", state.staking_contract_address),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Token Contract Address: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{:?}", state.token_contract_address),
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

    // Calculate stats from htx_tracking
    let htx_submitted_count = state
        .htx_tracking
        .values()
        .filter(|tx| tx.submitted_sender.is_some())
        .count();
    let htx_assigned_count = state
        .htx_tracking
        .values()
        .filter(|tx| !tx.assigned_nodes.is_empty())
        .count();
    let htx_responded_count = state
        .htx_tracking
        .values()
        .filter(|tx| !tx.responded_nodes.is_empty())
        .count();

    // Format balances
    let token_balance_formatted =
        format_units(state.token_balance, 6).unwrap_or_else(|_| "0".to_string());
    let eth_balance_formatted =
        format_units(state.eth_balance, 18).unwrap_or_else(|_| "0".to_string());

    // Stats
    let stats_text = vec![
        Line::from(vec![
            Span::styled("ETH Balance: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{} ETH", eth_balance_formatted),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("TEST Balance: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{} TEST", token_balance_formatted),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
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
                htx_submitted_count.to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("HTX Assigned: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                htx_assigned_count.to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("HTX Responded: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                htx_responded_count.to_string(),
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

    // Get most recent HTX transactions
    let mut recent_htxs: Vec<&HTXTransaction> = state.htx_tracking.values().collect();
    recent_htxs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    if !recent_htxs.is_empty() {
        activity_lines.push(Line::from(vec![Span::styled(
            "Latest HTX Transactions:",
            Style::default().fg(Color::Yellow),
        )]));

        for tx in recent_htxs.iter().take(5) {
            let htx_short = format_short_hex(&tx.htx_id);
            let status = if !tx.responded_nodes.is_empty() {
                format!(
                    "✓ Responded ({}/{})",
                    tx.responded_nodes.len(),
                    tx.assigned_nodes.len()
                )
            } else if !tx.assigned_nodes.is_empty() {
                format!("⧗ Assigned ({})", tx.assigned_nodes.len())
            } else if tx.submitted_sender.is_some() {
                "○ Submitted".to_string()
            } else {
                "? Unknown".to_string()
            };

            activity_lines.push(Line::from(vec![
                Span::raw("  • "),
                Span::raw(htx_short),
                Span::raw(" - "),
                Span::raw(status),
            ]));
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
    let registered_count = state.nodes.iter().filter(|n| n.is_registered).count();
    let total_count = state.nodes.len();

    let items: Vec<ListItem> = state
        .nodes
        .iter()
        .enumerate()
        .map(|(idx, node_info)| {
            let stake_formatted =
                format_units(node_info.stake, 6).unwrap_or_else(|_| "0".to_string());
            let eth_balance_formatted =
                format_units(node_info.eth_balance, 18).unwrap_or_else(|_| "0".to_string());
            let status = if node_info.is_registered {
                "✓ Registered"
            } else {
                "✗ Not Registered"
            };
            let status_color = if node_info.is_registered {
                Color::Green
            } else {
                Color::Red
            };

            let content = vec![
                Span::raw(format!("{}. ", idx + 1)),
                Span::styled(
                    status,
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(
                    " | {:?} | Stake: {} TEST | ETH: {}",
                    node_info.address, stake_formatted, eth_balance_formatted
                )),
            ];

            let style = if Some(idx) == state.selected_node_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(content)).style(style)
        })
        .collect();

    let title = format!(
        "Nodes with Stake ({} total, {} registered) - Use ↑↓ to select",
        total_count, registered_count
    );

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

            let assigned_count = tx.assigned_nodes.len();
            let responded_count = tx.responded_nodes.len();

            Row::new(vec![
                row_num.to_string(),
                format_short_hex(&tx.htx_id),
                submitted,
                assigned_count.to_string(),
                responded_count.to_string(),
            ])
            .style(Style::default().fg(Color::White))
        })
        .collect();

    // Create table header
    let header = Row::new(vec![
        "#",
        "HTX ID",
        "Submitted (Sender)",
        "Assigned (Count)",
        "Responded (Count)",
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

fn render_validation_status(f: &mut Frame, area: Rect, state: &mut MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Instructions
            Constraint::Min(0),    // Table
        ])
        .split(area);

    // Instructions/status bar
    let last_refresh_secs = state.validation_last_refresh.elapsed().as_secs();
    let status_text = if state.validation_is_loading {
        "Loading validation data from contract...".to_string()
    } else if state.validation_statuses.is_empty() {
        format!(
            "Press 'v' to load validation status (polled from contract). Last refresh: {}s ago",
            last_refresh_secs
        )
    } else {
        format!(
            "Showing {} HTXs - Press 'v' to refresh. Last refresh: {}s ago",
            state.validation_statuses.len(),
            last_refresh_secs
        )
    };

    let instructions = Paragraph::new(status_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Validation Status (Polled)")
                .title_style(Style::default().fg(Color::Magenta)),
        )
        .style(Style::default().fg(Color::White));

    f.render_widget(instructions, chunks[0]);

    // Initialize selection if not set and items exist
    if state.validation_status_state.selected().is_none() && !state.validation_statuses.is_empty() {
        state.validation_status_state.select(Some(0));
    }

    // Build list items
    let list_items: Vec<ListItem> = state
        .validation_statuses
        .iter()
        .enumerate()
        .map(|(idx, status)| {
            let htx_short = format_short_hex(&status.htx_id);
            let expected = status.expected_validators.len();
            let actual = status.actual_validators.len();
            let missing = status.missing_validators.len();

            let (status_icon, status_color) = if status.is_complete {
                ("✓", Color::Green)
            } else if actual > 0 {
                ("◐", Color::Yellow) // Partial
            } else {
                ("✗", Color::Red)
            };

            let missing_text = if missing > 0 {
                let missing_addrs: Vec<String> = status
                    .missing_validators
                    .iter()
                    .map(|a| format_short_hex(&format!("{:?}", a)))
                    .collect();
                format!(" Missing: {}", missing_addrs.join(", "))
            } else {
                String::new()
            };

            let content = vec![
                Span::raw(format!("{}. ", idx + 1)),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::raw(format!(
                    " {} | {}/{} validated",
                    htx_short, actual, expected
                )),
                Span::styled(
                    missing_text,
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::DIM),
                ),
            ];

            ListItem::new(Line::from(content))
        })
        .collect();

    // Summary stats
    let complete_count = state
        .validation_statuses
        .iter()
        .filter(|s| s.is_complete)
        .count();
    let partial_count = state
        .validation_statuses
        .iter()
        .filter(|s| !s.is_complete && !s.actual_validators.is_empty())
        .count();
    let none_count = state
        .validation_statuses
        .iter()
        .filter(|s| s.actual_validators.is_empty())
        .count();

    let title = format!(
        "HTX Validation Status - Complete: {} | Partial: {} | None: {} - Use ↑↓ to scroll",
        complete_count, partial_count, none_count
    );

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(Style::default().fg(Color::Green)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, chunks[1], &mut state.validation_status_state);
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
            "v",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Load Validation  "),
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

    // Add node-specific controls when in Nodes tab or ValidationStatus tab
    if state.current_tab == Tab::Nodes
        || state.current_tab == Tab::ValidationStatus
        || state.current_tab == Tab::HTXTracking
        || state.current_tab == Tab::TokenHolders
    {
        help_spans.extend(vec![
            Span::styled(
                "↑↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Select  "),
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

fn render_staking(f: &mut Frame, area: Rect, state: &MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Private Key
            Constraint::Length(3), // Target Address
            Constraint::Length(3), // Amount
            Constraint::Length(5), // Status/Feedback
            Constraint::Min(0),    // Help
        ])
        .split(area);

    let pk_style = if state.staking_active_input == StakingInputField::PrivateKey {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let target_style = if state.staking_active_input == StakingInputField::TargetAddress {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let amount_style = if state.staking_active_input == StakingInputField::Amount {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let pk_input = Paragraph::new(state.staking_private_key.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Private Key (Optional - leave empty to use default)"),
        )
        .style(pk_style)
        .scroll((0, state.staking_private_key.len().saturating_sub(50) as u16));

    // Add character counter to help user see if full address is entered
    let target_title = format!(
        "Target Operator Address (entered: {} chars, need: 42)",
        state.staking_target_address.len()
    );
    // Auto-scroll to show the end of the address as user types
    let target_scroll_offset = state.staking_target_address.len().saturating_sub(50) as u16;
    let target_input = Paragraph::new(state.staking_target_address.as_str())
        .block(Block::default().borders(Borders::ALL).title(target_title))
        .style(target_style)
        .scroll((0, target_scroll_offset));

    let amount_input = Paragraph::new(state.staking_amount.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Amount (TEST)"),
        )
        .style(amount_style)
        .scroll((0, 0));

    f.render_widget(pk_input, chunks[0]);
    f.render_widget(target_input, chunks[1]);
    f.render_widget(amount_input, chunks[2]);

    // Status feedback box - always show status
    let status_color = if state.status_message.contains("Error") {
        Color::Red
    } else if state.status_message.contains("submitted")
        || state.status_message.contains("Stake submitted")
    {
        Color::Green
    } else if state.status_message.contains("Submitting")
        || state.status_message.contains("stake...")
    {
        Color::Yellow
    } else {
        Color::Cyan
    };

    let status_text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            &state.status_message,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        )]),
    ];

    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .alignment(Alignment::Center);

    f.render_widget(status, chunks[3]);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Press Enter to Submit Stake",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("Use Up/Down to navigate fields"),
    ];

    let help = Paragraph::new(help_text).block(Block::default().borders(Borders::NONE));

    f.render_widget(help, chunks[4]);
}

fn render_token_holders(f: &mut Frame, area: Rect, state: &mut MonitorState) {
    // Initialize selection if not set and items exist
    if state.token_holders_state.selected().is_none() && !state.token_holders.is_empty() {
        state.token_holders_state.select(Some(0));
    }

    let list_items: Vec<ListItem> = state
        .token_holders
        .iter()
        .enumerate()
        .map(|(idx, holder)| {
            let balance_formatted =
                format_units(holder.balance, 18).unwrap_or_else(|_| "0".to_string());

            let content = vec![
                Span::raw(format!("{}. ", idx + 1)),
                Span::styled(
                    format!("{:?}", holder.address),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" | Balance: {} TEST", balance_formatted)),
            ];

            ListItem::new(Line::from(content))
        })
        .collect();

    let scroll_help = if !state.token_holders.is_empty() {
        " - Use ↑↓ to scroll"
    } else {
        ""
    };

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    "TEST Token Holders - Nodes & Operators ({}){}",
                    state.token_holders.len(),
                    scroll_help
                ))
                .title_style(Style::default().fg(Color::Green)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut state.token_holders_state);
}

fn render_minting(f: &mut Frame, area: Rect, state: &MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Private Key
            Constraint::Length(3), // Target Address
            Constraint::Length(3), // Amount
            Constraint::Length(5), // Status/Feedback
            Constraint::Min(0),    // Help
        ])
        .split(area);

    let pk_style = if state.minting_active_input == MintingInputField::PrivateKey {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let target_style = if state.minting_active_input == MintingInputField::TargetAddress {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let amount_style = if state.minting_active_input == MintingInputField::Amount {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let pk_input = Paragraph::new(state.minting_private_key.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Private Key (Optional - leave empty to use default)"),
        )
        .style(pk_style)
        .scroll((0, state.minting_private_key.len().saturating_sub(50) as u16));

    // Add character counter to help user see if full address is entered
    let target_title = format!(
        "Target Address (entered: {} chars, need: 42)",
        state.minting_target_address.len()
    );
    // Auto-scroll to show the end of the address as user types
    let target_scroll_offset = state.minting_target_address.len().saturating_sub(50) as u16;
    let target_input = Paragraph::new(state.minting_target_address.as_str())
        .block(Block::default().borders(Borders::ALL).title(target_title))
        .style(target_style)
        .scroll((0, target_scroll_offset));

    let amount_input = Paragraph::new(state.minting_amount.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Amount (TEST tokens)"),
        )
        .style(amount_style)
        .scroll((0, 0));

    f.render_widget(pk_input, chunks[0]);
    f.render_widget(target_input, chunks[1]);
    f.render_widget(amount_input, chunks[2]);

    // Status feedback box - always show status
    let status_color = if state.status_message.contains("Error") {
        Color::Red
    } else if state.status_message.contains("minted")
        || state.status_message.contains("Tokens minted")
    {
        Color::Green
    } else if state.status_message.contains("Minting") {
        Color::Yellow
    } else {
        Color::Cyan
    };

    let status_text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            &state.status_message,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        )]),
    ];

    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .alignment(Alignment::Center);

    f.render_widget(status, chunks[3]);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Press Enter to Mint Tokens",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("Use Up/Down to navigate fields"),
    ];

    let help = Paragraph::new(help_text).block(Block::default().borders(Borders::NONE));

    f.render_widget(help, chunks[4]);
}

fn render_transfer_eth(f: &mut Frame, area: Rect, state: &MonitorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Private Key
            Constraint::Length(3), // Target Address
            Constraint::Length(3), // Amount
            Constraint::Length(5), // Status/Feedback
            Constraint::Min(0),    // Help
        ])
        .split(area);

    let pk_style = if state.transfer_active_input == TransferInputField::PrivateKey {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let target_style = if state.transfer_active_input == TransferInputField::TargetAddress {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let amount_style = if state.transfer_active_input == TransferInputField::Amount {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let pk_input = Paragraph::new(state.transfer_private_key.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Private Key (Optional - leave empty to use default)"),
        )
        .style(pk_style)
        .scroll((
            0,
            state.transfer_private_key.len().saturating_sub(50) as u16,
        ));

    let target_title = format!(
        "Target Address (entered: {} chars, need: 42)",
        state.transfer_target_address.len()
    );
    let target_scroll_offset = state.transfer_target_address.len().saturating_sub(50) as u16;
    let target_input = Paragraph::new(state.transfer_target_address.as_str())
        .block(Block::default().borders(Borders::ALL).title(target_title))
        .style(target_style)
        .scroll((0, target_scroll_offset));

    let amount_input = Paragraph::new(state.transfer_amount.as_str())
        .block(Block::default().borders(Borders::ALL).title("Amount (ETH)"))
        .style(amount_style)
        .scroll((0, 0));

    f.render_widget(pk_input, chunks[0]);
    f.render_widget(target_input, chunks[1]);
    f.render_widget(amount_input, chunks[2]);

    // Status feedback box
    let status_color = if state.status_message.contains("Error") {
        Color::Red
    } else if state.status_message.contains("transferred")
        || state.status_message.contains("ETH transferred")
    {
        Color::Green
    } else if state.status_message.contains("Transferring") {
        Color::Yellow
    } else {
        Color::Cyan
    };

    let status_text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            &state.status_message,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        )]),
    ];

    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .alignment(Alignment::Center);

    f.render_widget(status, chunks[3]);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Press Enter to Transfer ETH",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("Use Up/Down to navigate fields"),
    ];

    let help = Paragraph::new(help_text).block(Block::default().borders(Borders::NONE));

    f.render_widget(help, chunks[4]);
}

// WebSocket event listener for Transfer events (parallel processing)
// Refreshes token balances for all system addresses (nodes and operators)
async fn listen_token_transfers(
    client: Arc<NilUVClient>,
    state: Arc<Mutex<MonitorState>>,
) -> Result<()> {
    let token_arc = Arc::new(client.token.clone());
    token_arc
        .listen_transfer_events(move |_event| {
            let state = state.clone();
            let client = client.clone();
            async move {
                // Collect system addresses: registered nodes + staked operators
                let mut system_addresses = HashSet::new();

                // Add all registered nodes
                if let Ok(registered_nodes) = client.manager.get_nodes().await {
                    for addr in registered_nodes {
                        system_addresses.insert(addr);
                    }
                }

                // Add all operators with stake (efficient: direct contract query)
                if let Ok(staked_operators) = client.staking.get_operators_with_stake().await {
                    for operator in staked_operators {
                        system_addresses.insert(operator);
                    }
                }

                // Update tracked addresses in state
                {
                    let mut state_guard = state.lock().await;
                    state_guard.token_holder_addresses = system_addresses.clone();
                }

                // Fetch updated balances for all system addresses
                let mut holders = Vec::new();
                for addr in system_addresses {
                    if let Ok(balance) = client.token.balance_of(addr).await {
                        if balance > U256::ZERO {
                            holders.push(TokenHolder {
                                address: addr,
                                balance,
                            });
                        }
                    }
                }

                // Sort by balance descending
                holders.sort_by(|a, b| b.balance.cmp(&a.balance));

                // Update state
                {
                    let mut state_guard = state.lock().await;
                    state_guard.token_holders = holders;
                    state_guard.last_update = std::time::Instant::now();
                }

                Ok(())
            }
        })
        .await
}

/// Load validation status by polling the contract directly (not via streaming)
/// This queries historical events and computes which HTXs have missing validations
/// Lookback for validation status - query more history than the default 50 blocks
const VALIDATION_LOOKBACK_BLOCKS: u64 = 10_000;

async fn load_validation_status(client: &NilUVClient) -> Result<Vec<HTXValidationStatus>> {
    let mut statuses = Vec::new();

    // Get all assigned events (RoundStarted) - this tells us expected validators
    // Use larger lookback than default to see more history
    let assigned_events = client
        .manager
        .get_htx_assigned_events_with_lookback(VALIDATION_LOOKBACK_BLOCKS)
        .await?;

    // Get all responded events (OperatorVoted) - this tells us actual validators
    let responded_events = client
        .manager
        .get_htx_responded_events_with_lookback(VALIDATION_LOOKBACK_BLOCKS)
        .await?;

    // Build a map of htx_id -> responded operators
    let mut responded_map: HashMap<String, HashSet<Address>> = HashMap::new();
    for event in responded_events {
        let htx_id = bytes_to_hex(event.heartbeatKey.as_slice());
        responded_map
            .entry(htx_id)
            .or_default()
            .insert(event.operator);
    }

    // Process each assigned event to build validation status
    for event in assigned_events {
        let htx_id = bytes_to_hex(event.heartbeatKey.as_slice());
        let expected_validators: Vec<Address> = event.members.to_vec();
        let expected_set: HashSet<Address> = expected_validators.iter().cloned().collect();

        let actual_validators: Vec<Address> = responded_map
            .get(&htx_id)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        let actual_set: HashSet<Address> = actual_validators.iter().cloned().collect();

        let missing_validators: Vec<Address> = expected_set
            .difference(&actual_set)
            .cloned()
            .collect();

        let is_complete = missing_validators.is_empty() && !expected_validators.is_empty();

        statuses.push(HTXValidationStatus {
            htx_id,
            expected_validators,
            actual_validators,
            missing_validators,
            is_complete,
        });
    }

    // Sort by completion status (incomplete first), then by number of missing validators
    statuses.sort_by(|a, b| {
        match (a.is_complete, b.is_complete) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => b.missing_validators.len().cmp(&a.missing_validators.len()),
        }
    });

    Ok(statuses)
}
