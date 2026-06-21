//! Transaction signing (Phase A3 / FEATURE_SET G14). Native Ed25519 signing of a
//! Stellar transaction envelope, so the wallet (or relayer) secret never leaves the
//! Rust core — replacing the previous path that forwarded the secret into the ZK
//! Docker container for the `stellar` CLI to sign.
//!
//! This is the signing primitive only: [`chain`](super::chain) builds the
//! `InvokeHostFunction` transaction, simulates it, then calls [`sign_transaction`] to
//! produce the `DecoratedSignature`, and submits via RPC.

use super::CoreError;
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use stellar_xdr::curr::{
    AccountId, DecoratedSignature, Hash, Limits, MuxedAccount, PublicKey, Signature, SignatureHint,
    Transaction, TransactionSignaturePayload, TransactionSignaturePayloadTaggedTransaction,
    Uint256, WriteXdr,
};

/// A wallet/relayer Ed25519 signer derived from a Stellar secret seed (`S…`). The
/// secret key lives only inside this struct (and the `ed25519_dalek` key it wraps); it
/// is never serialized or logged.
pub struct Signer {
    signing: SigningKey,
    public: [u8; 32],
}

impl Signer {
    /// Parse a Stellar secret seed (`S…`) into a signer.
    pub fn from_secret(secret: &str) -> Result<Signer, CoreError> {
        let sk = stellar_strkey::ed25519::PrivateKey::from_string(secret)
            .map_err(|e| CoreError::Chain(format!("invalid source secret: {e}")))?;
        let signing = SigningKey::from_bytes(&sk.0);
        let public = signing.verifying_key().to_bytes();
        Ok(Signer { signing, public })
    }

    /// The signer's Ed25519 public key bytes.
    pub fn public_bytes(&self) -> [u8; 32] {
        self.public
    }

    /// The signer's classic account id (`AccountId` for ledger-key lookups).
    pub fn account_id(&self) -> AccountId {
        AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(self.public)))
    }

    /// The signer as a transaction source account.
    pub fn muxed(&self) -> MuxedAccount {
        MuxedAccount::Ed25519(Uint256(self.public))
    }

    fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }
}

/// The network id: `SHA-256(network_passphrase)` (e.g. testnet's
/// `cee0302d…ecd472`). Bound into every signature so a tx can't be replayed on
/// another network.
pub fn network_id(passphrase: &str) -> [u8; 32] {
    Sha256::digest(passphrase.as_bytes()).into()
}

/// The 32-byte payload a Stellar signature is computed over:
/// `SHA-256( XDR(TransactionSignaturePayload{ network_id, Tx(tx) }) )`.
pub fn tx_signature_hash(passphrase: &str, tx: &Transaction) -> Result<[u8; 32], CoreError> {
    let payload = TransactionSignaturePayload {
        network_id: Hash(network_id(passphrase)),
        tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(tx.clone()),
    };
    let bytes = payload
        .to_xdr(Limits::none())
        .map_err(|e| CoreError::Chain(format!("xdr signature payload: {e}")))?;
    Ok(Sha256::digest(&bytes).into())
}

/// Sign `tx` for `passphrase`'s network, returning the `DecoratedSignature` to attach
/// to the transaction envelope. The signature hint is the last 4 bytes of the public
/// key (how a validator locates this signer's key).
pub fn sign_transaction(
    signer: &Signer,
    passphrase: &str,
    tx: &Transaction,
) -> Result<DecoratedSignature, CoreError> {
    let hash = tx_signature_hash(passphrase, tx)?;
    let sig = signer.signing.sign(&hash);
    let p = signer.public;
    let hint = SignatureHint([p[28], p[29], p[30], p[31]]);
    let signature = Signature(
        sig.to_bytes()
            .to_vec()
            .try_into()
            .map_err(|_| CoreError::Chain("signature length".into()))?,
    );
    Ok(DecoratedSignature { hint, signature })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signature as DalekSignature;
    use stellar_xdr::curr::{
        BumpSequenceOp, Memo, Operation, OperationBody, Preconditions, SequenceNumber, Transaction,
        TransactionExt, VecM,
    };

    /// A signer from a deterministic 32-byte seed (encoded as a real `S…` strkey, so
    /// this also exercises `from_secret`'s strkey parse).
    fn test_signer() -> Signer {
        let seed = [7u8; 32];
        let s = stellar_strkey::ed25519::PrivateKey(seed).to_string();
        Signer::from_secret(&s).unwrap()
    }

    fn dummy_tx(signer: &Signer) -> Transaction {
        let op = Operation {
            source_account: None,
            body: OperationBody::BumpSequence(BumpSequenceOp {
                bump_to: SequenceNumber(0),
            }),
        };
        let operations: VecM<Operation, 100> = vec![op].try_into().unwrap();
        Transaction {
            source_account: signer.muxed(),
            fee: 100,
            seq_num: SequenceNumber(1),
            cond: Preconditions::None,
            memo: Memo::None,
            operations,
            ext: TransactionExt::V0,
        }
    }

    #[test]
    fn testnet_network_id_matches_known_vector() {
        // The canonical Stellar testnet network id.
        let id = network_id("Test SDF Network ; September 2015");
        assert_eq!(
            hex::encode(id),
            "cee0302d59844d32bdca915c8203dd44b33fbb7edc19051ea37abedf28ecd472"
        );
    }

    #[test]
    fn signature_verifies_against_the_public_key() {
        // The produced DecoratedSignature must verify under the signer's public key
        // over the exact payload hash — i.e. sign_transaction binds network + tx.
        let signer = test_signer();
        let tx = dummy_tx(&signer);
        let passphrase = "Test SDF Network ; September 2015";
        let dec = sign_transaction(&signer, passphrase, &tx).unwrap();

        let hash = tx_signature_hash(passphrase, &tx).unwrap();
        let sig_bytes: [u8; 64] = dec.signature.0.to_vec().try_into().unwrap();
        let sig = DalekSignature::from_bytes(&sig_bytes);
        signer
            .verifying_key()
            .verify_strict(&hash, &sig)
            .expect("signature must verify under the signer's key");

        // Hint = last 4 bytes of the public key.
        assert_eq!(dec.hint.0, signer.public_bytes()[28..32]);
    }

    #[test]
    fn different_network_changes_the_signing_hash() {
        // The network id is bound into the payload: same tx, different passphrase ⇒
        // different signing hash (replay protection across networks).
        let signer = test_signer();
        let tx = dummy_tx(&signer);
        let a = tx_signature_hash("Test SDF Network ; September 2015", &tx).unwrap();
        let b = tx_signature_hash("Public Global Stellar Network ; September 2015", &tx).unwrap();
        assert_ne!(a, b);
    }
}
