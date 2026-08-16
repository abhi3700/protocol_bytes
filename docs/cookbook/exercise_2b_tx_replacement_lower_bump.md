# Exercise 2b: Lower the Txpool Price Bump to Accept a Small Replacement

## Background

The previous exercise demonstrated an important distinction:

```
Ethereum transaction semantics
        vs
Reth transaction-pool policy
```

Ethereum defines transactions, nonces, signatures, fee fields, and execution behavior.

But the decision:

```
"How much more expensive must a replacement transaction be?"
```

is largely a mempool policy decision.

Reth exposes this through its transaction-pool configuration.

Conceptually:

```
NodeConfig
    │
    └── txpool
          │
          └── price_bump
```

The default replacement threshold is around:

```
10%
```

But we can lower it before starting the node:

```
config.txpool.price_bump = 1;
```

which means:

```
1%
```

Now the same transaction that would previously have been rejected can become a valid replacement with only a small fee increase.

This exercise demonstrates how node configuration changes transaction-pool behavior.

Important Architectural Detail

The transaction pool is created as part of node construction.

Conceptually:

```
NodeConfig
    │
    ▼
NodeBuilder
    │
    ▼
TransactionPool
    │
    ▼
running node
```

Therefore, doing this after startup:

```
node.inner.pool().config()
```

only gives you access to the pool’s effective configuration.

It is not the correct place to change the replacement policy.

Instead, configure the node before launch using:

```
E2ETestSetupBuilder
```

and:

```
.with_node_config_modifier(...)
```

## Problem

Create a single Reth node where the replacement threshold is reduced from the default value to:

```
1%
```

Then:

1. Configure txpool.price_bump = 1 before starting the node.
2. Verify that the running pool actually uses a 1% threshold.
3. Send an original transaction from Alice with nonce = 0.
4. Verify that it appears in the pending pool.
5. Construct a replacement transaction with:
    * the same sender,
    * the same nonce,
    * approximately 1% higher max_fee_per_gas,
    * approximately 1% higher max_priority_fee_per_gas.
6. Submit the replacement.
7. Verify that no replacement transaction underpriced error occurs.
8. Verify that the original transaction disappears from Alice’s pending transactions.
9. Verify that exactly one transaction remains and its hash belongs to the replacement.

## Solution

[Commit](https://github.com/abhi3700/reth/commit/b44273769a2ee0c0c3d2709e4c4ecea26463c310)

```rust
//! Exercise 2b:
//! Lower Reth's transaction replacement threshold to 1%.
//!
//! ```
//! cargo test --package reth-node-ethereum --test e2e -- exercise_2b_tx_replacement_custom_bump::exercise_2b_tx_replacement_custom_bump --exact --nocapture --include-ignored
//! ```

use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_eips::Encodable2718;
use alloy_network::TxSignerSync;
use alloy_primitives::{Address, TxKind, U256};
use reth_chainspec::{ChainSpecBuilder, EthChainSpec};
use reth_e2e_test_utils::{wallet::Wallet, E2ETestSetupBuilder};
use reth_node_ethereum::EthereumNode;
use reth_transaction_pool::TransactionPool;
use revm::primitives::ONE_ETHER;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn exercise_2b_tx_replacement_custom_bump() -> eyre::Result<()> {
    reth_tracing::init_test_tracing();
    let chain_spec = Arc::new(
        ChainSpecBuilder::mainnet()
            .genesis(serde_json::from_str(include_str!("../assets/genesis.json")).unwrap())
            .cancun_activated()
            .prague_activated()
            .build(),
    );
    let chain_id = chain_spec.chain_id();
    // --------------------------------------------------
    // 1. Configure the node before startup
    // --------------------------------------------------
    let (mut nodes, _) = E2ETestSetupBuilder::<EthereumNode, _>::new(
        1,
        chain_spec.clone(),
        crate::utils::eth_payload_attributes,
    )
    .with_node_config_modifier(|mut config| {
        // Default is approximately 10%.
        //
        // Lower the replacement threshold to 1%.
        config.txpool.price_bump = 1;
        config
    })
    .build()
    .await?;
    let node = nodes.remove(0);
    // --------------------------------------------------
    // 2. Inspect the effective pool configuration
    // --------------------------------------------------
    let price_bumps = node.inner.pool().config().price_bumps;
    assert_eq!(price_bumps.default_price_bump, 1);
    let initial_base_fee =
        node.inner.chain_spec().initial_base_fee().expect("Must have initial base fee") as u128;
    let mut wallets = Wallet::new(1).with_chain_id(chain_id).wallet_gen();
    let alice = wallets.remove(0);
    // --------------------------------------------------
    // 3. Original transaction
    // --------------------------------------------------
    let original_fee = initial_base_fee;
    let mut tx1 = TxEip1559 {
        chain_id,
        nonce: 0,
        gas_limit: 21_000,
        max_fee_per_gas: original_fee,
        max_priority_fee_per_gas: original_fee,
        to: TxKind::Call(Address::random()),
        value: U256::from(ONE_ETHER),
        ..Default::default()
    };
    let signature1 = alice.sign_transaction_sync(&mut tx1)?;
    let envelope1 = TxEnvelope::Eip1559(tx1.into_signed(signature1));
    let tx_hash1 = *envelope1.tx_hash();
    node.rpc.inject_tx(envelope1.encoded_2718().into()).await?;
    let pending = node.inner.pool().get_pending_transactions_by_sender(alice.address());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].hash(), &tx_hash1);
    // --------------------------------------------------
    // 4. Create a replacement only ~1% more expensive
    // --------------------------------------------------
    let replacement_fee = original_fee * 101 / 100;
    let mut tx2 = TxEip1559 {
        chain_id,
        // Same nonce is what makes tx2 a replacement.
        nonce: 0,
        gas_limit: 21_000,
        max_fee_per_gas: replacement_fee,
        max_priority_fee_per_gas: replacement_fee,
        to: TxKind::Call(Address::random()),
        value: U256::from(ONE_ETHER),
        ..Default::default()
    };
    let signature2 = alice.sign_transaction_sync(&mut tx2)?;
    let envelope2 = TxEnvelope::Eip1559(tx2.into_signed(signature2));
    let tx_hash2 = *envelope2.tx_hash();
    // --------------------------------------------------
    // 5. Submit replacement
    // --------------------------------------------------
    node.rpc.inject_tx(envelope2.encoded_2718().into()).await?;
    // If the pool were still using the default 10%
    // replacement policy, this call would have returned:
    //
    // "replacement transaction underpriced"
    // --------------------------------------------------
    // 6. Verify replacement
    // --------------------------------------------------
    let pending = node.inner.pool().get_pending_transactions_by_sender(alice.address());
    assert_eq!(pending.len(), 1, "There should still be only one tx for nonce 0");
    assert_eq!(pending[0].hash(), &tx_hash2, "The replacement should now occupy nonce 0");
    assert_ne!(tx_hash1, tx_hash2);
    println!(
        "✅ Custom 1% threshold accepted replacement\n\
         original:    {tx_hash1}\n\
         replacement: {tx_hash2}"
    );
    Ok(())
}
```

Why the Same Replacement Behaves Differently

Exercise 2a:

```
Pool configuration
price bump = 10%
tx1
fee = 100
tx2
fee = 101
required ≈ 110
101 < 110
       ↓
replacement transaction underpriced
```

Exercise 2b:

```
Pool configuration
price bump = 1%
tx1
fee = 100
tx2
fee = 101
required ≈ 101
101 >= 101
       ↓
replacement accepted
```

The transaction itself did not gain any special capability.

What changed was the node’s admission policy.

What Reth is Conceptually Doing

When tx2 enters the pool:

```
                      tx2
                       │
                       ▼
                transaction pool
                       │
                       ▼
              sender + nonce lookup
                       │
             ┌─────────┴─────────┐
             │                   │
      nonce slot empty      tx1 already exists
             │                   │
             ▼                   ▼
         insert tx2         replacement check
                                 │
                                 ▼
                         compare fee fields
                                 │
                      ┌──────────┴──────────┐
                      │                     │
                bump sufficient       bump too small
                      │                     │
                      ▼                     ▼
                 remove tx1              reject tx2
                 insert tx2
```

With:

```
config.txpool.price_bump = 1;
```

we are changing the condition at:

```
"bump sufficient?"
```

rather than changing the overall replacement mechanism.

## Key Takeaways

The exercise-2a teaches that a replacement transaction is not simply a transaction with the same nonce. It must also satisfy the node’s fee-bump policy.

The exercise-2b reveals the deeper architectural point: the replacement threshold is txpool configuration, and Reth allows it to be customized during node construction.

So you can think of the complete path as:

```
NodeConfig
   │
   └── txpool.price_bump
              │
              ▼
       TransactionPool
              │
              ▼
 transaction arrives
              │
              ▼
 same sender + nonce?
              │
              ▼
 replacement candidate
              │
              ▼
 sufficient fee bump?
       │              │
      yes             no
       │              │
       ▼              ▼
   replace tx     underpriced
```

One useful follow-up exercise after these two would be to use two connected nodes with different price_bump policies. Then the same replacement transaction could be accepted by Node A but rejected by Node B, which is a great way to understand that mempool contents are not necessarily identical across Ethereum nodes.
