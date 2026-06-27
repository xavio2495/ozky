//! ozky onboarding funder — the server that turns a brand-new wallet address into a funded
//! Stellar account.
//!
//! Why it exists: a fresh account doesn't exist on-chain until something runs a classic
//! `CreateAccount` op above the base reserve. The app can't do it (no XLM, no account yet)
//! and a Soroban contract can't create accounts, so a server-held funded key does it. The
//! wallet POSTs its address here on onboarding; the service runs `CreateAccount(10 XLM)` and
//! the app then sets up its trustlines locally with the new XLM.
//!
//! Security posture: holds ONLY the funder key — a small XLM float, never any user key
//! material. Funding is idempotent (an existing account returns 200 without re-funding) and
//! serialized (one CreateAccount at a time) so the funder's sequence number can't collide.
//!
//! Routes:
//!   GET  /health   liveness (open)
//!   POST /fund     body = {"address":"G…"}  → CreateAccount(10 XLM); 200 on success/no-op
//!
//! Auth: if OZKY_FUNDER_TOKEN is set, /fund requires `Authorization: Bearer <token>`; if
//! unset, /fund is open (a testnet faucet posture — front it with a rate limit in prod).
//!
//! Env: PORT (default 8080), OZKY_FUNDER_SECRET (S…, required), OZKY_RPC_URL,
//! OZKY_NETWORK_PASSPHRASE, OZKY_FUNDER_TOKEN (optional), OZKY_FUND_AMOUNT (stroops; default
//! 100_000_000 = 10 XLM).

mod chain;

use std::sync::Mutex;

struct Config {
    rpc_url: String,
    passphrase: String,
    funder_secret: String,
    token: Option<String>,
    amount_stroops: i64,
    port: u16,
}

fn env(k: &str) -> Result<String, String> {
    std::env::var(k).ok().filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("missing env {k}"))
}

fn load_config() -> Result<Config, String> {
    Ok(Config {
        rpc_url: std::env::var("OZKY_RPC_URL")
            .unwrap_or_else(|_| "https://soroban-testnet.stellar.org".into()),
        passphrase: std::env::var("OZKY_NETWORK_PASSPHRASE")
            .unwrap_or_else(|_| "Test SDF Network ; September 2015".into()),
        funder_secret: env("OZKY_FUNDER_SECRET")?,
        token: std::env::var("OZKY_FUNDER_TOKEN").ok().filter(|s| !s.trim().is_empty()),
        amount_stroops: std::env::var("OZKY_FUND_AMOUNT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100_000_000), // 10 XLM
        port: std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080),
    })
}

/// Pull `address` out of a `{"address":"G…"}` JSON body.
fn parse_address(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let addr = v.get("address")?.as_str()?.trim().to_string();
    // Cheap shape check; chain::create_account does the real strkey validation.
    if addr.len() == 56 && addr.starts_with('G') {
        Some(addr)
    } else {
        None
    }
}

fn main() {
    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ozky-funder-service: {e}");
            std::process::exit(1);
        }
    };
    // Serialize funding so the funder's sequence number can't be reused under concurrency.
    let fund_lock: Mutex<()> = Mutex::new(());

    let addr = format!("0.0.0.0:{}", cfg.port);
    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ozky-funder-service: bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "ozky funder serving on {addr} ({} stroops/account, {})",
        cfg.amount_stroops, cfg.rpc_url
    );

    for mut req in server.incoming_requests() {
        let method = req.method().clone();
        let url = req.url().to_string();

        if method == tiny_http::Method::Get && url == "/health" {
            respond(req, 200, "ok");
            continue;
        }

        if !(method == tiny_http::Method::Post && url == "/fund") {
            respond(req, 404, "not found");
            continue;
        }

        // Optional bearer auth.
        if let Some(token) = cfg.token.as_deref() {
            let authed = req.headers().iter().any(|h| {
                h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {token}")
            });
            if !authed {
                respond(req, 401, "unauthorized");
                continue;
            }
        }

        let mut buf = String::new();
        if std::io::Read::read_to_string(req.as_reader(), &mut buf).is_err() {
            respond(req, 400, "bad body");
            continue;
        }
        let Some(address) = parse_address(&buf) else {
            respond(req, 400, "expected body {\"address\":\"G…\"}");
            continue;
        };

        let (code, body) = {
            let _guard = fund_lock.lock().unwrap_or_else(|e| e.into_inner());
            match chain::account_exists(&cfg.rpc_url, &address) {
                Ok(true) => (200, "{\"status\":\"already_funded\"}".to_string()),
                Ok(false) => match chain::create_account(
                    &cfg.rpc_url,
                    &cfg.passphrase,
                    &cfg.funder_secret,
                    &address,
                    cfg.amount_stroops,
                ) {
                    Ok(hash) => (200, format!("{{\"status\":\"funded\",\"tx\":\"{hash}\"}}")),
                    Err(e) => {
                        eprintln!("fund {address} failed: {e}");
                        (502, format!("{{\"error\":{}}}", json_str(&e)))
                    }
                },
                Err(e) => {
                    eprintln!("exists-check {address} failed: {e}");
                    (502, format!("{{\"error\":{}}}", json_str(&e)))
                }
            }
        };
        respond(req, code, &body);
    }
}

/// JSON-encode a string (quote + escape) so error messages can't break the response body.
fn json_str(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

fn respond(req: tiny_http::Request, code: u16, body: &str) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let resp = tiny_http::Response::from_string(body).with_status_code(code).with_header(header);
    let _ = req.respond(resp);
}
