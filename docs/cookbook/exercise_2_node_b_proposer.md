# Exercise 2: Node B Proposes the Second Transaction

## Background

This two-node derivative makes the handoff between proposers deterministic:

- Node A receives Alice's warm-up transaction and proposes block 1.
- Node B receives Alice's second transaction through its RPC interface.
- The transaction gossips from B back to A.
- Node B then proposes block 2 from its own txpool.
- Node A validates and canonicalizes B's payload.

"Node B sends the transaction" is convenient shorthand, but the precise roles are:

- **Alice** is the account that signs and sends value.
- **Node B** is the RPC ingress node and block-2 proposer.
- **Node A** is the peer that learns the transaction through devp2p gossip and later receives the completed execution payload.

There is no live beacon node or validator election in this E2E helper. Calling `advance_block()` on B represents B's local consensus-layer partner asking its execution layer to build, validate, and canonicalize the next payload.

As in Exercise 1, B's canonical head is updated by `submit_payload()` and `sync_to()`, but its local payload timestamp is not. Copy A's timestamp to B before B builds block 2.

## Task

1. Spawn nodes A and B without automatic connectivity, then connect them.
2. Submit Alice's nonce-0 warm-up transaction to A and let A propose block 1.
3. Submit and canonicalize block 1 on B.
4. Sign an Alice-to-Bob transfer with nonce 1 and submit it to node B.
5. Verify the transaction gossips from B to A.
6. Let node B propose block 2 and verify the block contains the transaction.
7. Submit and canonicalize block 2 on A.
8. Verify both nodes are at block 2, Alice's confirmed nonce is 2, and Bob received 1 ETH.

## Reference solution

```rust
//! Node A proposes the warm-up block, then node B receives the next tx and proposes block 2.

use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_eips::Encodable2718;
use alloy_network::TxSignerSync;
use alloy_primitives::{TxKind, U256};
use reth_chainspec::{ChainSpecBuilder, MAINNET};
use reth_e2e_test_utils::{
    setup_engine_with_connection, transaction::TransactionTestContext, wallet::Wallet,
    NodeHelperType,
};
use reth_node_ethereum::EthereumNode;
use reth_rpc_api::EthApiServer;
use revm::primitives::ONE_ETHER;
use std::{sync::Arc, time::Duration};

#[tokio::test(flavor = "multi_thread")]
async fn node_b_proposes_second_transaction() -> eyre::Result<()> {
    reth_tracing::init_test_tracing();

    let chain_spec = Arc::new(
        ChainSpecBuilder::default()
            .chain(MAINNET.chain)
            .genesis(serde_json::from_str(include_str!("../assets/genesis.json")).unwrap())
            .cancun_activated()
            .prague_activated()
            .build(),
    );
    let chain_id = chain_spec.chain().into();

    let mut wallets = Wallet::new(2).with_chain_id(chain_id).wallet_gen();
    let alice = wallets.remove(0);
    let bob = wallets.remove(0);

    let (mut nodes, _) = setup_engine_with_connection::<EthereumNode>(
        2,
        chain_spec,
        false,
        Default::default(),
        crate::utils::eth_payload_attributes,
        false,
    )
    .await?;
    let mut node_a: NodeHelperType<EthereumNode> = nodes.remove(0);
    let mut node_b: NodeHelperType<EthereumNode> = nodes.remove(0);
    node_a.connect(&mut node_b).await;

    // A receives Alice's nonce-0 transaction and proposes block 1.
    let seed_tx = TransactionTestContext::transfer_tx_bytes(chain_id, alice.clone()).await;
    node_a.rpc.inject_tx(seed_tx).await?;
    let warmup_payload = node_a.advance_block().await?;
    assert_eq!(warmup_payload.block().number, 1);

    node_b.submit_payload(warmup_payload.clone()).await?;
    node_b.sync_to(warmup_payload.block().hash()).await?;

    // B now has A's canonical head, but its payload helper still has its original timestamp.
    node_b.payload.timestamp = node_a.payload.timestamp;

    let bob_balance_before =
        node_a.rpc.inner.eth_api().balance(bob.address(), Default::default()).await?;

    // Alice signs nonce 1, while node B is the RPC ingress node.
    let mut tx = TxEip1559 {
        chain_id,
        nonce: 1,
        gas_limit: 21_000,
        max_fee_per_gas: 1_000_000_000,
        max_priority_fee_per_gas: 1_000_000_000,
        to: TxKind::Call(bob.address()),
        value: U256::from(ONE_ETHER),
        ..Default::default()
    };
    let signature = alice.sign_transaction_sync(&mut tx)?;
    let envelope = TxEnvelope::Eip1559(tx.into_signed(signature));
    let tx_hash = *envelope.tx_hash();

    node_b.rpc.inject_tx(envelope.encoded_2718().into()).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        node_b.rpc.inner.eth_api().transaction_by_hash(tx_hash).await?.is_some(),
        "node B should contain the locally submitted transaction"
    );
    assert!(
        node_a.rpc.inner.eth_api().transaction_by_hash(tx_hash).await?.is_some(),
        "node A should receive the transaction through gossip from B"
    );

    // B's local Engine API flow builds and canonicalizes block 2.
    let payload = node_b.advance_block().await?;
    let block = payload.block();
    assert_eq!(block.number, 2);
    let included_tx = block.body().transactions().next().expect("block 2 should contain a tx");
    assert_eq!(*included_tx.tx_hash(), tx_hash);

    // A receives B's completed payload; this is separate from tx gossip.
    node_a.submit_payload(payload.clone()).await?;
    node_a.sync_to(block.hash()).await?;

    assert_eq!(node_a.rpc.inner.eth_api().block_number()?, 2);
    assert_eq!(node_b.rpc.inner.eth_api().block_number()?, 2);
    assert_eq!(node_a.rpc.inner.eth_api().transaction_count(alice.address(), None).await?, 2);
    assert_eq!(node_b.rpc.inner.eth_api().transaction_count(alice.address(), None).await?, 2);

    let bob_balance_on_a =
        node_a.rpc.inner.eth_api().balance(bob.address(), Default::default()).await?;
    let bob_balance_on_b =
        node_b.rpc.inner.eth_api().balance(bob.address(), Default::default()).await?;
    assert_eq!(bob_balance_on_a - bob_balance_before, U256::from(ONE_ETHER));
    assert_eq!(bob_balance_on_b, bob_balance_on_a);

    Ok(())
}
```

To run it, save the code as `exercise_2_node_b_proposer.rs`, register
`mod exercise_2_node_b_proposer;` in `main.rs`, and run:

```bash
cargo test run -p reth-node-ethereum --test e2e node_b_proposes_second_transaction
```

[Commit](https://github.com/abhi3700/reth/commit/783368b693302ce19775ae7dfd6417b1b7237fc8)
