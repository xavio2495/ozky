# viewkeys

The **view-key registry and disclosure-grant trail**. By design this contract is a **thin
on-chain record keeper** — the actual cryptographic disclosure happens **off-chain**.

**Deployed (testnet):**
[`CDTYQIHSCRUPNGI42SLXMHHXXWMX4DQTBYMHBEXUALZ7GVZWXRI3MBSV`](https://stellar.expert/explorer/testnet/contract/CDTYQIHSCRUPNGI42SLXMHHXXWMX4DQTBYMHBEXUALZ7GVZWXRI3MBSV)

## Off-chain vs on-chain

- **Off-chain (the real disclosure):** an auditor handed the viewing + detection keys for a
  scope re-derives the note contents and the commitment, then verifies against the on-chain
  commitments — **no contract involvement**. The BIP32-style view-key hierarchy
  (ECDH → HKDF → AEAD) is derived in the Rust core.
- **On-chain (this contract):** the **auditable trail** — which scoped viewing keys an owner
  published, and which disclosure grants they made. So a grant, and its later **revocation**,
  is provable and timestamped.

## Why a trail matters

Spending authority is never involved in disclosure: an exported scope contains a viewing
secret and `owner_pk` but **no `owner_sk`**. The registry makes the act of granting — and
revoking — accountable without ever exposing the ability to spend.

> The user-facing side of this is **[Auditor disclosure](/docs/features/disclosure)**.
