//! Minimal Stellar/Soroban submit path for the cloud keeper — a faithful port of the wallet
//! core's `chain.rs` (build → simulate → sign → submit → poll) + `sign.rs`, with no Tauri /
//! soroban-sdk deps so it compiles into a lean Cloud Run image (like the indexer).
//!
//! It ONLY submits pre-proved bundles via the relayer; it never proves (no `owner_sk`) and never
//! reconstructs the note tree (the on-chain verifier enforces epoch + nullifier-root, so a stale
//! proof simply fails on-chain). Submit args mirror the pool's `split` / `transfer4` entrypoints
//! byte-for-byte (asset_tag U256, public_inputs/proof Bytes, then the output ciphertext vecs).

use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use stellar_xdr::curr::{
    AccountId, BytesM, ContractId, DecoratedSignature, Hash, HostFunction, InvokeContractArgs,
    InvokeHostFunctionOp, LedgerEntryData, LedgerKey, LedgerKeyAccount, Limits, Memo, MuxedAccount,
    Operation, OperationBody, Preconditions, PublicKey, ReadXdr, ScAddress, ScBytes, ScSymbol,
    ScVal, ScVec, SequenceNumber, Signature, SignatureHint, SorobanAuthorizationEntry,
    SorobanTransactionData, Transaction, TransactionEnvelope, TransactionExt,
    TransactionSignaturePayload, TransactionSignaturePayloadTaggedTransaction, TransactionV1Envelope,
    UInt256Parts, Uint256, VecM, WriteXdr,
};

pub type R<T> = Result<T, String>;

const BASE_FEE: u32 = 100;
pub const LEDGER_PER_EPOCH: u64 = 110_000;

// ----------------------------- raw RPC -----------------------------

fn rpc_call(rpc_url: &str, method: &str, params: Value) -> R<Value> {
    let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
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

/// The current epoch = `latest_ledger_sequence / 110_000` (the pool's `current_epoch`).
pub fn current_epoch(rpc_url: &str) -> R<u32> {
    let r = rpc_call(rpc_url, "getLatestLedger", json!({}))?;
    let seq = r
        .get("sequence")
        .and_then(|v| v.as_u64())
        .ok_or("getLatestLedger: no sequence")?;
    Ok((seq / LEDGER_PER_EPOCH) as u32)
}

// ----------------------------- signing (port of core/sign.rs) -----------------------------

struct Signer {
    signing: SigningKey,
    public: [u8; 32],
}

impl Signer {
    fn from_secret(secret: &str) -> R<Signer> {
        let sk = stellar_strkey::ed25519::PrivateKey::from_string(secret)
            .map_err(|e| format!("invalid relayer secret: {e}"))?;
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
    let bytes = payload
        .to_xdr(Limits::none())
        .map_err(|e| format!("xdr signature payload: {e}"))?;
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    let sig = signer.signing.sign(&hash);
    let p = signer.public;
    let hint = SignatureHint([p[28], p[29], p[30], p[31]]);
    let signature = Signature(sig.to_bytes().to_vec().try_into().map_err(|_| "sig len")?);
    Ok(DecoratedSignature { hint, signature })
}

// ----------------------------- ScVal builders -----------------------------

fn sc_u256_be(bytes: &[u8; 32]) -> ScVal {
    ScVal::U256(UInt256Parts {
        hi_hi: u64::from_be_bytes(bytes[0..8].try_into().unwrap()),
        hi_lo: u64::from_be_bytes(bytes[8..16].try_into().unwrap()),
        lo_hi: u64::from_be_bytes(bytes[16..24].try_into().unwrap()),
        lo_lo: u64::from_be_bytes(bytes[24..32].try_into().unwrap()),
    })
}

/// 32-byte big-endian from `0x…` hex (left-zero-padded) — for the bundle's `asset_tag`.
fn u256_hex(h: &str) -> R<ScVal> {
    let h = h.strip_prefix("0x").unwrap_or(h);
    let raw = hex::decode(h).map_err(|_| format!("bad u256 hex: {h}"))?;
    if raw.len() > 32 {
        return Err(format!("u256 overflow: {h}"));
    }
    let mut b = [0u8; 32];
    b[32 - raw.len()..].copy_from_slice(&raw);
    Ok(sc_u256_be(&b))
}

fn sc_bytes(b: &[u8]) -> R<ScVal> {
    let bm: BytesM = b.to_vec().try_into().map_err(|_| "bytes arg too long")?;
    Ok(ScVal::Bytes(ScBytes(bm)))
}

fn sc_vec(items: Vec<ScVal>) -> R<ScVal> {
    let v: VecM<ScVal> = items.try_into().map_err(|_| "vec arg too long")?;
    Ok(ScVal::Vec(Some(ScVec(v))))
}

fn sc_symbol(s: &str) -> R<ScSymbol> {
    Ok(ScSymbol(s.try_into().map_err(|_| format!("bad symbol: {s}"))?))
}

fn contract_address(c: &str) -> R<ScAddress> {
    let id = stellar_strkey::Contract::from_string(c).map_err(|e| format!("bad contract id {c}: {e}"))?;
    Ok(ScAddress::Contract(ContractId(Hash(id.0))))
}

// ----------------------------- build / simulate / submit -----------------------------

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
        .ok_or("relayer account not found on-chain (unfunded?)")?;
    match LedgerEntryData::from_xdr_base64(xdr, Limits::none())
        .map_err(|e| format!("decode account entry: {e}"))?
    {
        LedgerEntryData::Account(a) => Ok(a.seq_num.0),
        _ => Err("ledger entry is not an account".into()),
    }
}

fn build_tx(
    source: &MuxedAccount,
    seq: i64,
    fee: u32,
    host_function: HostFunction,
    auth: VecM<SorobanAuthorizationEntry>,
    ext: TransactionExt,
) -> R<Transaction> {
    let op = Operation {
        source_account: None,
        body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp { host_function, auth }),
    };
    let operations: VecM<Operation, 100> = vec![op].try_into().map_err(|_| "operations")?;
    Ok(Transaction {
        source_account: source.clone(),
        fee,
        seq_num: SequenceNumber(seq),
        cond: Preconditions::None,
        memo: Memo::None,
        operations,
        ext,
    })
}

fn parse_sim_auth(sim: &Value) -> R<VecM<SorobanAuthorizationEntry>> {
    let mut entries: Vec<SorobanAuthorizationEntry> = Vec::new();
    if let Some(auths) = sim
        .get("results")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("auth"))
        .and_then(|v| v.as_array())
    {
        for a in auths {
            if let Some(s) = a.as_str() {
                entries.push(
                    SorobanAuthorizationEntry::from_xdr_base64(s, Limits::none())
                        .map_err(|e| format!("decode auth entry: {e}"))?,
                );
            }
        }
    }
    entries.try_into().map_err(|_| "auth entries".into())
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

/// Build → simulate → sign (relayer) → submit → poll an `InvokeHostFunction`. Returns the tx hash.
fn invoke_contract(
    rpc_url: &str,
    passphrase: &str,
    relayer_secret: &str,
    contract_id: &str,
    fn_name: &str,
    args: Vec<ScVal>,
) -> R<String> {
    let signer = Signer::from_secret(relayer_secret)?;
    let source = signer.muxed();
    let seq = account_seq(rpc_url, &signer.account_id())? + 1;

    let call_args: VecM<ScVal> = args.try_into().map_err(|_| "too many call args")?;
    let host_function = HostFunction::InvokeContract(InvokeContractArgs {
        contract_address: contract_address(contract_id)?,
        function_name: sc_symbol(fn_name)?,
        args: call_args,
    });

    let sim_tx = build_tx(&source, seq, BASE_FEE, host_function.clone(), VecM::default(), TransactionExt::V0)?;
    let sim_env = TransactionEnvelope::Tx(TransactionV1Envelope { tx: sim_tx, signatures: VecM::default() });
    let sim_b64 = sim_env.to_xdr_base64(Limits::none()).map_err(|e| format!("xdr sim envelope: {e}"))?;
    let sim = rpc_call(rpc_url, "simulateTransaction", json!({ "transaction": sim_b64 }))?;
    if let Some(err) = sim.get("error").and_then(|v| v.as_str()) {
        let head: String = err.chars().take(180).collect();
        return Err(format!("{fn_name} simulate failed: {head}"));
    }
    if sim.get("restorePreamble").map(|v| !v.is_null()).unwrap_or(false) {
        return Err(format!("{fn_name}: contract state needs restore (archived entries)"));
    }
    let soroban_data = sim
        .get("transactionData")
        .and_then(|v| v.as_str())
        .ok_or(format!("{fn_name} simulate: no transactionData"))?;
    let soroban_data = SorobanTransactionData::from_xdr_base64(soroban_data, Limits::none())
        .map_err(|e| format!("decode soroban data: {e}"))?;
    let min_resource_fee: u64 = sim
        .get("minResourceFee")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .ok_or(format!("{fn_name} simulate: no minResourceFee"))?;
    let auth = parse_sim_auth(&sim)?;

    let fee = BASE_FEE.saturating_add(u32::try_from(min_resource_fee).unwrap_or(u32::MAX));
    let tx = build_tx(&source, seq, fee, host_function, auth, TransactionExt::V1(soroban_data))?;
    let sig = sign_transaction(&signer, passphrase, &tx)?;
    let signatures: VecM<DecoratedSignature, 20> = vec![sig].try_into().map_err(|_| "signatures")?;
    let env = TransactionEnvelope::Tx(TransactionV1Envelope { tx, signatures });
    let env_b64 = env.to_xdr_base64(Limits::none()).map_err(|e| format!("xdr envelope: {e}"))?;

    submit_and_poll(rpc_url, fn_name, &env_b64)
}

/// Submit one pre-proved bundle's `split` / `transfer4` proof via the relayer. The arg shape mirrors
/// the pool entrypoints exactly: (asset_tag U256, public_inputs Bytes, proof Bytes, enc_notes,
/// ephemeral_pubs, view_tags).
#[allow(clippy::too_many_arguments)]
pub fn submit_bundle(
    rpc_url: &str,
    passphrase: &str,
    relayer_secret: &str,
    pool_contract: &str,
    fn_name: &str,
    asset_tag_hex: &str,
    public_inputs: &[u8],
    proof: &[u8],
    outputs: &[(Vec<u8>, [u8; 32], u32)], // (enc_note, ephemeral_pub, view_tag)
) -> R<String> {
    let enc_notes = sc_vec(outputs.iter().map(|o| sc_bytes(&o.0)).collect::<R<Vec<_>>>()?)?;
    let ephemeral_pubs = sc_vec(outputs.iter().map(|o| sc_bytes(&o.1)).collect::<R<Vec<_>>>()?)?;
    let view_tags = sc_vec(outputs.iter().map(|o| ScVal::U32(o.2)).collect())?;
    let args = vec![
        u256_hex(asset_tag_hex)?,
        sc_bytes(public_inputs)?,
        sc_bytes(proof)?,
        enc_notes,
        ephemeral_pubs,
        view_tags,
    ];
    invoke_contract(rpc_url, passphrase, relayer_secret, pool_contract, fn_name, args)
}
