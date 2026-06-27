# ozky onboarding funder

A tiny HTTP service that turns a brand-new wallet address into a funded Stellar account.

## Why it exists

A fresh account doesn't exist on-chain until something runs a classic `CreateAccount` op
above the base reserve. The wallet can't do it (it has no XLM and no account yet), and a
Soroban contract **can't create accounts** (no host function for it; the native-XLM SAC only
credits *existing* balances). So a server-held funded key does it. On onboarding the wallet
POSTs its address here; the service runs `CreateAccount(10 XLM)`, and the app then establishes
its USDC/EURC trustlines **locally**, paid by the new XLM.

It holds **only the funder key** — a small XLM float, never any user key material. Funding is
idempotent (an existing account returns `200` without re-funding) and serialized (one
`CreateAccount` at a time, so the funder's sequence number can't collide).

## API

| Method | Path      | Body                  | Notes                                   |
|--------|-----------|-----------------------|-----------------------------------------|
| GET    | `/health` | —                     | liveness (open)                         |
| POST   | `/fund`   | `{"address":"G…"}`    | `CreateAccount(10 XLM)`; 200 on success/no-op |

If `OZKY_FUNDER_TOKEN` is set, `/fund` requires `Authorization: Bearer <token>`; if unset,
`/fund` is open (a testnet-faucet posture — front it with a rate limit in production).

### Env

- `OZKY_FUNDER_SECRET` (required) — the funder account `S…` secret (must be a **funded**
  account; on testnet, friendbot-fund it once).
- `OZKY_RPC_URL` (default `https://soroban-testnet.stellar.org`)
- `OZKY_NETWORK_PASSPHRASE` (default testnet)
- `OZKY_FUNDER_TOKEN` (optional bearer token)
- `OZKY_FUND_AMOUNT` (stroops; default `100000000` = 10 XLM)
- `PORT` (Cloud Run / GKE set it; default `8080`)

## Deploy

**Cloud Run (recommended — scale-to-zero, $0 idle, repo-consistent):**

```bash
FUNDER_SECRET=S... bash deploy_cloudrun.sh
```

**GKE (Autopilot + LoadBalancer):**

```bash
FUNDER_SECRET=S... bash deploy_gke.sh
```

Either script prints the URL/IP. Point the wallet at it:

```
OZKY_FUNDER_URL=<url>/fund
OZKY_FUNDER_TOKEN=<token>   # only if you set one
```

(in `ozky.config.json` or the environment — see `ozky/src-tauri/src/core/config.rs`.)

## How the app uses it

`core::funder::request_funding(address)` POSTs to `OZKY_FUNDER_URL`; `core::trustline::
provision_new_account` calls it on onboarding, waits for the account to appear, then submits
the trustlines locally. Surfaced via the `provision_account` Tauri command, invoked after the
2FA-confirmed `finish_setup`. With no `OZKY_FUNDER_URL` configured, onboarding silently skips
funding (dev) and the user can retry later.
