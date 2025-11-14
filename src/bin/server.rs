use std::{
    collections::{HashMap, HashSet},
    env,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::{net::TcpListener, sync::Mutex, time::interval};
use tokio_tungstenite::tungstenite::Message;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use nilav::{
    choose_k, load_config_from_path,
    types::{AssignmentMsg, Htx, VerificationResultMsg},
    Config,
};

type Tx = tokio::sync::mpsc::UnboundedSender<Message>;

#[derive(Default)]
struct AppState {
    nodes: Mutex<HashMap<String, Tx>>, // nodeId -> sender to its socket task
    slot_states: Mutex<HashMap<u64, SlotState>>, // slot -> assignment/approvals
    pubkeys: Mutex<HashMap<String, VerifyingKey>>, // nodeId -> pubkey
}

#[derive(Default, Debug, Clone)]
struct SlotState {
    assigned: HashSet<String>,
    approvals: HashSet<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    println!(
        "[sim] WebSocket server listening on ws://localhost:{}",
        port
    );

    // Load config
    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    let config: Config = load_config_from_path(&config_path).unwrap_or_default();
    println!(
        "[sim] config: validators_per_htx={}, approve_threshold={}",
        config.election.validators_per_htx, config.election.approve_threshold
    );

    let state = Arc::new(AppState::default());
    // Load HTXs from file
    let htxs_path = env::var("HTXS_PATH").unwrap_or_else(|_| "data/htxs.json".to_string());
    let htxs_str = std::fs::read_to_string(&htxs_path).unwrap_or_else(|_| "[]".to_string());
    let htxs: Vec<Htx> = serde_json::from_str(&htxs_str).unwrap_or_else(|_| Vec::new());
    let htxs = Arc::new(htxs);

    // Slot ticker
    let state_clone = state.clone();
    let config_clone = config.clone();
    let htxs_clone = htxs.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_millis(config_clone.slot_ms));
        let mut slot: u64 = 0;
        loop {
            ticker.tick().await;
            slot += 1;
            // Pick HTX round-robin from file (fallback to empty if none)
            let htx = if !htxs_clone.is_empty() {
                let idx = ((slot - 1) as usize) % htxs_clone.len();
                htxs_clone[idx].clone()
            } else {
                continue;
            };

            let nodes: Vec<String> = {
                let map = state_clone.nodes.lock().await;
                map.keys().cloned().collect()
            };
            if nodes.is_empty() {
                println!("[sim] slot {}: no nodes registered", slot);
                continue;
            }
            let assigned = choose_k(
                &nodes,
                nodes.len().min(config_clone.election.validators_per_htx),
            );
            println!(
                "[sim] slot {}: assigning nodes [{}]",
                slot,
                assigned.join(", ")
            );

            // Track slot state
            let mut states = state_clone.slot_states.lock().await;
            let mut st = SlotState::default();
            st.assigned = assigned.iter().cloned().collect();
            states.insert(slot, st);

            for node_id in assigned {
                let msg = AssignmentMsg {
                    msg_type: "assignment".into(),
                    slot,
                    node_id: node_id.clone(),
                    htx: htx.clone(),
                };
                let text = serde_json::to_string(&msg).unwrap();
                let maybe_tx = { state_clone.nodes.lock().await.get(&node_id).cloned() };
                if let Some(tx) = maybe_tx {
                    let _ = tx.send(Message::Text(text.into()));
                }
            }
        }
    });

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr().ok();
        let ws_stream = tokio_tungstenite::accept_async(stream).await?;
        println!("[sim] new connection from {:?}", peer);

        let (mut sink, mut stream) = ws_stream.split();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

        // Writer task
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let _ = sink.send(msg).await;
            }
        });

        let state_conn = state.clone();
        let config_conn = config.clone();
        tokio::spawn(async move {
            let mut registered_id: Option<String> = None;
            while let Some(Ok(msg)) = stream.next().await {
                if let Message::Text(txt) = msg {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                        if v.get("type") == Some(&json!("register")) {
                            if let Some(node_id) = v
                                .get("nodeId")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string())
                            {
                                state_conn
                                    .nodes
                                    .lock()
                                    .await
                                    .insert(node_id.clone(), tx.clone());
                                if let Some(pk_hex) = v.get("publicKey").and_then(|x| x.as_str()) {
                                    if let Ok(bytes) = hex::decode(pk_hex.trim_start_matches("0x"))
                                    {
                                        if bytes.len() == 32 {
                                            let mut arr = [0u8; 32];
                                            arr.copy_from_slice(&bytes);
                                            if let Ok(vk) = VerifyingKey::from_bytes(&arr) {
                                                state_conn
                                                    .pubkeys
                                                    .lock()
                                                    .await
                                                    .insert(node_id.clone(), vk);
                                            }
                                        }
                                    }
                                }
                                registered_id = Some(node_id.clone());
                                let ack = json!({"type":"registered", "nodeId": node_id});
                                let _ = tx.send(Message::Text(ack.to_string().into()));
                            }
                        } else if v.get("type") == Some(&json!("verification_result")) {
                            // Aggregate approvals per slot
                            if let Ok(res) =
                                serde_json::from_value::<VerificationResultMsg>(v.clone())
                            {
                                let mut states = state_conn.slot_states.lock().await;
                                if let Some(st) = states.get_mut(&res.slot) {
                                    let is_verified = res.payload.transaction.valid;

                                    // Serialize transaction canonically for signature check
                                    let msg = match serde_json::to_value(&res.payload.transaction) {
                                        Ok(vv) => {
                                            let c = nilav::canonicalize_json(&vv);
                                            serde_json::to_string(&c).unwrap_or_default()
                                        }
                                        Err(_) => String::new(),
                                    };

                                    // Attempt signature verification if we have a pubkey
                                    let mut sig_ok = false;
                                    if let Some(vk) =
                                        state_conn.pubkeys.lock().await.get(&res.node_id).cloned()
                                    {
                                        if let Ok(sig_bytes) = hex::decode(
                                            res.payload.signature.trim_start_matches("0x"),
                                        ) {
                                            if let Ok(sig) = Signature::from_slice(&sig_bytes) {
                                                sig_ok = vk.verify(msg.as_bytes(), &sig).is_ok();
                                            }
                                        }
                                    }

                                    // Colored status lines
                                    let green = "\x1b[32m";
                                    let red = "\x1b[31m";
                                    let reset = "\x1b[0m";
                                    let reason_suffix = if !is_verified {
                                        match &res.payload.transaction.reason {
                                            Some(r) if !r.is_empty() => format!(" (reason: {})", r),
                                            _ => " (reason: unknown)".to_string(),
                                        }
                                    } else {
                                        String::new()
                                    };
                                    let verified_str = if is_verified {
                                        format!("{}Verified{}", green, reset)
                                    } else {
                                        format!("{}Not Verified{}{}", red, reset, reason_suffix)
                                    };
                                    let sig_str = if sig_ok {
                                        format!("{}signature valid{}", green, reset)
                                    } else {
                                        format!("{}signature invalid{}", red, reset)
                                    };
                                    println!(
                                        "[sim] slot {} node {}: {} | {}",
                                        res.slot, res.node_id, verified_str, sig_str
                                    );

                                    // Count approvals only if both conditions are met
                                    if st.assigned.contains(&res.node_id) && is_verified && sig_ok {
                                        st.approvals.insert(res.node_id.clone());
                                        let count = st.approvals.len();
                                        println!(
                                            "[sim] slot {} approvals: {}/{}",
                                            res.slot, count, config_conn.election.approve_threshold
                                        );
                                        if count >= config_conn.election.approve_threshold {
                                            println!("[sim] slot {} HTX deemed VALID (threshold reached)", res.slot);
                                        }
                                    }
                                }
                            } else {
                                println!("[sim] malformed verification_result: {}", txt);
                            }
                        }
                    }
                }
            }
            // Cleanup on disconnect
            if let Some(id) = registered_id {
                state_conn.nodes.lock().await.remove(&id);
            }
        });
    }

    Ok(())
}
