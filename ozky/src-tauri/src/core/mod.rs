//! ozky wallet Rust core (Part 2). All heavy cryptography runs here, off the UI
//! thread; the Svelte UI only invokes plain actions. This module is the A0
//! skeleton: the submodule layout + interfaces are laid out, each phase fills in
//! its part:
//!
//! - [`keys`]     — 12-word BIP39 -> Stellar Ed25519 key + distinct BN254 ZK root (A1)
//! - [`keychain`] — OS keychain storage for secrets (wired in A0, used from A1)
//! - [`encrypt`]  — note payload encryption: ECDH -> HKDF -> key-committing AEAD (A2)
//! - [`scan`]     — note scanning via view-tag trial match against the indexer (A2)
//! - [`proving`]  — client-side Noir/UltraHonk proving + witness generation (A2)
//! - [`sign`]     — transaction binding / spend-authorization signing (A3)
//! - [`chain`]    — Stellar RPC + indexer client (A2/A3)
//!
//! Functions not yet implemented return [`CoreError::NotImplemented`] naming the
//! phase that owns them, so the wiring is exercised end-to-end before the crypto
//! lands.

// A0 is the interface skeleton: many core fns are defined ahead of the command(s)
// that will call them in later phases. Allow that without warnings here.
#![allow(dead_code)]

pub mod accounts;
pub mod chain;
pub mod channel;
pub mod config;
pub mod deposit;
pub mod disclose;
pub mod encrypt;
pub mod enroll;
pub mod escrow;
pub mod history;
pub mod keeper;
pub mod keeper_task;
pub mod keychain;
pub mod keys;
pub mod notes;
pub mod payroll;
pub mod pedersen;
pub mod poseidon;
pub mod price;
pub mod proving;
pub mod scan;
pub mod send;
pub mod session;
pub mod sign;
pub mod subscriptions;
pub mod swap;
pub mod totp;
pub mod trustline;
pub mod vault;
pub mod withdraw;
pub mod witness;

use serde::Serialize;
use std::fmt;

/// Error surface returned across the `invoke` boundary (serialized to the UI).
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum CoreError {
    /// A function whose implementation belongs to a later phase was called.
    NotImplemented(String),
    /// OS keychain access failed.
    Keychain(String),
    /// No wallet has been created/restored yet.
    NoWallet,
    /// A wallet exists but is locked — unlock (password + TOTP) required first.
    Locked,
    /// Chain / indexer access failed (network, decode).
    Chain(String),
    /// Cryptographic operation failed (encryption, decryption, key agreement).
    Crypto(String),
    /// Proof generation failed (witness solve, prover, verify).
    Proving(String),
}

impl CoreError {
    pub fn not_implemented(what: &str) -> Self {
        CoreError::NotImplemented(what.to_string())
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::NotImplemented(w) => write!(f, "not implemented yet: {w}"),
            CoreError::Keychain(e) => write!(f, "keychain error: {e}"),
            CoreError::NoWallet => write!(f, "no wallet initialized"),
            CoreError::Locked => write!(f, "wallet is locked"),
            CoreError::Chain(e) => write!(f, "chain/indexer error: {e}"),
            CoreError::Crypto(e) => write!(f, "crypto error: {e}"),
            CoreError::Proving(e) => write!(f, "proving error: {e}"),
        }
    }
}

impl std::error::Error for CoreError {}
