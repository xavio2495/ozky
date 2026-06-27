# Features

## Core payments

- **Deposit / Withdraw** — move stablecoins between your public balance and the shielded pool.
- **Send** — a shielded transfer; amount, sender, and receiver are hidden, and change returns to
  you automatically.
- **Consolidate** — merge several notes into one.
- **Split** — pay one sender's balance out to many recipients in a single transfer.

## Money movement

- **Swap** — exchange one asset for another (edge phase; privacy ends at the public DEX leg).
- **Pay** — pay in one asset funded from another.
- **Multi-send** — one payment fanned out to many recipients.

## Escrow & channels

- **Escrow** — many payers fund one payee privately (invoices / group pay), with refund on expiry.
  This is the honest substitute for "pull", since no one can spend your notes for you.
- **Payment channels** — open a capped, shielded channel for high-frequency draw-down, settled
  with a single on-chain close.

## Automation

- **Payroll** — a saved recipient list + amounts + cadence, with run history and a "next run".
  Each run is one multi-output transfer.
- **Subscriptions** — recurring push payments you control; cancel by removing the schedule.
- **Keeper** — a local or cloud keeper that **replays wallet-prepared proofs** on schedule. It
  cannot forge spends — it has no spending key.

> Recurring flows amplify timing/amount fingerprints, so correlation defenses (standard
> denominations, jittered timing) matter most here.
