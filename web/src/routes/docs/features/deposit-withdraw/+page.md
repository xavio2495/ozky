# Deposit & withdraw

The two **edges** of the pool: moving value in from your public balance, and back out. These
are public Stellar legs by nature, so ozky treats them carefully.

## Deposit (shield)

Locks public stablecoin into the asset **vault** and appends a fresh note **commitment** for
you. From that point the value is private. Onboarding is **relayer-funded** — your account is
created and **USDC / EURC** trustlines are established for you (live on real Circle testnet
USDC; USDT is defined but not yet live), and you never touch a public XLM account.

## Withdraw (unshield)

Spends a shielded note — publishing its **nullifier** and proving membership — and releases
public stablecoin from the vault to a public `G…` destination. **`dest_bind` is enforced
on-chain**: a withdrawal can't be redirected, and contract destinations are rejected.

## The privacy edge

Deposit and withdraw are visible on Stellar. Timing and amount at the edge can correlate a
withdrawal with an earlier deposit. ozky reduces this with a **client-side** denomination /
timing policy — there is **no on-chain flag**, so the chain stays indistinguishable:

- **Instant** — submitted right away.
- **Maximum privacy** — held for a randomized client-side delay (cancelable, with a "Send
  now" override) to decorrelate submission time.
- **Standard-denomination quick-picks** on the public edges so visible amounts blend.

> See **[Concepts → the edges](/docs/concepts)** for the full trust posture.
