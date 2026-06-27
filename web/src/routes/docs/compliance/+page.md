# Compliance & disclosure

Privacy and compliance are not opposites in ozky — both hold at once.

## ASP — approved-set membership

The **Association Set Provider (ASP)** publishes an approved-set root. Every shielded transfer
proves **in-circuit** that its funds trace to a depositor in that approved set — _without
revealing which one_. Shielded funds are provably clean; the transaction graph stays private.

On mainnet this extends to denied-set **non-membership** (freeze bad actors at withdrawal time).

## Selective disclosure — scoped view keys

When you need to show activity to an auditor, you share a **scoped, revocable view key**:

```
master_view_secret
 └─ account_i
     └─ asset_j
         └─ epoch_k  →  { incoming-viewing key, detection key }
```

The auditor receives exactly the node at `account / asset / epoch`. From it they find, decrypt,
and **re-derive the commitment to verify against the chain** — provable disclosure — with no path
back up to any other account, asset, or epoch. A **per-transaction** disclosure path is also
available for one-off requests.

Revoke a view key and the auditor's window closes.

> See **[Privacy model](/docs/privacy)** for trust assumptions and the testnet posture.
