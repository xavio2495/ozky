//! Minimal classic Stellar submit path for the funder — a trimmed port of the wallet core's
//! `chain.rs`/`sign.rs` (build → sign → submit → poll), with no Tauri/soroban-sdk deps so it
//! compiles into a lean container. It only ever submits a single `CreateAccount` op from the
//! server funder key; it holds no user key material and proves nothing.

use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stellar_xdr::curr::{
    AccountId, CreateAccountOp, DecoratedSignature, Hash, LedgerEntryData, LedgerKey,
    LedgerKeyAccount, Limits, Memo, MuxedAccount, Operation, OperationBody, Preconditions,
    PublicKey, ReadXdr, SequenceNumber, Signature, SignatureHint, Transaction,
    TransactionEnvelope, TransactionExt, TransactionSignaturePayload,
    TransactionSignaturePayloadTaggedTransaction, TransactionV1Envelope, Uint256, VecM, WriteXdr,
};

pub type R<T> = Result<T, String>;

const BASE_FEE: u32 = 100;

// ----------------------------- raw RPC -----------------------------

fn rpc_call(rpc_url: &str, method: &str, params: Value) -> R<Value> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let resp: Value = ureq::post(rpc_url)
        .send_json(body)
        .map_err(|e| format!("rpc {method} transport: {e}"))?
        .into_json()
        .map_err(|e| format!("rpc {method} decode: {e}"))?;
    if let Some(err) = resp.get("error") {
        return Err(format!("rpc {method} error: {err}"));
    }
    resp.get("result").cloned().ok_or_else(|| format!("rpc {method}: no result"))
}

// ----------------------------- signing (port of core/sign.rs) -----------------------------

struct Signer {
    signing: SigningKey,
    public: [u8; 32],
}

impl Signer {
    fn from_secret(secret: &str) -> R<Signer> {
        let sk = stellar_strkey::ed25519::PrivateKey::from_string(secret)
            .map_err(|e| format!("invalid funder secret: {e}"))?;
        let signing = SigningKey::from_bytes(&sk.0);
        let public = signing.verifying_key().to_bytes();
        Ok(Signer { signing, public })
    }
    fn account_id(&self) -> AccountId {
        AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(self.public)))
    }
    fn muxed(&self) -> MuxedAccount {
        MuxedAccount::Ed25519(Uint256(self.public))
    }
}

fn network_id(passphrase: &str) -> [u8; 32] {
    Sha256::digest(passphrase.as_bytes()).into()
}

fn sign_transaction(signer: &Signer, passphrase: &str, tx: &Transaction) -> R<DecoratedSignature> {
    let payload = TransactionSignaturePayload {
        network_id: Hash(network_id(passphrase)),
        tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(tx.clone()),
    };
    let bytes = payload.to_xdr(Limits::none()).map_err(|e| format!("xdr signature payload: {e}"))?;
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    let sig = signer.signing.sign(&hash);
    let p = signer.public;
    let hint = SignatureHint([p[28], p[29], p[30], p[31]]);
    let signature = Signature(sig.to_bytes().to_vec().try_into().map_err(|_| "sig len")?);
    Ok(DecoratedSignature { hint, signature })
}

// ----------------------------- accounts -----------------------------

fn account_id_from_str(addr: &str) -> R<AccountId> {
    let pk = stellar_strkey::ed25519::PublicKey::from_string(addr)
        .map_err(|e| format!("bad address {addr}: {e}"))?;
    Ok(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(pk.0))))
}

/// Whether `addr` already has an account entry on-chain (so funding would be a no-op).
pub fn account_exists(rpc_url: &str, addr: &str) -> R<bool> {
    let account = account_id_from_str(addr)?;
    let key = LedgerKey::Account(LedgerKeyAccount { account_id: account });
    let kb64 = key.to_xdr_base64(Limits::none()).map_err(|e| format!("xdr ledger key: {e}"))?;
    let r = rpc_call(rpc_url, "getLedgerEntries", json!({ "keys": [kb64] }))?;
    Ok(r.get("entries").and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false))
}

fn account_seq(rpc_url: &str, account: &AccountId) -> R<i64> {
    let key = LedgerKey::Account(LedgerKeyAccount { account_id: account.clone() });
    let kb64 = key.to_xdr_base64(Limits::none()).map_err(|e| format!("xdr ledger key: {e}"))?;
    let r = rpc_call(rpc_url, "getLedgerEntries", json!({ "keys": [kb64] }))?;
    let xdr = r
        .get("entries")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|e| e.get("xdr"))
        .and_then(|v| v.as_str())
        .ok_or("funder account not found on-chain (unfunded?)")?;
    match LedgerEntryData::from_xdr_base64(xdr, Limits::none())
        .map_err(|e| format!("decode account entry: {e}"))?
    {
        LedgerEntryData::Account(a) => Ok(a.seq_num.0),
        _ => Err("ledger entry is not an account".into()),
    }
}

fn submit_and_poll(rpc_url: &str, what: &str, env_b64: &str) -> R<String> {
    let send = rpc_call(rpc_url, "sendTransaction", json!({ "transaction": env_b64 }))?;
    let hash = send.get("hash").and_then(|v| v.as_str()).unwrap_or("").to_string();
    match send.get("status").and_then(|v| v.as_str()).unwrap_or("") {
        "PENDING" | "DUPLICATE" => {}
        "ERROR" => {
            let detail = send.get("errorResultXdr").and_then(|v| v.as_str()).unwrap_or("");
            return Err(format!("{what} send ERROR: {detail}"));
        }
        "TRY_AGAIN_LATER" => return Err(format!("{what} send: try again later (seq/rate)")),
        other => return Err(format!("{what} send: unexpected status {other}")),
    }
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let r = rpc_call(rpc_url, "getTransaction", json!({ "hash": hash }))?;
        match r.get("status").and_then(|v| v.as_str()).unwrap_or("NOT_FOUND") {
            "SUCCESS" => return Ok(hash),
            "FAILED" => {
                let rx = r.get("resultXdr").and_then(|v| v.as_str()).unwrap_or("");
                return Err(format!("{what} FAILED on-chain: {rx}"));
            }
            _ => continue,
        }
    }
    Err(format!("{what}: timed out awaiting confirmation (hash {hash})"))
}

/// Submit a classic `CreateAccount(dest, starting_balance)` from the funder key. Returns the
/// confirmed tx hash. The funder pays the starting balance + fee.
pub fn create_account(
    rpc_url: &str,
    passphrase: &str,
    funder_secret: &str,
    dest_addr: &str,
    starting_stroops: i64,
) -> R<String> {
    let signer = Signer::from_secret(funder_secret)?;
    let seq = account_seq(rpc_url, &signer.account_id())? + 1;
    let dest = account_id_from_str(dest_addr)?;

    let op = Operation {
        source_account: None, // tx source = the funder
        body: OperationBody::CreateAccount(CreateAccountOp {
            destination: dest,
            starting_balance: starting_stroops,
        }),
    };
    let operations: VecM<Operation, 100> = vec![op].try_into().map_err(|_| "operations")?;
    let tx = Transaction {
        source_account: signer.muxed(),
        fee: BASE_FEE,
        seq_num: SequenceNumber(seq),
        cond: Preconditions::None,
        memo: Memo::None,
        operations,
        ext: TransactionExt::V0,
    };

    let sig = sign_transaction(&signer, passphrase, &tx)?;
    let signatures: VecM<DecoratedSignature, 20> = vec![sig].try_into().map_err(|_| "signatures")?;
    let env = TransactionEnvelope::Tx(TransactionV1Envelope { tx, signatures });
    let env_b64 = env.to_xdr_base64(Limits::none()).map_err(|e| format!("xdr envelope: {e}"))?;
    submit_and_poll(rpc_url, "create_account", &env_b64)
}
