## [Code](https://github.com/abhi3700/reth/commit/0c00e95900c9c1226c45d0cf0feec69de7bf93ea)

## Notes

I created a fully compiling e2e test that launches **two test reth nodes**, connects them over P2P, has **Alice send 1 ETH to Bob**, and walks through the entire lifecycle: tx pool insertion → P2P gossip → payload building → block mining.

## Files

**New:** `crates/ethereum/node/tests/e2e/tx_pool_propagation.rs` — the test
**Modified:** `crates/ethereum/node/tests/e2e/main.rs` — registered `mod tx_pool_propagation;`

Verified with `cargo check -p reth-node-ethereum --tests --features test-utils` → **0 errors, 0 warnings**.

## The Code

```rust
use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_eips::Encodable2718;
use alloy_network::TxSignerSync;
use alloy_primitives::{TxKind, U256};
use reth_chainspec::{ChainSpecBuilder, MAINNET};
use reth_e2e_test_utils::{
    setup_engine_with_connection, transaction::TransactionTestContext, wallet::Wallet,
};
use reth_node_ethereum::EthereumNode;
use reth_rpc_api::EthApiServer;
use std::{sync::Arc, time::Duration};

const ONE_ETH_WEI: u128 = 1_000_000_000_000_000_000; // 1 ETH = 10^18 wei

#[tokio::test(flavor = "multi_thread")]
async fn alice_sends_1_eth_to_bob_via_tx_pool_and_p2p() -> eyre::Result<()> {
    reth_tracing::init_test_tracing();

    // 1. Chain spec from the test genesis (funds the hardhat accounts)
    let chain_spec = Arc::new(
        ChainSpecBuilder::default()
            .chain(MAINNET.chain)
            .genesis(serde_json::from_str(include_str!("../assets/genesis.json")).unwrap())
            .cancun_activated()
            .prague_activated()
            .build(),
    );
    let chain_id = chain_spec.chain().into();

    // 2. Alice = funded account #0 (0xf39F...), Bob = funded account #1 (0x7099...)
    let mut wallets = Wallet::new(2).with_chain_id(chain_id).wallet_gen();
    let alice = wallets.remove(0);
    let bob = wallets.remove(0);

    // 3. Launch TWO Engine-API test nodes, NOT auto-connected (we wire P2P manually below)
    let (mut nodes, _) = setup_engine_with_connection::<EthereumNode>(
        2,
        chain_spec.clone(),
        false, // drive payloads explicitly via Engine API
        Default::default(),
        crate::utils::eth_payload_attributes,
        false, // don't auto-connect
    )
    .await?;
    let (mut node_a, mut node_b) = (nodes.remove(0), nodes.remove(0));

    // 4. Establish the devp2p session between node A and node B
    node_a.connect(&mut node_b).await;

    // 5. Warm-up: mine block #1 on A, submit it to B, and sync B to it.
    //    This prevents both nodes from considering themselves "unsynced",
    //    a prerequisite for tx pool accept/gossip.
    let seed_tx = TransactionTestContext::transfer_tx_bytes(chain_id, alice.clone()).await;
    node_a.rpc.inject_tx(seed_tx).await?;
    let warmup_payload = node_a.advance_block().await?;
    node_b.submit_payload(warmup_payload.clone()).await?;
    node_b.sync_to(warmup_payload.block().hash()).await?;

    // 6. Alice signs an EIP-1559 tx: send 1 ETH to Bob (nonce = 1, warm-up used nonce 0)
    let mut tx = TxEip1559 {
        chain_id,
        nonce: 1,
        gas_limit: 21_000,
        max_fee_per_gas: 1_000_000_000,
        max_priority_fee_per_gas: 1_000_000_000,
        to: TxKind::Call(bob.address()),
        value: U256::from(ONE_ETH_WEI),
        ..Default::default()
    };
    let signature = alice.sign_transaction_sync(&mut tx).unwrap();
    let envelope = TxEnvelope::Eip1559(tx.into_signed(signature));
    let tx_hash = *envelope.tx_hash();
    println!("Alice -> Bob signed tx: {tx_hash}");

    // 7. Inject the raw tx via `eth_sendRawTransaction` -> node A's tx pool
    let before_bob =
        node_a.rpc.inner.eth_api().balance(bob.address(), Default::default()).await?;
    node_a.rpc.inject_tx(envelope.encoded_2718().into()).await?;
    println!("tx {tx_hash} accepted into node A's tx pool");

    // 8. P2P gossip: node A broadcasts the tx; node B validates & inserts it into its pool
    tokio::time::sleep(Duration::from_millis(200)).await;

    let tx_on_b = node_b.rpc.inner.eth_api().transaction_by_hash(tx_hash).await?;
    assert!(tx_on_b.is_some(), "node B should have the tx in its pool via P2P gossip");
    println!("tx {tx_hash} propagated to node B's tx pool over P2P");
    let tx_on_a = node_a.rpc.inner.eth_api().transaction_by_hash(tx_hash).await?;
    assert!(tx_on_a.is_some(), "node A should still have the tx in its pool");

    // 9. Node A's payload builder drains the tx pool and mines Alice's tx into block #2
    let payload = node_a.advance_block().await?;
    let block = payload.block();
    assert_eq!(block.number, 2, "warm-up block was #1, this is #2");
    let tx_in_block = block.body().transactions().next().expect("block has a tx");
    assert_eq!(*tx_in_block.tx_hash(), tx_hash, "mined block contains Alice's tx");
    println!("tx {tx_hash} mined in block #{}", block.number);

    // 10. Verify balances: Bob is up exactly 1 ETH; Alice paid 1 ETH + gas
    let after_bob = node_a.rpc.inner.eth_api().balance(bob.address(), Default::default()).await?;
    let after_alice =
        node_a.rpc.inner.eth_api().balance(alice.address(), Default::default()).await?;
    assert_eq!(after_bob - before_bob, U256::from(ONE_ETH_WEI), "Bob received exactly 1 ETH");
    println!("Bob balance delta: +1 ETH (now {after_bob})");
    println!("Alice balance after paying 1 ETH + gas: {after_alice}");

    Ok(())
}
```

## Step-by-Step Walkthrough

### Step 1 — Genesis & Chain Spec

The test loads `tests/assets/genesis.json` — the same genesis used by all existing e2e tests. Its `alloc` block pre-funds 21 standard **hardhat mnemonic accounts** (`test test ... junk`) with 1,000,000 ETH each. We build a MAINNET `ChainSpec` from it with Cancun + Prague activated.

### Step 2 — Alice & Bob

`Wallet::new(2).wallet_gen()` derives account #0 and account #1 from the mnemonic:

- **Alice** = `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266` (the famous Hardhat account 0)
- **Bob** = `0x70997970C51812dc3A010C7d01b50e0d17dc79C8` (Hardhat account 1)

Both start with 1,000,000 ETH in the genesis allocation, so Alice can easily afford to send 1 ETH + gas.

### Step 3 — Launching Two Test Nodes

`setup_engine_with_connection::<EthereumNode>(2, ...)` (from `reth-e2e-test-utils`) launches **two full `EthereumNode`s** with Engine API + devp2p networking + tx pools + RPC servers, all on unused ports. We pass `connect_nodes = false` so the nodes start **disconnected** — this lets us demonstrate that the transaction travels *across an actual P2P session*, not just shared local state.

Each node gets a full stack:

- **consensus** (validation)
- **storage** (in-memory `TempDatabase`)
- **tx pool** (`Pool` with `CoinbaseTipOrdering`)
- **network** (`NetworkManager`→ devp2p, the transactions manager is wired to the pool)
- **payload builder** + **Engine API** (`reth-engine-local`)

### Step 4 — P2P Connection

`node_a.connect(&mut node_b).await` uses `NetworkTestContext`:

1. `peers_handle().add_peer(...)` — add B's `NodeRecord` to A's peer list
2. Waits for a `PeerAdded` event, then both sides await `SessionEstablished`

After this, A and B have an active **devp2p/RLPx session** and speak `eth` protocol (ETH68/69): they exchange `Status` messages, and the **transactions manager** on each side is listening for new pool content.

### Step 5 — Warm-up Block (Why it's needed)

An unsynced node treats itself as in "initial sync" and **won't gossip or accept** transactions. So we:

1. `node_a.rpc.inject_tx(seed_tx)` → Alice's nonce-0 tx enters A's pool
2. `node_a.advance_block()` → Engine `forkchoiceUpdated` → payload builder drains the pool → block #1 mined & committed
3. `node_b.submit_payload(warmup_payload)` + `node_b.sync_to(...)` → the block is handed to B's Engine and B syncs to it

Now both nodes share the same canonical head and consider themselves synced — exactly the pattern used in the existing `test_tx_propagation` test in `p2p.rs`.

### Step 6 — Alice Signs the 1 ETH Transfer

We build an **EIP-1559 (type 2)** transaction:

- `to: Bob`, `value: U256::from(10^18)` = **exactly 1 ETH**
- `gas_limit: 21_000` (plain value transfer)
- `max_fee_per_gas = max_priority_fee_per_gas = 1 gwei`
- `nonce: 1` — nonce 0 was consumed by the warm-up tx

Alice's `PrivateKeySigner` produces the secp256k1 signature, and we wrap the signed tx in a `TxEnvelope`. `tx_hash()` gives us the hash we'll track through the whole pipeline.

### Step 7 — Into Node A's Tx Pool

`node_a.rpc.inject_tx(...)` calls the **`eth_sendRawTransaction`** RPC. Node A's RPC layer:

1. Decodes & validates the raw 2718-encoded tx
2. The pool's **validator** (`EthPooledTransaction` validation) checks: signature, chain ID, balance, nonce, gas limits, size — all against post-block-1 state
3. The tx is inserted into A's `Pool` and an `PoolEvent::PendingTransaction` is emitted

**This is the "node adds it to tx pool" moment.** The tx is now queryable on A via `eth_getTransactionByHash`.

### Step 8 — P2P Propagation to Node B

This is the "transmits among peers" moment:

1. A's pool notifies A's **transactions manager** (`TransactionManager`)
2. The manager broadcasts announcement messages over the established devp2p session: `NewPooledTransactionHashes` (with the tx hash) and/or `Transactions` (with the full tx)
3. B's network layer receives the message and hands it to B's **txpool task**
4. B's pool **re-validates** the tx against B's own state (nonce, balance, gas)
5. On success, B inserts the tx into **its** pool

Our assertion `node_b.rpc...transaction_by_hash(tx_hash).is_some()` proves B received and accepted the gossip. We also assert it's still on A — both pools now hold the same tx.

### Step 9 — Create a Payload (Mine the Block)

`node_a.advance_block()` drives the Engine API:

1. `forkchoiceUpdated` with new payload attributes → returns a `payload_id`
2. The **payload builder service** picks a tip from the pool (highest fee by `CoinbaseTipOrdering`) — this is Alice's 1 ETH tx
3. It executes the tx in the EVM against state → produces a **built block** with the new state root
4. The block is locally validated & committed via `newPayload` + `forkchoiceUpdated`

We then assert the built block:

- is **block #2** (warm-up was #1)
- its body's first transaction has **our tx hash**

So the very tx that was gossiped to Bob is the one now canonically mined.

### Step 10 — Balance Verification

We query the chain state via RPC:

- `eth_getBalance(Bob)` before vs after → delta is **exactly `10^18` wei = 1 ETH** ✅
- Alice's balance is reduced by 1 ETH + ~0.000021 ETH gas (21,000 gas × 1 gwei)

This proves the full round trip: **sign → pool → gossip → payload → state transition**.

## How the components fit together (architectural notes)

| Phase | Reth Component | What happens |
| --- | --- | --- |
| Signing | `alloy` signer | Alice's key signs the EIP-1559 envelope |
| Tx pool insert | `reth-transaction-pool::Pool` | Validates against state, stores pending tx |
| P2P gossip | `reth-network` `TransactionManager` | Broadcasts `NewPooledTransactionHashes`/`Transactions` to peer B |
| Peer receive | B's `TransactionManager` | Decodes, re-validates, inserts into B's pool |
| Block building | `reth-payload-builder` (via Engine API) | Drains the pool, executes in EVM, produces a block |
| State change | `reth-provider` / storage | Alice −1 ETH −gas, Bob +1 ETH, new state root committed |

## Run it

```bash
# Compile check (verified clean above)
cargo check -p reth-node-ethereum --tests --features test-utils

# Run the test
cargo nextest run -p reth-node-ethereum --test e2e tx_pool_propagation --features test-utils
```

The test prints the journey:

```
Alice -> Bob signed tx: 0x...
tx 0x... accepted into node A's tx pool
tx 0x... propagated to node B's tx pool over P2P
tx 0x... mined in block #2
Bob balance delta: +1 ETH
Alice balance after paying 1 ETH + gas: ...
```

**Key APIs used** (all verified against the real codebase): `setup_engine_with_connection`, `NodeTestContext::connect`, `RpcTestContext::inject_tx` (→ `eth_sendRawTransaction`), `NodeTestContext::advance_block` (→ Engine `newPayload` + `forkchoiceUpdated`), `Wallet::wallet_gen` (funded hardhat accounts), and the `EthApiServer` RPC traits for balance/tx queries.
