In reth, you normally do **not** manually create an execution payload for “Alice sends Bob 10 tokens.” You submit a transaction to reth; then the payload builder/miner/consensus layer builds a block payload that includes it.

For a local example:

```bash
reth node --dev --http
```

`--dev` prefunds 20 accounts from the mnemonic `test test test test test test test test test test test junk` with 10,000 ETH each.

Native ETH transfer, Alice -> Bob:

```bash
cast send \
  --rpc-url http://127.0.0.1:8545 \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  0x70997970C51812dc3A010C7d01b50e0d17dc79C8 \
  --value 10ether
```

Equivalent JSON-RPC using the dev signer:

```bash
curl -s http://127.0.0.1:8545 \
  -H 'content-type: application/json' \
  -d '{
    "jsonrpc":"2.0",
    "id":1,
    "method":"eth_sendTransaction",
    "params":[{
      "from":"0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
      "to":"0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
      "value":"0x8ac7230489e80000"
    }]
  }'
```

`0x8ac7230489e80000` is `10 * 10^18` wei.

<details>
<summary>On reth terminal:</summary>

```sh
2026-07-29T15:28:54.628576Z  INFO New payload job created id=0x9d0cd7f377cb9633 parent=0x683713729fcb72be6f3d8b88c8cda3e10569d73b9640d3bf6f5184d94bd97616
2026-07-29T15:28:54.629047Z  INFO Received block from consensus engine number=1 hash=0x7fbd345af68311adc7eaa8f827402c2d762f06f4add22d7f05fac13da6c21fc5
2026-07-29T15:28:54.631659Z  INFO State root task finished state_root=0x76a12748a24c23cd2e143a33bfd96455c53e53ab749ee9cc56e2ca88dc524f98 elapsed=998.583µs
2026-07-29T15:28:54.631722Z  INFO Block added to canonical chain number=1 hash=0x7fbd345af68311adc7eaa8f827402c2d762f06f4add22d7f05fac13da6c21fc5 peers=0 txs=1 gas_used=21.00Kgas gas_throughput=7.95Mgas/second gas_limit=30.03Mgas full=0.1% base_fee=0.88Gwei blobs=0 excess_blobs=0 elapsed=2.639875ms
2026-07-29T15:28:54.909364Z  INFO Canonical chain committed number=1 hash=0x7fbd345af68311adc7eaa8f827402c2d762f06f4add22d7f05fac13da6c21fc5 elapsed=24.292µs
2026-07-29T15:29:35.884886Z  INFO Status connected_peers=0 latest_block=1
```

</details>

---

For an ERC-20 token transfer instead:

```bash
cast send \
  --rpc-url http://127.0.0.1:8545 \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  <TOKEN_CONTRACT_ADDRESS> \
  "transfer(address,uint256)" \
  0x70997970C51812dc3A010C7d01b50e0d17dc79C8 \
  10000000000000000000
```

That assumes the token uses 18 decimals. The token contract must already exist.

If by “payload” you specifically mean **Engine API execution payload**, the flow is:

1. Send Alice’s transaction to reth via `eth_sendRawTransaction` / `eth_sendTransaction`.
2. The consensus layer calls `engine_forkchoiceUpdatedV*` with payload attributes.
3. Reth builds a payload from the txpool and returns a `payloadId`.
4. The consensus layer calls `engine_getPayloadV*` to retrieve the execution payload.

In `--dev`, reth’s local miner handles this for you automatically. Relevant local source: [dev prefunding/signers](reth/crates/node/core/src/args/dev.rs:18), [dev signer registration](reth/crates/node/builder/src/rpc.rs:1146), [eth_sendTransaction](reth/crates/rpc/rpc-eth-api/src/core.rs:906), [engine_getPayload/forkchoice handlers](reth/crates/rpc/rpc-engine-api/src/engine_api.rs:1268).
