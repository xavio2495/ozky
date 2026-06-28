# Payroll & subscriptions

The **scheduler**: recurring shielded payments you set up once. It is wallet-resident (runs
while the app is open) and can hand scheduled payroll to a cloud keeper to run while it's
closed.

## Payroll

Save a recipient list with amounts and a cadence. The **Payroll** page shows the schedule,
past runs, and what's due next. Each run is one **[split](/docs/features/notes)** transfer
(1-in / N-out). Approval is configurable per payroll — **Auto** (runs on schedule) or
**Manual** (you confirm each run).

## Subscriptions

Recurring **push** payments you authorize and can cancel. Each cycle runs as a normal shielded
payment — there is **no standing pull on your notes**, because no one can spend your notes for
you. Cancel by removing the schedule and no further payments happen.

## The keeper (local or cloud)

A payroll run can execute in two places, set per item:

- **Local** — the desktop app runs it while open.
- **Cloud** — the `keeper-service` runs it on schedule even when your app is closed.

The wallet **pushes pre-proved runs** to the keeper; a scheduler hits `/tick` and it submits
any due bundle via your **dedicated relayer**. The keeper holds **no `owner_sk` and no
`notes_key`** and carries no plaintext amounts — it **cannot forge spends**, and a consumed
nullifier prevents replay. See **[Cloud runtimes](/docs/cloud)**.

## Privacy note

Fixed amounts on a fixed cadence are a strong fingerprint. Standard denominations and
jittered timing (**[edge policy](/docs/features/deposit-withdraw)**) reduce correlation.
