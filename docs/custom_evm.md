# Custom EVM

## Background

Ideally we could run a Anvil node. But the following modifications won't be possible. And that's why we use reth.

- Add new precompile at a contract address with custom runtime bytecode.
- Customize `base_fee_per_gas`.
- Add a new EOA with a pre-funded balance at genesis.

## Run

```sh
cargo r --example custom_evm
```

## Access

We use Foundry's `cast` CLI tool to connect to running node.

### get block

```sh
$ cast block                                                                                     ⏎


baseFeePerGas        1000000001
difficulty           0
extraData            0x
gasLimit             0
gasUsed              0
hash                 0x2206
...
...
```

### get contract | code, size & more

```sh
$ cast code 0x0000000000000000000000000000000000000999
0x6000546001018060005560005260206000f3

$ cast codesize 0x0000000000000000000000000000000000000999                                  
18

$ cast call 0x0000000000000000000000000000000000000999
0x

$ cast codehash 0x0000000000000000000000000000000000000999
0xa77ddcd430e0b8d87debfbcefb2bd7752a3410ae5a798a8c5d6f207f7fa2a3d9
```

### get EOA

```sh
# eth balance
$ cast balance 0x61C358A8d071451e16eDbA56e9fBF40fE6974B19                                
1000000000000000000
```
