//! Example: Handling basefee constraint violations in `reth_transaction_pool::Pool`.
//!
//! Diagram:
//!
//! This example demonstrates how EIP-1559 transactions are classified by the
//! transaction pool depending on the current `pending_basefee` in block info.
//!
//! It walks through three simulated block states:
//! - block `#100`: base fee = `7`
//! - block `#101`: base fee = `10`
//! - block `#102`: base fee = `8`
//!
//! The example shows three important behaviors:
//! 1. A transaction whose `max_fee_per_gas` is below the pool's minimum acceptable protocol fee cap
//!    is rejected on insertion.
//! 2. A transaction whose fee cap is valid for the protocol, but still below the current
//!    `pending_basefee`, is accepted into the pool and placed into the `basefee` subpool instead of
//!    the `pending` subpool.
//! 3. When the block base fee drops, previously parked `basefee` transactions can become
//!    immediately executable and move into the `pending` subpool.
//!
//! In short, this example is useful for understanding the difference between:
//! - outright pool rejection, and
//! - accepted-but-not-yet-pending transactions.

use reth_ethereum::pool::{
	Pool, PoolConfig, PoolSize, TransactionPool, TransactionPoolExt,
	blobstore::InMemoryBlobStore,
	test_utils::{MockOrdering, MockTransaction, OkValidator},
};

/// Runs a small scenario against the mock transaction pool and asserts how the
/// pool reclassifies transactions as the simulated block base fee changes.
#[tokio::main]
async fn main() {
	// Create a pool backed by test-only components:
	// - `OkValidator` accepts mock transactions
	// - `MockOrdering` provides deterministic ordering behavior
	// - `InMemoryBlobStore` stores blob data in memory
	let pool = Pool::new(
		OkValidator::default(),
		MockOrdering::default(),
		InMemoryBlobStore::default(),
		PoolConfig::default(),
	);

	// Start from the pool's current block context and mutate it to simulate new blocks.
	let mut info = pool.block_info();

	//====== After block #100 ==========
	// Simulate block #100 where the next block's base fee is 7.
	info.pending_basefee = 7;
	assert_eq!(info.pending_basefee, 7);

	// Default mock EIP-1559 transaction has `max_fee_per_gas = 7`, so it is
	// eligible for the current base fee and should enter the pending subpool.
	let tx = MockTransaction::eip1559();
	assert_eq!(tx.get_max_fee(), Some(7));

	let res = pool.add_external_transaction(tx).await;
	assert!(res.is_ok());

	// Lower the fee cap to 6. This is below the pool's minimum acceptable fee
	// cap in this setup, so insertion should fail instead of parking the tx.
	let mut tx = MockTransaction::eip1559();
	tx.set_max_fee(6);
	assert_eq!(tx.get_max_fee(), Some(6));

	let res = pool.add_external_transaction(tx).await;
	assert!(res.is_err());

	// Only the first transaction remains pending.
	assert_eq!(pool.pending_transactions().len(), 1);

	//====== After block #101 ==========

	// Simulate block #101 with a higher base fee of 10 and update the pool.
	info.pending_basefee = 10;
	pool.set_block_info(info);

	// Fee cap 9 is valid enough to be accepted by the pool, but it is below the
	// current pending base fee (10), so it should land in the `basefee` subpool.
	let mut tx = MockTransaction::eip1559();
	tx.set_max_fee(9);
	assert_eq!(tx.get_max_fee(), Some(9));

	let res = pool.add_external_transaction(tx).await;
	assert!(res.is_ok());

	// Fee cap 11 clears the current base fee (10), so this one should be
	// immediately pending.
	let mut tx = MockTransaction::eip1559();
	tx.set_max_fee(11);
	assert_eq!(tx.get_max_fee(), Some(11));

	let res = pool.add_external_transaction(tx).await;
	assert!(res.is_ok());

	// Expect one executable tx in `pending` and two parked in `basefee`:
	// - the earlier tx with fee cap 7
	// - the newly inserted tx with fee cap 9
	let PoolSize { pending, basefee, queued, .. } = pool.pool_size();
	assert_eq!(pending, 1);
	assert_eq!(queued, 0);
	assert_eq!(basefee, 2);

	//====== After block #102 ==========
	// Simulate block #102 where the base fee drops to 8.
	// This should make the previously parked fee-cap-9 transaction executable.
	info.pending_basefee = 8;
	pool.set_block_info(info);

	// Insert another tx with fee cap 9. Under base fee 8, it should now be
	// directly pending.
	let mut tx = MockTransaction::eip1559();
	tx.set_max_fee(9);
	assert_eq!(tx.get_max_fee(), Some(9));

	let res = pool.add_external_transaction(tx).await;
	assert!(res.is_ok());
	// After the base fee drop:
	// - the old fee-cap-9 tx should have moved from `basefee` to `pending`
	// - the new fee-cap-9 tx should also be pending immediately
	// - only the old fee-cap-7 tx should remain in `basefee`
	let PoolSize { pending, basefee, queued, .. } = pool.pool_size();
	assert_eq!(pending, 3);
	assert_eq!(queued, 0);
	assert_eq!(basefee, 1);
}
