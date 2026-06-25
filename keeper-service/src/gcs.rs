//! Minimal Google Cloud Storage client for cold-start durability (no heavy client crate).
//!
//! The cloud keeper's run store must survive Cloud Run cold starts / evictions (scale-to-zero) so a
//! run PUSHed in one warm window still TICKs days later. We persist the store as one JSON object in a
//! GCS bucket: load it on boot, overwrite it after every mutation.
//!
//! Auth uses the Cloud Run runtime service account's token from the GCP metadata server (no key
//! file), and the GCS JSON API over `ureq` (already a dep). Off-GCP (local), `OZKY_KEEPER_BUCKET` is
//! unset and none of this runs — the store stays in-memory.

use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub type R<T> = Result<T, String>;

const METADATA_TOKEN_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

/// Cached access token (valid ~1h); refreshed when within 60s of expiry. The metadata fetch is cheap
/// but caching avoids a round-trip on every /tick + /push during a 2-day stress run.
static TOKEN_CACHE: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn access_token() -> R<String> {
    if let Some((tok, exp)) = TOKEN_CACHE.lock().unwrap().as_ref() {
        if *exp > Instant::now() + Duration::from_secs(60) {
            return Ok(tok.clone());
        }
    }
    let resp: serde_json::Value = ureq::get(METADATA_TOKEN_URL)
        .set("Metadata-Flavor", "Google")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| format!("metadata token: {e}"))?
        .into_json()
        .map_err(|e| format!("metadata token decode: {e}"))?;
    let tok = resp
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("metadata token: no access_token")?
        .to_string();
    let secs = resp.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(3600);
    *TOKEN_CACHE.lock().unwrap() = Some((tok.clone(), Instant::now() + Duration::from_secs(secs)));
    Ok(tok)
}

/// Percent-encode a GCS object name for use in a URL path/query (object names may contain `/`, `.`).
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Download an object's bytes; `Ok(None)` if it doesn't exist yet (404).
pub fn get_object(bucket: &str, name: &str) -> R<Option<Vec<u8>>> {
    let token = access_token()?;
    let url = format!(
        "https://storage.googleapis.com/storage/v1/b/{}/o/{}?alt=media",
        enc(bucket),
        enc(name)
    );
    match ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .timeout(Duration::from_secs(20))
        .call()
    {
        Ok(resp) => {
            let mut buf = Vec::new();
            resp.into_reader()
                .read_to_end(&mut buf)
                .map_err(|e| format!("gcs read body: {e}"))?;
            Ok(Some(buf))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(format!("gcs get {name}: {e}")),
    }
}

/// Upload (overwrite) an object with `body`.
pub fn put_object(bucket: &str, name: &str, body: &[u8]) -> R<()> {
    let token = access_token()?;
    let url = format!(
        "https://storage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&name={}",
        enc(bucket),
        enc(name)
    );
    ureq::post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(20))
        .send_bytes(body)
        .map(|_| ())
        .map_err(|e| format!("gcs put {name}: {e}"))
}
