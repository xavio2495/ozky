# ASP compliance

Privacy and compliance hold at once. Every shielded transfer proves **in-circuit** that its
funds are clean — without revealing the transaction graph.

## Approved-set membership

An **Association Set Provider (ASP)** publishes a depth-20 Poseidon approved-set root (held
in the **[`policy`](/docs/contracts/policy)** contract). Each transfer proves
`owner_pk ∈ asp_root` — that its funds trace to a depositor in that approved set, _without
revealing which one_ (and the set has more than one member). Shielded funds are provably
clean; the graph stays private.

## What you see in the app

Compliance status surfaces on **Send / Withdraw** as a check, not a flag that partitions
users. The proof is part of the normal spend — there's no extra step and no disclosure of
who you transacted with.

## On mainnet

This extends to denied-set **non-membership** (freeze bad actors at withdrawal time), and
the approved-set root moves under governance/multisig control.

> Distinct from **[auditor disclosure](/docs/features/disclosure)**: compliance is
> automatic and reveals nothing; disclosure is opt-in and scoped.
