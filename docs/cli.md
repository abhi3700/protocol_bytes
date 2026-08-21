# Reth CLI

Explore the commands.

#### Run the node with dev-chain

```sh
reth node --dev --http
```

With custom data dir:

```sh
reth node --dev --http --datadir ./data
```

#### Stop the node first, then drop the dev-chain DB

> reth db drop

```sh
reth db drop --chain dev --force
reth node --dev --http
```

With custom data dir:

> reth db drop deletes database entries; it isn’t necessarily equivalent to removing the entire custom datadir. \
> Since ./alice is a disposable dev-node directory, to completely reset it: `$ rm -rf ./data`.

```sh
reth db --datadir ./data --chain dev drop --force
reth node --dev --http --datadir ./data
```
