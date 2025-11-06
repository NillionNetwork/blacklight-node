# nilAV (Nillion Auditor-Verifier) - Rust

A Rust simulation of an L2 scheduler and a `nilAV` node client.

- Server: rolls a dice every slot, assigns registered nodes to verify an HTX (count set by config).
- Node: subscribes, runs a placeholder verification, signs the result (Ed25519) over canonical JSON, submits back.

## Requirements

- Rust (stable)

## Build

```bash
cargo build
```

## Run the simulator server

```bash
cargo run --bin server
```

This starts a WebSocket server on `ws://localhost:8080`.

### Docker (server)

```bash
docker compose -f docker/compose.server.yml up --build
# Server at ws://localhost:8080
```

## Run nilAV nodes

Open separate terminals and run nodes with unique IDs:

```bash
NODE_ID=1 cargo run --bin nilav_node
NODE_ID=2 cargo run --bin nilav_node
NODE_ID=3 cargo run --bin nilav_node
NODE_ID=4 cargo run --bin nilav_node
```

Optional configuration:

- `WS_URL` (default `ws://localhost:8080`)
- `NODE_SECRET` (hex): deterministic Ed25519 key seed. If absent, an ephemeral key is used.

Key management:

- If `NODE_SECRET` is not set, the node will look for a local file named `{NODE_ID}.env` in the working directory.
- If the file exists, it reads `NODE_SECRET=0x...` from it; otherwise it creates a new Ed25519 key and writes the file.
- On registration the node also includes its `publicKey` so the server can verify signatures.

Signature scope:

- The node signs a canonical (key-sorted) JSON string of the `transaction` object: `{ htx, valid }`.
- The server verifies the signature for each `verification_result` using the advertised public key before counting an approval.

### Docker (nodes)

Scale N nodes and connect to a server running on the host (default `WS_URL=ws://host.docker.internal:8080`):

```bash
docker compose -f docker/compose.nodes.yml up --build --scale nilav_node=3
```

Override server URL if needed:

```bash
WS_URL=ws://localhost:8080 docker compose -f docker/compose.nodes.yml up --build --scale nilav_node=5
```

## HTX (heartbeat transaction):

```json
{
  "workload_id": {
    "current": 123,
    "previous": 12
  },
  "nilCC_operator": {
    "id": 4,
    "name": "My Cloud"
  },
  "builder": {
    "id": 94323,
    "name": "0xlala"
  },
  "nilCC_measurement": {
    "url": "https://nilcc.com/...",
    "version": "v1.3.0",
    "vCPUs": 2,
    "GPUs": 1
  },
  "builder_measurement": { "url": "https://github.com/0xlala/..." }
}
```

Verification result:

```json
{
  "transaction": {
    "htx": { /* HTX */ },
    "valid": true
  },
  "signature": "0x..."
}
```
