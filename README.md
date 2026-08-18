# Bitcoin Wallet in Rust — Part III

Companion code for part III of the [Let's Build a Bitcoin Wallet in Rust](https://medium.com/@gunnar.h.karlsson/lets-build-a-bitcoin-wallet-in-rust-part-iii-4fc873425910) tutorial series.

This part spends testnet BTC: it selects UTXOs, builds a legacy P2PKH transaction, signs it, and broadcasts it through a local Bitcoin Core node.

Requires Rust 1.85 or later and a Bitcoin Core testnet node listening on `http://127.0.0.1:18332`. Update the RPC username and password in `get_rpc_client()` to match your node.

## Run

Create a wallet if needed, then fund it from a faucet as in part II:

```bash
cargo run -- new
cargo run -- receive
cargo run -- balance
```

Send testnet BTC to another address:

```bash
cargo run -- send <recipient_address> 0.0001
```

This is testnet learning code. The private key is stored unencrypted in `wallet.json`, fee estimation is a fixed placeholder, and signing is simplified for the tutorial — do not use it on mainnet.

## License

MIT. See [LICENSE](LICENSE).
