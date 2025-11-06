use std::env;

use futures_util::{Sink, SinkExt, StreamExt};
use nilav::{
    stable_stringify, AssignmentMsg, TransactionEnvelope, VerificationPayload,
    VerificationResultMsg,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use blake3::Hasher as Blake3;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::random;
use reqwest::Client;
use std::fs;
use std::path::PathBuf;
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

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

    // Prepare to verify via HTTP
    let (valid, reason) = match verify_htx(&assign.htx).await {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e.message())),
    };
    let tx = TransactionEnvelope {
        htx: assign.htx,
        valid,
        reason,
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
    let verdict = if result.payload.transaction.valid {
        format!("{}Verified{}", GREEN, RESET)
    } else {
        let why = result
            .payload
            .transaction
            .reason
            .as_deref()
            .unwrap_or("unknown");
        format!("{}Not Verified{} (reason: {})", RED, RESET, why)
    };
    println!("[nilAV:{}] slot {}: {}", node_id, assign.slot, verdict);
    Ok(())
}

#[derive(Debug)]
enum VerificationError {
    NilccUrl(String),
    NilccJson(String),
    MissingMeasurement,
    BuilderUrl(String),
    BuilderJson(String),
    NotInBuilderIndex,
}

impl VerificationError {
    fn message(&self) -> String {
        match self {
            VerificationError::NilccUrl(e) => format!("invalid nil_cc_measurement URL: {}", e),
            VerificationError::NilccJson(e) => format!("invalid nil_cc_measurement JSON: {}", e),
            VerificationError::MissingMeasurement => {
                "missing `measurement` field (looked at root and report.measurement)".to_string()
            }
            VerificationError::BuilderUrl(e) => format!("invalid builder_measurement URL: {}", e),
            VerificationError::BuilderJson(e) => format!("invalid builder_measurement JSON: {}", e),
            VerificationError::NotInBuilderIndex => {
                "measurement not found in builder index".to_string()
            }
        }
    }
}

async fn verify_htx(htx: &nilav::Htx) -> Result<(), VerificationError> {
    let client = Client::new();
    // Fetch nil_cc measurement
    let meas_url = &htx.nil_cc_measurement.url;
    let meas_resp = client.get(meas_url).send().await;
    let meas_json: serde_json::Value = match meas_resp.and_then(|r| r.error_for_status()) {
        Ok(resp) => match resp.json().await {
            Ok(v) => v,
            Err(e) => return Err(VerificationError::NilccJson(e.to_string())),
        },
        Err(e) => return Err(VerificationError::NilccUrl(e.to_string())),
    };
    let measurement = meas_json
        .get("measurement")
        .and_then(|v| v.as_str())
        .or_else(|| {
            meas_json
                .get("report")
                .and_then(|r| r.get("measurement"))
                .and_then(|v| v.as_str())
        });
    let measurement = match measurement {
        Some(s) => s.to_string(),
        None => return Err(VerificationError::MissingMeasurement),
    };

    // Fetch builder measurement index
    let builder_resp = client.get(&htx.builder_measurement.url).send().await;
    let builder_json: serde_json::Value = match builder_resp.and_then(|r| r.error_for_status()) {
        Ok(resp) => match resp.json().await {
            Ok(v) => v,
            Err(e) => return Err(VerificationError::BuilderJson(e.to_string())),
        },
        Err(e) => return Err(VerificationError::BuilderUrl(e.to_string())),
    };

    let mut matches_any = false;
    match builder_json {
        serde_json::Value::Object(map) => {
            for (_k, v) in map {
                if let Some(val) = v.as_str() {
                    if val == measurement {
                        matches_any = true;
                        break;
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                if let Some(val) = v.as_str() {
                    if val == measurement {
                        matches_any = true;
                        break;
                    }
                }
            }
        }
        _ => {}
    }

    if matches_any {
        Ok(())
    } else {
        Err(VerificationError::NotInBuilderIndex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn make_htx(nilcc_url: String, builder_url: String) -> nilav::Htx {
        nilav::Htx {
            workload_id: nilav::WorkloadId {
                current: 1,
                previous: 0,
            },
            nil_cc_operator: nilav::NilCcOperator {
                id: 1,
                name: "op".into(),
            },
            builder: nilav::Builder {
                id: 1,
                name: "builder".into(),
            },
            nil_cc_measurement: nilav::NilCcMeasurement {
                url: nilcc_url,
                nilcc_version: "0.0.0".into(),
                cpu_count: 1,
                gpus: 0,
            },
            builder_measurement: nilav::BuilderMeasurement { url: builder_url },
        }
    }

    #[tokio::test]
    async fn verify_ok_when_measurement_matches_root() {
        let nilcc = MockServer::start();
        let builder = MockServer::start();
        nilcc.mock(|when, then| {
            when.method(GET).path("/m");
            then.status(200)
                .json_body(serde_json::json!({"measurement":"deadbeef"}));
        });
        builder.mock(|when, then| {
            when.method(GET).path("/b");
            then.status(200)
                .json_body(serde_json::json!({"0.2.1":"deadbeef"}));
        });
        let htx = make_htx(
            format!("{}/m", nilcc.base_url()),
            format!("{}/b", builder.base_url()),
        );
        assert!(verify_htx(&htx).await.is_ok());
    }

    #[tokio::test]
    async fn verify_ok_when_measurement_matches_nested() {
        let nilcc = MockServer::start();
        let builder = MockServer::start();
        nilcc.mock(|when, then| {
            when.method(GET).path("/m");
            then.status(200)
                .json_body(serde_json::json!({"report":{"measurement":"cafebabe"}}));
        });
        builder.mock(|when, then| {
            when.method(GET).path("/b");
            then.status(200)
                .json_body(serde_json::json!(["cafebabe", "xxxx"]));
        });
        let htx = make_htx(
            format!("{}/m", nilcc.base_url()),
            format!("{}/b", builder.base_url()),
        );
        assert!(verify_htx(&htx).await.is_ok());
    }

    #[tokio::test]
    async fn verify_err_missing_measurement() {
        let nilcc = MockServer::start();
        let builder = MockServer::start();
        nilcc.mock(|when, then| {
            when.method(GET).path("/m");
            then.status(200).json_body(serde_json::json!({"foo":"bar"}));
        });
        builder.mock(|when, then| {
            when.method(GET).path("/b");
            then.status(200).json_body(serde_json::json!({}));
        });
        let htx = make_htx(
            format!("{}/m", nilcc.base_url()),
            format!("{}/b", builder.base_url()),
        );
        let err = verify_htx(&htx).await.err().unwrap();
        assert!(matches!(err, VerificationError::MissingMeasurement));
    }

    #[tokio::test]
    async fn verify_err_builder_json() {
        let nilcc = MockServer::start();
        let builder = MockServer::start();
        nilcc.mock(|when, then| {
            when.method(GET).path("/m");
            then.status(200)
                .json_body(serde_json::json!({"measurement":"aa"}));
        });
        builder.mock(|when, then| {
            when.method(GET).path("/b");
            then.status(200).body("not-json");
        });
        let htx = make_htx(
            format!("{}/m", nilcc.base_url()),
            format!("{}/b", builder.base_url()),
        );
        let err = verify_htx(&htx).await.err().unwrap();
        assert!(matches!(err, VerificationError::BuilderJson(_)));
    }

    #[tokio::test]
    async fn verify_err_not_in_index() {
        let nilcc = MockServer::start();
        let builder = MockServer::start();
        nilcc.mock(|when, then| {
            when.method(GET).path("/m");
            then.status(200)
                .json_body(serde_json::json!({"measurement":"nomatch"}));
        });
        builder.mock(|when, then| {
            when.method(GET).path("/b");
            then.status(200)
                .json_body(serde_json::json!({"0.2.1":"deadbeef"}));
        });
        let htx = make_htx(
            format!("{}/m", nilcc.base_url()),
            format!("{}/b", builder.base_url()),
        );
        let err = verify_htx(&htx).await.err().unwrap();
        assert!(matches!(err, VerificationError::NotInBuilderIndex));
    }
}
