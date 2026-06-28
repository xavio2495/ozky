# Consolidate & split

Housekeeping for your **notes**. Because balances are a set of UTXO-style notes, their shape
affects how cheap and how private future spends are.

## Consolidate

Merges several owned notes into one. The **transfer4** circuit spends up to **4 owned notes**
(producing 2 outputs), so coin selection can sweep a fragmented balance into a single larger
note — fewer inputs on later spends means smaller proofs and lower cost. A Settings
**"Consolidate notes"** action runs it. Live on testnet.

## Split

Divides one note into many. The **split** circuit is **1-in / N-out** (padded to 6), so it
both prepares **standard denominations** ahead of time and acts as the **multi-recipient
payout** primitive — one shielded transfer paying several outputs at once.

Standard-sized notes let later sends and withdrawals blend into a shared anonymity set
instead of revealing an exact amount.

## When to use it

Consolidate when your note set gets fragmented; split to pre-stage standard denominations or
to pay many recipients in one action. Splitting pairs naturally with the
**[deposit/withdraw](/docs/features/deposit-withdraw)** denomination policy and underpins
**[payroll](/docs/features/payroll)**.

> Both run the same prove → verify path as a **[shielded send](/docs/features/shielded-send)**;
> only you can spend your notes.
