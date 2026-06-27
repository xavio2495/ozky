# Overview

ozky is a native desktop wallet for **fully shielded stablecoin payments on Stellar**.
Amount, sender, and receiver are private on-chain — by default.

Under the hood it is a **UTXO shielded pool**: your balance is a set of private notes,
spending a note proves Merkle membership and publishes a nullifier, and an
**ASP compliance layer** keeps shielded funds provably clean without revealing them.
Heavy cryptography — proving, note scanning, encryption, signing — runs in a native Rust
core, off the UI thread.

> **Status:** ozky is testnet-first and **unaudited**. Use testnet funds only. An independent
> audit is required before mainnet.

## Where to start

- **[Concepts](/docs/concepts)** — notes, commitments, nullifiers, Merkle tree, epochs.
- **[Privacy model](/docs/privacy)** — what is hidden, and where privacy ends.
- **[Features](/docs/features)** — send, deposit/withdraw, split, escrow, channels, payroll.
- **[Compliance & disclosure](/docs/compliance)** — ASP membership and scoped view keys.

New here? The fastest path is the **[Quickstart](/quickstart)**.
