# Exercise 4a: Contract Deploy, Call, and State Verification

## Background

So far, most of the transactions in the exercises have been simple value transfers between externally owned accounts.

Smart-contract transactions introduce another important part of an Ethereum execution client:

```text
signed transaction
       │
       ▼
    Reth txpool
       │
       ▼
 transaction included
    in a block
       │
       ▼
       EVM
       │
       ▼
contract creation / execution
       │
       ▼
state changes committed
```

Reth is responsible for executing Ethereum transactions using the EVM and committing the resulting state changes.

For a smart contract, there are three particularly important operations to understand:

```text
contract deployment

state-changing contract call

read-only contract call
```

This exercise walks through all three.

---

## Contract Deployment

A normal ETH transfer contains a destination address:

```text
Alice
  │
  │ transaction
  │ to = Bob
  ▼
 Bob
```

A contract deployment is different.

The transaction does not target an existing account:

```text
Alice
  │
  │ contract creation transaction
  │ to = CREATE
  │ input = init bytecode
  ▼
 EVM
```

The EVM executes the contract's **creation bytecode**.

That creation code performs initialization and eventually returns the contract's **runtime bytecode**.

Conceptually:

```text
deployment transaction
        │
        ▼
 creation bytecode
        │
        ▼
      EVM
        │
        ├── execute constructor
        │
        ├── modify storage
        │
        └── return runtime bytecode
                │
                ▼
        contract account created
```

In this exercise, the constructor receives:

```solidity
constructor(uint256 x) {
    stored = x;
}
```

and we deploy with:

```text
x = 42
```

Therefore the deployment changes Ethereum state in two ways:

```text
contract account
    │
    ├── code    = Storage runtime bytecode
    │
    └── slot 0  = 42
```

---

## Contract Storage

The contract contains:

```solidity
uint256 public stored;
```

Because `stored` is the first state variable, Solidity assigns it to storage slot:

```text
slot 0
```

After deployment:

```text
Storage contract

storage
────────────
slot 0 -> 42
```

Later we will execute:

```solidity
set(99);
```

which changes the state to:

```text
Storage contract

storage
────────────
slot 0 -> 99
```

This gives us a useful opportunity to verify the result directly through Reth's RPC layer using:

```text
eth_getStorageAt
```

---

## State-Changing Calls vs `eth_call`

The exercise also demonstrates two very different ways of executing contract code.

### State-changing transaction

Calling:

```solidity
set(99)
```

requires a real Ethereum transaction:

```text
Alice
  │
  │ signed transaction
  │ calldata = set(99)
  ▼
txpool
  │
  ▼
block
  │
  ▼
 EVM
  │
  ▼
slot 0 = 99
```

Because Ethereum state changes, the transaction must:

```text
be signed
    +
enter the txpool
    +
be included in a block
    +
consume gas
```

### Read-only `eth_call`

Calling:

```solidity
get()
```

does not require a transaction to be mined.

Instead we can use:

```text
eth_call
```

Conceptually:

```text
RPC request
   │
   ▼
eth_call
   │
   ▼
execute get() against
existing Ethereum state
   │
   ▼
return ABI-encoded result
```

The EVM still executes the contract code, but the execution is simulated against the selected block state.

Nothing is committed to the blockchain.

This distinction is fundamental:

```text
send_transaction                 eth_call
────────────────                 ────────

signed                           unsigned request

txpool                           no txpool

included in block                no block inclusion

changes state                    state discarded

costs gas                        no ETH actually spent
```

---

## Problem

Create a single Reth node and exercise the complete lifecycle of a simple `Storage` smart contract.

Define the contract using `alloy_sol_types::sol!`:

```solidity
contract Storage {
    uint256 public stored;

    constructor(uint256 x) {
        stored = x;
    }

    function set(uint256 x) external {
        stored = x;
    }

    function get() public returns (uint256) {
        return stored;
    }
}
```

Then:

1. Start a Reth E2E node.
2. Create an Alloy provider connected to the node's HTTP RPC endpoint.
3. Deploy `Storage` with:

    ```text
    stored = 42
    ```

4. Mine the deployment transaction.
5. Obtain the deployed contract address from the transaction receipt.
6. Query the contract using `eth_getCode` and verify that runtime bytecode exists.
7. Call `get()` and verify that the constructor initialized the value to:

    ```text
    42
    ```

8. Send a state-changing transaction calling:

    ```text
    set(99)
    ```

9. Mine the setter transaction.
10. Verify that the transaction succeeded.
11. Call `get()` again and verify that it returns:

    ```text
    99
    ```

12. Query storage slot `0` directly using `eth_getStorageAt` and verify:

    ```text
    slot 0 = 99
    ```

13. Perform a raw `eth_call` for `get()`.
14. ABI-decode the returned bytes and verify that the result is also:

```text
99
```

The final contract state should therefore be:

```text
Storage contract
────────────────

code:
    non-empty runtime bytecode

storage:
    slot 0 -> 99

get():
    99
```

---

## Solution

[Commit](https://github.com/abhi3700/reth/commit/9fa561670d2038b7efaa14407483209232c40cc2)

```rust
//! Exercise 4a:
//! Deploy a smart contract, execute a state-changing contract call,
//! and verify the resulting Ethereum state.
//!
//! This exercise demonstrates the complete contract lifecycle:
//!
//! - deploy contract bytecode
//! - execute the constructor
//! - obtain the contract address from the receipt
//! - verify runtime bytecode with `eth_getCode`
//! - send a state-changing contract call
//! - verify storage with `eth_getStorageAt`
//! - execute a read-only call with `eth_call`
//!
//! ```
//! cargo test --package reth-node-ethereum --test e2e -- exercise_4a_contract_lifecycle::exercise_4a_contract_lifecycle --exact --nocapture --include-ignored
//! ```

use alloy_network::TransactionBuilder;
use alloy_primitives::{Bytes, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types_eth::TransactionRequest;
use alloy_sol_types::{sol, SolCall};
use reth_chainspec::{ChainSpecBuilder, MAINNET};
use reth_e2e_test_utils::setup_engine;
use reth_node_ethereum::EthereumNode;
use revm::primitives::ONE_GWEI;
use std::sync::Arc;

sol! {
    #[sol(
        rpc,
        bytecode = "0x6080604052348015600e575f5ffd5b5060405161010a38038061010a833981016040819052602b916031565b5f556047565b5f602082840312156040575f5ffd5b5051919050565b60b7806100535f395ff3fe6080604052348015600e575f5ffd5b5060043610603a575f3560e01c806360fe47b114603e5780636d4ce63c14604f578063c2985578146064575b5f5ffd5b604d6049366004606b565b5f55565b005b5f545b60405190815260200160405180910390f35b60525f5481565b5f60208284031215607a575f5ffd5b503591905056fea264697066735822122040e30ee6d93bebf71509d5b17e11c509362ae2050b8556f774704784b8391d2e64736f6c634300081b0033"
    )]
    contract Storage {
        uint256 public stored;

        constructor(uint256 x) {
            stored = x;
        }

        function set(uint256 x) external {
            stored = x;
        }

        function get() public returns (uint256) {
            return stored;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn exercise_4a_contract_lifecycle() -> eyre::Result<()> {
    reth_tracing::init_test_tracing();

    let chain_spec = Arc::new(
        ChainSpecBuilder::default()
            .chain(MAINNET.chain)
            .genesis(
                serde_json::from_str(include_str!("../assets/genesis.json"))
                    .unwrap(),
            )
            .cancun_activated()
            .prague_activated()
            .build(),
    );

    let (mut nodes, wallet) = setup_engine::<EthereumNode>(
        1,
        chain_spec,
        false,
        Default::default(),
        crate::utils::eth_payload_attributes,
    )
    .await?;

    let mut node = nodes.pop().unwrap();

    let alice = wallet.inner;

    let provider = ProviderBuilder::new()
        .wallet(alice.clone())
        .connect_http(node.rpc_url());

    // --------------------------------------------------
    // 1. Deploy Storage with stored = 42
    // --------------------------------------------------
    //
    // `deploy_builder` creates the contract-creation
    // transaction:
    //
    //     creation bytecode
    //            +
    //     ABI-encoded constructor argument: 42
    //
    // The transaction is sent to Reth but the contract
    // does not become part of canonical state until we
    // mine the transaction.
    // --------------------------------------------------

    let deploy_pending = Storage::deploy_builder(
        &provider,
        U256::from(42),
    )
    .send()
    .await?;

    // --------------------------------------------------
    // 2. Mine the deployment transaction
    // --------------------------------------------------

    node.advance_block().await?;

    let deploy_receipt = deploy_pending.get_receipt().await?;

    assert!(
        deploy_receipt.status(),
        "Contract deployment must succeed"
    );

    // --------------------------------------------------
    // 3. Obtain the newly created contract address
    // --------------------------------------------------
    //
    // A successful contract-creation receipt contains the
    // address of the newly deployed contract.
    // --------------------------------------------------

    let contract = deploy_receipt
        .contract_address
        .expect("A contract must be deployed");

    println!("Deployed Storage at {contract}");

    // --------------------------------------------------
    // 4. Verify runtime bytecode with eth_getCode
    // --------------------------------------------------
    //
    // During deployment, the EVM executes the creation
    // bytecode and stores the returned runtime bytecode
    // under the new contract account.
    //
    // Therefore:
    //
    //     eth_getCode(contract)
    //
    // must return non-empty bytes.
    // --------------------------------------------------

    let code = provider.get_code_at(contract).await?;

    assert!(
        !code.is_empty(),
        "Contract runtime bytecode should exist"
    );

    println!("Contract code is {} bytes", code.len());

    // --------------------------------------------------
    // 5. Verify constructor initialized slot 0 to 42
    // --------------------------------------------------
    //
    // The constructor executed:
    //
    //     stored = 42;
    //
    // `stored` is the first uint256 state variable and
    // therefore occupies storage slot 0.
    // --------------------------------------------------

    let deployed_contract = Storage::new(
        contract,
        provider.clone(),
    );

    let value = deployed_contract.get().call().await?;

    assert_eq!(
        value,
        U256::from(42),
        "Constructor should initialize stored to 42"
    );

    let slot0_before = provider
        .get_storage_at(contract, U256::ZERO)
        .await?;

    assert_eq!(
        slot0_before,
        U256::from(42),
        "Storage slot 0 should initially contain 42"
    );

    // --------------------------------------------------
    // 6. Build calldata for set(99)
    // --------------------------------------------------
    //
    // ABI encoding produces:
    //
    //     function selector
    //            +
    //     encoded uint256(99)
    //
    // This becomes the transaction's input/calldata.
    // --------------------------------------------------

    let set_call = Storage::setCall {
        x: U256::from(99),
    };

    // --------------------------------------------------
    // 7. Send the state-changing contract transaction
    // --------------------------------------------------
    //
    // Unlike `eth_call`, set(99) modifies Ethereum state.
    //
    // Therefore it must be:
    //
    //     signed
    //       ↓
    //     submitted
    //       ↓
    //     txpool
    //       ↓
    //     included in a block
    //       ↓
    //     executed by the EVM
    // --------------------------------------------------

    let set_pending = provider
        .send_transaction(
            TransactionRequest::default()
                .with_to(contract)
                .with_input(set_call.abi_encode())
                .with_gas_limit(100_000)
                .with_max_fee_per_gas(20_000_000_000)
                .with_max_priority_fee_per_gas(ONE_GWEI),
        )
        .await?;

    // --------------------------------------------------
    // 8. Mine the setter transaction
    // --------------------------------------------------

    node.advance_block().await?;

    let set_receipt = set_pending.get_receipt().await?;

    assert!(
        set_receipt.status(),
        "set(99) transaction must succeed"
    );

    // --------------------------------------------------
    // 9. Verify get() now returns 99
    // --------------------------------------------------

    let value = deployed_contract.get().call().await?;

    assert_eq!(
        value,
        U256::from(99),
        "get() should return the updated value"
    );

    // --------------------------------------------------
    // 10. Verify raw contract storage
    // --------------------------------------------------
    //
    // Solidity:
    //
    //     uint256 public stored;
    //
    // occupies slot 0.
    //
    // Therefore:
    //
    //     eth_getStorageAt(contract, 0)
    //
    // should now return 99.
    // --------------------------------------------------

    let slot0_after = provider
        .get_storage_at(contract, U256::ZERO)
        .await?;

    assert_eq!(
        slot0_after,
        U256::from(99),
        "Storage slot 0 should contain 99"
    );

    // --------------------------------------------------
    // 11. Perform get() manually using raw eth_call
    // --------------------------------------------------
    //
    // Until now Alloy's generated contract binding handled
    // calldata encoding and result decoding for us.
    //
    // Here we manually:
    //
    //     ABI encode get()
    //            ↓
    //         eth_call
    //            ↓
    //     receive raw bytes
    //            ↓
    //     ABI decode uint256
    //
    // This exposes what the generated binding is doing
    // underneath.
    // --------------------------------------------------

    let get_call = Storage::getCall {};

    let result: Bytes = provider
        .raw_request(
            "eth_call".into(),
            (
                TransactionRequest::default()
                    .with_to(contract)
                    .input(get_call.abi_encode().into()),
                "latest",
            ),
        )
        .await?;

    let decoded =
        Storage::getCall::abi_decode_returns(&result)?;

    assert_eq!(
        decoded,
        U256::from(99),
        "eth_call get() should return 99"
    );

    println!(
        "✅ Contract lifecycle verified\n\
         Contract: {contract}\n\
         Initial value: 42\n\
         Updated value: 99\n\
         Storage slot 0: {slot0_after}"
    );

    Ok(())
}
```

## What Happens During Deployment

The first important operation is:

```rust
let deploy_pending = Storage::deploy_builder(
    &provider,
    U256::from(42),
)
.send()
.await?;
```

The Alloy-generated binding combines:

```text
Storage creation bytecode
          +
ABI encoding of constructor argument 42
```

into a contract-creation transaction.

Conceptually:

```text
Alice
  │
  │ signed CREATE transaction
  │
  │ input =
  │   creation bytecode
  │   +
  │   constructor(42)
  ▼
Reth txpool
```

At this point the transaction exists in the node, but the contract has not yet become part of canonical state.

We then call:

```rust
node.advance_block().await?;
```

Now the transaction enters a block:

```text
txpool
  │
  ▼
payload builder
  │
  ▼
block
  │
  ▼
execution
```

During execution:

```text
creation transaction
       │
       ▼
      EVM
       │
       ├── run constructor
       │
       │      stored = 42
       │
       └── return runtime bytecode
                    │
                    ▼
             contract account
```

The resulting state is approximately:

```text
contract address
      │
      ├── nonce
      ├── balance
      ├── code hash ───> Storage runtime bytecode
      │
      └── storage
             │
             └── slot 0 -> 42
```

---

## Why We Get the Contract Address From the Receipt

A contract-creation transaction does not specify a normal recipient:

```text
to = CREATE
```

Instead the contract address is derived during creation.

After execution, the transaction receipt exposes:

```rust
deploy_receipt.contract_address
```

which gives us:

```text
Alice deployment tx
        │
        ▼
       EVM
        │
        ▼
new contract account
        │
        ▼
contract_address
```

The test verifies this with:

```rust
let contract = deploy_receipt
    .contract_address
    .expect("A contract must be deployed");
```

---

## Why `eth_getCode` Matters

The existence of a successful deployment receipt alone is not the strongest possible verification that a contract now exists.

We additionally ask the node:

```rust
let code = provider.get_code_at(contract).await?;
```

which corresponds conceptually to:

```text
eth_getCode(contract, latest)
```

For a normal EOA:

```text
code = 0x
```

For our deployed contract:

```text
code = 0x60806040...
```

Therefore:

```rust
assert!(!code.is_empty());
```

confirms that the canonical state contains runtime bytecode at the generated address.

The path through Reth is conceptually:

```text
eth_getCode
    │
    ▼
RPC
    │
    ▼
provider/state lookup
    │
    ▼
contract account
    │
    ▼
code hash
    │
    ▼
runtime bytecode
```

---

## Constructor Execution Changes Storage

Our constructor is:

```solidity
constructor(uint256 x) {
    stored = x;
}
```

and deployment passes:

```text
x = 42
```

Therefore contract creation does more than install code.

It also executes:

```text
SSTORE(slot 0, 42)
```

Conceptually:

```text
constructor(42)
      │
      ▼
    EVM
      │
      ▼
SSTORE
      │
      ▼
slot 0 = 42
```

This is why immediately after deployment:

```rust
let value = deployed_contract.get().call().await?;
```

returns:

```text
42
```

and:

```rust
provider
    .get_storage_at(contract, U256::ZERO)
```

also returns:

```text
42
```

The getter and the raw storage query are observing the same underlying Ethereum state through two different mechanisms.

---

## Calling `set(99)`

The setter is:

```solidity
function set(uint256 x) external {
    stored = x;
}
```

We first create its ABI-encoded calldata:

```rust
let set_call = Storage::setCall {
    x: U256::from(99),
};
```

followed by:

```rust
set_call.abi_encode()
```

The resulting transaction conceptually contains:

```text
to:
    Storage contract

input:
    selector(set(uint256))
        +
    ABI encoded 99
```

The transaction then follows the normal Ethereum transaction path:

```text
Alice
  │
  │ set(99)
  ▼
RPC
  │
  ▼
Reth transaction pool
  │
  ▼
payload builder
  │
  ▼
block
  │
  ▼
EVM
```

Inside EVM execution:

```text
calldata
   │
   ▼
function selector
   │
   ▼
set(uint256)
   │
   ▼
decode x = 99
   │
   ▼
SSTORE(0, 99)
```

After the block becomes canonical:

```text
before

slot 0
  │
  ▼
 42


set(99)
  │
  ▼
 EVM
  │
  ▼
SSTORE


after

slot 0
  │
  ▼
 99
```

---

## Verifying State Through `eth_getStorageAt`

After executing the setter, we directly query the contract's storage:

```rust
let slot0_after = provider
    .get_storage_at(contract, U256::ZERO)
    .await?;
```

This corresponds to:

```text
eth_getStorageAt(
    contract,
    slot = 0,
    latest
)
```

The result should be:

```text
99
```

This verification is especially useful because it does not depend on the contract's getter implementation.

We are querying Ethereum state directly.

Conceptually:

```text
get()
 │
 └── execute contract code
          │
          ▼
        SLOAD(0)


eth_getStorageAt
 │
 └── directly query state
          │
          ▼
        slot 0
```

Both should observe:

```text
99
```

---

## What `eth_call` Actually Does

The final part deliberately avoids Alloy's generated convenience method:

```rust
deployed_contract.get().call().await?
```

and performs the RPC manually.

First:

```rust
let get_call = Storage::getCall {};
```

Then:

```rust
get_call.abi_encode()
```

generates the calldata for:

```solidity
get()
```

The RPC request becomes conceptually:

```json
eth_call({
    "to": "<Storage address>",
    "data": "<get() selector>"
}, "latest")
```

Reth then executes the call against the latest state:

```text
eth_call
   │
   ▼
latest state
   │
   ▼
Storage runtime bytecode
   │
   ▼
EVM
   │
   ▼
get()
   │
   ▼
SLOAD(0)
   │
   ▼
99
   │
   ▼
ABI encode result
```

The RPC response is still raw bytes:

```text
0x0000000000000000000000000000000000000000000000000000000000000063
```

where:

```text
0x63 = 99
```

We decode it with:

```rust
Storage::getCall::abi_decode_returns(&result)?
```

and finally assert:

```rust
assert_eq!(decoded, U256::from(99));
```

---

## `eth_call` Does Not Modify State

An important mental model is that `eth_call` still uses EVM execution.

It is not simply reading some cached result.

```text
eth_call
   │
   ▼
load Ethereum state
   │
   ▼
create temporary EVM execution environment
   │
   ▼
execute contract bytecode
   │
   ▼
return output
   │
   ▼
DISCARD resulting state changes
```

By comparison, a transaction included in a block does:

```text
transaction
   │
   ▼
load Ethereum state
   │
   ▼
execute EVM
   │
   ▼
produce state changes
   │
   ▼
COMMIT changes
```

So the main distinction is not:

```text
EVM vs no EVM
```

Both can involve EVM execution.

The distinction is:

```text
transaction execution

EVM
 │
 ▼
state changes
 │
 ▼
COMMIT


eth_call

EVM
 │
 ▼
temporary state changes
 │
 ▼
DISCARD
```

---

## Where Reth Fits Into the Lifecycle

This exercise touches several different parts of a Reth node.

### Deployment transaction

```text
Alloy
  │
  ▼
Reth RPC
  │
  ▼
transaction pool
  │
  ▼
payload builder
  │
  ▼
EVM execution
  │
  ▼
database/state
```

### Setter transaction

```text
set(99)
   │
   ▼
RPC transaction submission
   │
   ▼
txpool
   │
   ▼
block
   │
   ▼
EVM
   │
   ▼
SSTORE(0, 99)
   │
   ▼
new canonical state
```

### Getter through `eth_call`

```text
get()
 │
 ▼
eth_call
 │
 ▼
RPC
 │
 ▼
state provider
 │
 ▼
EVM
 │
 ▼
SLOAD(0)
 │
 ▼
99
```

### Direct storage query

```text
eth_getStorageAt
       │
       ▼
      RPC
       │
       ▼
state provider
       │
       ▼
contract storage
       │
       ▼
slot 0 = 99
```

The exercise therefore crosses several important execution-client boundaries:

```text
RPC

txpool

block building

EVM execution

contract code

account storage

state providers
```

---

## The Alloy Contract Binding

The `sol!` macro gives us strongly typed Rust representations of the Solidity ABI.

For example:

```rust
Storage::setCall {
    x: U256::from(99),
}
```

represents:

```solidity
set(uint256)
```

and:

```rust
Storage::getCall {}
```

represents:

```solidity
get()
```

This means Alloy can handle:

```text
Rust values
    │
    ▼
ABI encoding
    │
    ▼
Ethereum calldata
```

and in the opposite direction:

```text
RPC response bytes
       │
       ▼
ABI decoding
       │
       ▼
typed Rust value
```

The generated:

```rust
Storage::new(contract, provider.clone())
```

binding makes this even more convenient:

```rust
deployed_contract.get().call().await?
```

Conceptually, however, it still performs the same operations demonstrated by our raw `eth_call`:

```text
construct calldata
       │
       ▼
eth_call
       │
       ▼
raw return bytes
       │
       ▼
ABI decode
       │
       ▼
U256
```

---

## Why the Bytecode Is Included in `sol!`

The Solidity source describes the contract ABI:

```solidity
contract Storage {
    ...
}
```

but deployment requires actual EVM creation bytecode.

Therefore the binding uses:

```rust
#[sol(
    rpc,
    bytecode = "0x..."
)]
```

The bytecode was compiled ahead of time from the Solidity contract.

Conceptually:

```text
Solidity source
      │
      ▼
     solc
      │
      ├── ABI
      │
      └── creation bytecode
```

`sol!` gives Rust code access to the ABI, while the `bytecode` attribute supplies the executable code required for deployment.

Without deployment bytecode:

```text
ABI available
     │
     ▼
can encode calls

BUT

no creation bytecode
     │
     ▼
cannot deploy contract
```

For E2E tests, embedding precompiled bytecode is useful because the test itself does not need to invoke `solc` at runtime.

---

## Complete Mental Model

The whole exercise can be represented as:

```text
                           Alice
                             │
                             │ deploy Storage(42)
                             ▼
                         Reth RPC
                             │
                             ▼
                          txpool
                             │
                             ▼
                         mine block
                             │
                             ▼
                            EVM
                             │
                 ┌───────────┴───────────┐
                 │                       │
                 ▼                       ▼
          constructor(42)         runtime bytecode
                 │                       │
                 ▼                       ▼
          SSTORE(0, 42)          contract account
                 │                       │
                 └───────────┬───────────┘
                             ▼

                        Storage contract

                       code  = non-empty
                       slot0 = 42
                             │
                             │
                             │ Alice sends set(99)
                             ▼
                          txpool
                             │
                             ▼
                         mine block
                             │
                             ▼
                            EVM
                             │
                             ▼
                         set(99)
                             │
                             ▼
                       SSTORE(0, 99)
                             │
                             ▼

                        Storage contract

                       code  = non-empty
                       slot0 = 99
                             │
              ┌──────────────┴──────────────┐
              │                             │
              ▼                             ▼

           get()                     eth_getStorageAt
              │                             │
              ▼                             ▼
          eth_call                        slot 0
              │                             │
              ▼                             ▼
             EVM                            99
              │
              ▼
          SLOAD(0)
              │
              ▼
             99
```

All three observations now agree:

```text
get()                 -> 99

eth_call get()        -> 99

eth_getStorageAt(0)   -> 99
```

---

## Key Takeaways

A contract deployment is an Ethereum transaction whose execution creates a new account containing runtime bytecode:

```text
creation transaction
        │
        ▼
       EVM
        │
        ├── constructor
        ├── initial storage
        └── runtime bytecode
```

The constructor itself is real EVM execution and can modify Ethereum state:

```text
Storage(42)
    │
    ▼
SSTORE(0, 42)
```

State-changing contract functions such as:

```solidity
set(99)
```

must be submitted as transactions and included in blocks:

```text
signed tx
   │
   ▼
txpool
   │
   ▼
block
   │
   ▼
EVM
   │
   ▼
state commit
```

Read-only contract execution through `eth_call` still executes EVM bytecode, but the resulting state is not committed:

```text
eth_call
   │
   ▼
EVM execution
   │
   ▼
return result
   │
   ▼
discard temporary state
```

`eth_getStorageAt` bypasses the Solidity getter and lets us directly inspect the contract's underlying storage:

```text
contract
   │
   ▼
slot 0
   │
   ▼
99
```

Therefore this exercise connects several important concepts into one lifecycle:

```text
Solidity ABI
     │
     ▼
Alloy
     │
     ▼
Reth RPC
     │
     ▼
txpool
     │
     ▼
block building
     │
     ▼
EVM execution
     │
     ▼
Ethereum state
     │
     ├── contract code
     └── contract storage
```

The final mental model is:

> **A smart contract is code and state stored inside Ethereum. Transactions execute that code and can permanently modify the state, while `eth_call` executes the same EVM code against existing state without committing the result.**
