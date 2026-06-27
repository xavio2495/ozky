# Privacy model

## What is hidden

Inside the shielded pool, transfers hide **amount, sender, and receiver**. On-chain an observer
sees only indistinguishable commitments and nullifiers — not who paid whom, or how much.

Selective disclosure is **opt-in and scoped**: you choose to share a view key for one account,
asset, and epoch. There is no public ledger data that reveals your activity.

## Where privacy ends — the edges

Privacy is strongest **inside** the pool. It weakens at public boundaries:

- **Deposits and withdrawals** are public Stellar transactions. Timing and amount at the edge can
  correlate a withdrawal with a later deposit.
- **Swaps via the Stellar DEX** and **bridges** are public legs — routing value through them
  leaks the graph by timing and amount.

ozky surfaces this honestly: edge actions are labelled, and a **denomination / timing policy**
(standard amounts, jittered timing, one shared anonymity set) reduces correlation. The UI frames
it as _speed vs. privacy_ — "Instant (standard amounts)" vs "Maximum privacy (may take a few
minutes)" — not as an on-chain flag that partitions users.

## Trust assumptions (testnet)

- The verifying key is developer-controlled on testnet; this becomes governance/multisig or
  immutability before mainnet.
- A pre-funded **relayer** pays XLM fees so you never touch a public XLM account.
- The **indexer** is a speed/availability layer only — funds are always recoverable from on-chain
  data alone, even if it goes down.

> ozky is unaudited. These are deliberate testnet simplifications, each flagged to be revisited
> before mainnet.
