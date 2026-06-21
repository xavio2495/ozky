//! TOTP 2FA (RFC 6238) — the second factor on wallet unlock.
//!
//! Authenticator-app default parameters: HMAC-SHA1, 6 digits, 30-second step. The
//! shared secret is generated at wallet setup, stored inside the password-encrypted
//! [`super::vault`], and surfaced once as an `otpauth://` URI (QR) + base32 string for
//! the user to add to their authenticator. On unlock we re-derive the expected code and
//! compare (±1 step for clock skew).
//!
//! Note on the security model: the password is the *decryption* factor (it unlocks the
//! vault); TOTP is an *access gate* checked after decryption, not a second key-derivation
//! input (TOTP codes are ephemeral, so no stable key can be derived from them). This is
//! the standard wallet 2FA model — it stops someone who has the password but not the
//! authenticator.

use super::CoreError;
use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const DIGITS: u32 = 6;
const STEP_SECS: u64 = 30;
/// Secret length in bytes (160-bit, the RFC-6238 SHA1 recommendation).
pub const SECRET_LEN: usize = 20;

/// Generate a fresh random TOTP secret.
pub fn generate_secret() -> [u8; SECRET_LEN] {
    let mut s = [0u8; SECRET_LEN];
    rand_core::OsRng.fill_bytes(&mut s);
    s
}

/// Base32 (RFC 4648, no padding) encoding of the secret — what the user types into an
/// authenticator app if they can't scan the QR.
pub fn secret_base32(secret: &[u8]) -> String {
    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, secret)
}

/// `otpauth://totp/...` provisioning URI (rendered as a QR by the UI).
pub fn provisioning_uri(secret: &[u8], account: &str, issuer: &str) -> String {
    let b32 = secret_base32(secret);
    let label = urlencode(&format!("{issuer}:{account}"));
    let iss = urlencode(issuer);
    format!(
        "otpauth://totp/{label}?secret={b32}&issuer={iss}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECS}"
    )
}

/// The 6-digit code for a given unix time.
pub fn code_at(secret: &[u8], unix_secs: u64) -> String {
    let counter = unix_secs / STEP_SECS;
    hotp(secret, counter)
}

/// Verify `code` against the current time, allowing ±1 step for clock skew. Constant
/// across the small window; rejects malformed (non-6-digit) input.
pub fn verify(secret: &[u8], code: &str, now_unix: u64) -> bool {
    let code = code.trim();
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let counter = now_unix / STEP_SECS;
    // window: previous, current, next step.
    for c in [counter.wrapping_sub(1), counter, counter + 1] {
        if constant_eq(hotp(secret, c).as_bytes(), code.as_bytes()) {
            return true;
        }
    }
    false
}

/// Current wall-clock unix seconds.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Verify `code` against the secret stored in the unlocked session (the signup
/// 2FA-confirm step and any in-app re-check).
pub fn verify_session(code: &str) -> Result<bool, CoreError> {
    let secret = super::session::totp_secret()?;
    Ok(verify(&secret, code, now()))
}

// --- RFC 4226 HOTP (the TOTP building block) ---------------------------------------

fn hotp(secret: &[u8], counter: u64) -> String {
    let mut mac = HmacSha1::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(&counter.to_be_bytes());
    let hs = mac.finalize().into_bytes();
    // Dynamic truncation (RFC 4226 §5.3).
    let offset = (hs[19] & 0x0f) as usize;
    let bin = ((hs[offset] as u32 & 0x7f) << 24)
        | ((hs[offset + 1] as u32) << 16)
        | ((hs[offset + 2] as u32) << 8)
        | (hs[offset + 3] as u32);
    let otp = bin % 10u32.pow(DIGITS);
    format!("{otp:0width$}", width = DIGITS as usize)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Minimal percent-encoding for otpauth labels (alnum / -_.~ pass through).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

use rand_core::RngCore;

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 Appendix B test vectors (SHA1 seed = ASCII "12345678901234567890").
    // The RFC tabulates 8-digit codes; the 6-digit code is its last 6 digits.
    const SEED: &[u8] = b"12345678901234567890";

    #[test]
    fn rfc6238_sha1_vectors() {
        // (unix_time, 8-digit RFC value) -> our 6-digit = last 6.
        let cases = [
            (59u64, "94287082"),
            (1111111109, "07081804"),
            (1111111111, "14050471"),
            (1234567890, "89005924"),
            (2000000000, "69279037"),
        ];
        for (t, eight) in cases {
            let want6 = &eight[2..];
            assert_eq!(code_at(SEED, t), want6, "TOTP at t={t}");
        }
    }

    #[test]
    fn verify_accepts_current_and_skew_rejects_wrong() {
        let secret = generate_secret();
        let t = 1_700_000_000u64;
        let code = code_at(&secret, t);
        assert!(verify(&secret, &code, t), "current step verifies");
        // within +1 step skew (server clock 25s behind the code)
        assert!(verify(&secret, &code, t + 25), "+1 step skew verifies");
        // two steps away should fail
        assert!(!verify(&secret, &code, t + 70), "outside window rejected");
        assert!(!verify(&secret, "000000", t) || code == "000000", "wrong code rejected");
        assert!(!verify(&secret, "12345", t), "malformed (5-digit) rejected");
    }

    #[test]
    fn base32_and_uri_are_well_formed() {
        let secret = generate_secret();
        let b32 = secret_base32(&secret);
        assert!(!b32.contains('='), "no padding");
        let uri = provisioning_uri(&secret, "ozky", "ozky wallet");
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains(&format!("secret={b32}")));
        assert!(uri.contains("digits=6") && uri.contains("period=30"));
    }
}
