//! Decode the pool contract's events (ScVal XDR) into typed records.
//!
//! The pool emits:
//!  - `commit` : topics [Symbol("commit"), U32(leaf_index)],
//!               value = Vec[U256 commitment, Bytes enc_note, Bytes eph_pub, U32 view_tag]
//!               (deposit/transfer) OR U256 commitment (withdraw change).
//!  - `nullif` : value = U256 nullifier.
//!  - `roots`  : value = Vec[U256 commitment_root, U256 nullifier_root].

use crate::rpc::RawEvent;
use stellar_xdr::curr::{Limits, ReadXdr, ScVal};

pub fn u256_hex(v: &ScVal) -> Option<String> {
    if let ScVal::U256(p) = v {
        let mut b = [0u8; 32];
        b[0..8].copy_from_slice(&p.hi_hi.to_be_bytes());
        b[8..16].copy_from_slice(&p.hi_lo.to_be_bytes());
        b[16..24].copy_from_slice(&p.lo_hi.to_be_bytes());
        b[24..32].copy_from_slice(&p.lo_lo.to_be_bytes());
        Some(to_hex(&b))
    } else {
        None
    }
}

pub fn to_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(2 + b.len() * 2);
    s.push_str("0x");
    for byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

fn parse(b64: &str) -> Option<ScVal> {
    ScVal::from_xdr_base64(b64, Limits::none()).ok()
}

fn symbol_name(v: &ScVal) -> Option<String> {
    if let ScVal::Symbol(s) = v {
        Some(String::from_utf8_lossy(s.0.as_slice()).to_string())
    } else {
        None
    }
}

fn bytes_hex(v: &ScVal) -> Option<String> {
    if let ScVal::Bytes(b) = v {
        Some(to_hex(b.0.as_slice()))
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Commit {
    pub leaf_index: u32,
    pub commitment: String,
    pub enc_note: Option<String>,
    pub ephemeral_pub: Option<String>,
    pub view_tag: Option<u32>,
    pub ledger: u32,
    pub tx_hash: String,
}

#[derive(Clone, Debug)]
pub enum PoolEvent {
    Commit(Commit),
    Nullifier { value: String, ledger: u32 },
    Roots { commitment_root: String, nullifier_root: String, ledger: u32 },
}

pub fn classify(e: &RawEvent) -> Option<PoolEvent> {
    let topic0 = parse(e.topics.first()?)?;
    let name = symbol_name(&topic0)?;
    let value = parse(&e.value)?;
    match name.as_str() {
        "commit" => {
            let leaf_index = match parse(e.topics.get(1)?)? {
                ScVal::U32(n) => n,
                _ => return None,
            };
            // value is either a Vec (deposit/transfer) or a bare U256 (withdraw change).
            let (commitment, enc_note, ephemeral_pub, view_tag) = match &value {
                ScVal::Vec(Some(items)) => {
                    let c = u256_hex(items.get(0)?)?;
                    let enc = items.get(1).and_then(bytes_hex);
                    let eph = items.get(2).and_then(bytes_hex);
                    let vt = items.get(3).and_then(|v| match v {
                        ScVal::U32(n) => Some(*n),
                        _ => None,
                    });
                    (c, enc, eph, vt)
                }
                ScVal::U256(_) => (u256_hex(&value)?, None, None, None),
                _ => return None,
            };
            Some(PoolEvent::Commit(Commit {
                leaf_index,
                commitment,
                enc_note,
                ephemeral_pub,
                view_tag,
                ledger: e.ledger,
                tx_hash: e.tx_hash.clone(),
            }))
        }
        "nullif" => Some(PoolEvent::Nullifier {
            value: u256_hex(&value)?,
            ledger: e.ledger,
        }),
        "roots" => {
            if let ScVal::Vec(Some(items)) = &value {
                Some(PoolEvent::Roots {
                    commitment_root: u256_hex(items.get(0)?)?,
                    nullifier_root: u256_hex(items.get(1)?)?,
                    ledger: e.ledger,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}
