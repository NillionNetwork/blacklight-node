use std::env;

use futures_util::{Sink, SinkExt, StreamExt};
use nilav::{
    stable_stringify, AssignmentMsg, TransactionEnvelope, VerificationPayload,
    VerificationResultMsg,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use blake3::Hasher as Blake3;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::{random, Rng};
use std::fs;
use std::path::PathBuf;

fn signing_key_from_env_or_file(node_id: &str) -> SigningKey {
    if let Ok(secret_hex) = env::var("NODE_SECRET") {
        if let Ok(decoded) = hex::decode(secret_hex.trim_start_matches("0x")) {
            if decoded.len() == 32 {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&decoded);
                return SigningKey::from_bytes(&seed);
            }
            // fallback: hash arbitrary input to 32 bytes
            let mut hasher = Blake3::new();
            hasher.update(&decoded);
            let digest = hasher.finalize();
            let seed: [u8; 32] = digest.as_bytes().clone();
            return SigningKey::from_bytes(&seed);
        }
    }
    // Try nodeid.env in CWD
    let path = PathBuf::from(format!("{}.env", node_id));
    if path.exists() {
        if let Ok(contents) = fs::read_to_string(&path) {
            for line in contents.lines() {
                if let Some(val) = line.strip_prefix("NODE_SECRET=") {
                    if let Ok(decoded) = hex::decode(val.trim().trim_start_matches("0x")) {
                        if decoded.len() == 32 {
                            let mut seed = [0u8; 32];
                            seed.copy_from_slice(&decoded);
                            return SigningKey::from_bytes(&seed);
                        }
                    }
                }
            }
        }
    }
    // Create new seed and persist
    let seed: [u8; 32] = random();
    let line = format!("NODE_SECRET=0x{}\n", hex::encode(seed));
    let _ = fs::write(&path, line);
    SigningKey::from_bytes(&seed)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ws_url = env::var("WS_URL").unwrap_or_else(|_| "ws://localhost:8080".to_string());
    let node_id = env::var("NODE_ID")
        .ok()
        .or_else(|| env::var("HOSTNAME").ok())
        .unwrap_or_else(|| format!("node-{}", hex::encode(rand::random::<[u8; 4]>())));

    let sk = signing_key_from_env_or_file(&node_id);
    let vk = VerifyingKey::from(&sk);
    println!("[nilAV:{}] pubkey {}", node_id, hex::encode(vk.to_bytes()));

    let (mut ws, _) = connect_async(ws_url.clone()).await?;
    println!("[nilAV:{}] connected to {}", node_id, ws_url);
    ws.send(Message::Text(
        serde_json::json!({"type":"register","nodeId": node_id, "publicKey": hex::encode(vk.to_bytes())})
            .to_string()
            .into(),
    ))
    .await?;

    while let Some(msg) = ws.next().await {
        let msg = msg?;
        if let Message::Text(txt) = msg {
            if let Ok(assign) = serde_json::from_str::<AssignmentMsg>(&txt) {
                if assign.node_id == node_id && assign.msg_type == "assignment" {
                    handle_assignment(&mut ws, &sk, &node_id, assign).await?;
                }
            }
        }
    }

    Ok(())
}

async fn handle_assignment<S>(
    ws: &mut S,
    sk: &SigningKey,
    node_id: &str,
    assign: AssignmentMsg,
) -> anyhow::Result<()>
where
    S: Sink<Message> + Unpin,
    <S as Sink<Message>>::Error: std::fmt::Debug + std::error::Error + Send + Sync + 'static,
{
    println!(
        "[nilAV:{}] received assignment for slot {}",
        node_id, assign.slot
    );

    // Black-box verification placeholder: exact 20% chance to mark invalid
    let mut rng = rand::rng();
    let make_invalid = rng.random_range(0..5) == 0;
    let tx = TransactionEnvelope {
        htx: assign.htx,
        valid: !make_invalid,
    };
    let serialized = stable_stringify(&tx)?;
    let sig = sk.sign(serialized.as_bytes());

    let payload = VerificationPayload {
        transaction: tx,
        signature: hex::encode(sig.to_bytes()),
    };
    let result = VerificationResultMsg {
        msg_type: "verification_result".into(),
        node_id: node_id.to_string(),
        slot: assign.slot,
        payload,
    };

    ws.send(Message::Text(serde_json::to_string(&result)?.into()))
        .await?;
    println!(
        "[nilAV:{}] submitted verification for slot {}",
        node_id, assign.slot
    );
    Ok(())
}
