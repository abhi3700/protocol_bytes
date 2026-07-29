# Reth CLI

Explore the commands.

#### Run the node with dev-chain

```sh
reth node --dev --http
```

With custom data dir:

```sh
reth node --dev --http --datadir ./reth-dev-data
```

#### Stop the node first, then drop the dev-chain DB

```sh
reth db drop --chain dev --force
reth node --dev --http
```

With custom data dir:

```sh
reth db drop --chain dev --datadir ./reth-dev-data --force
reth node --dev --http --datadir ./reth-dev-data
```
