# Auditor disclosure

When you need to **show** activity — to an auditor, an accountant, a counterparty — you share
a **scoped, revocable view key**. It is read-only and never grants spending power.

## Scoped view keys

View keys derive in a BIP32-style tree (ECDH → HKDF → AEAD), in the Rust core:

```
master_view_secret
 └─ account_i
     └─ asset_j
         └─ epoch_k  →  { incoming-viewing key, detection key }
```

A scoped export carries the **viewing secret + `owner_pk`** for that node — but **no
`owner_sk`**. The auditor receives exactly the node at `account / asset / epoch`, finds and
decrypts those notes, and **re-derives each commitment to verify against the chain** —
provable disclosure — with **no path back up** to any other account, asset, or epoch.

## Revocation & trail

The grant (and its later revocation) is recorded on the
**[`viewkeys`](/docs/contracts/viewkeys)** contract — a provable, timestamped trail. Revoke a
key and the auditor's window **closes**. Spending authority is never involved at any point.

## Where in the app

The **Auditor** page issues and revokes scopes; **Settings** holds the master phrase the tree
derives from.

> The automatic, reveal-nothing counterpart is **[ASP compliance](/docs/features/compliance)**.
