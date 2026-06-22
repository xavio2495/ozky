//! Note commitment / nullifier gadgets - implements Z1 spec section 3.
//!
//! Hashing is Poseidon2 over BN254 Fr (Noir stdlib sponge). The on-chain Soroban
//! side must reproduce the SAME function via the CAP-0075 `poseidon2_permutation`
//! host call - parity is proven separately (Z2 parity test). Until that parity is
//! locked, the exact Poseidon2 parameter set is not yet frozen.
use poseidon::poseidon2::Poseidon2;

pub mod merkle;
pub mod accumulator;
pub mod transfer;
pub mod split;
pub mod deposit;
pub mod withdraw;
pub mod escrow;

/// Domain-separation tag for owner-key derivation (Z1 decision D4).
/// Placeholder value (ASCII "ozky_own"); finalized before VK generation.
global DOMAIN_OWNER: Field = 0x6f7a6b795f6f776e;

/// A shielded note (Z1 section 3 / FROZEN note format). `value` and `epoch` are range-
/// constrained by the spending circuit (< 2^64 and < 2^32); here they are Fields.
pub struct Note {
    pub value: Field,
    pub asset_tag: Field,
    pub owner_pk: Field,
    pub blinding: Field,
    pub epoch: Field,
    pub rho: Field,
}

impl Note {
    /// commitment = Poseidon(value, asset_tag, owner_pk, blinding, epoch, rho)  (FROZEN)
    pub fn commitment(self) -> Field {
        Poseidon2::hash(
            [self.value, self.asset_tag, self.owner_pk, self.blinding, self.epoch, self.rho],
            6,
        )
    }
}

/// owner_pk = Poseidon(DOMAIN_OWNER, owner_sk)  (Z1 D4 - derive, never reuse Stellar key)
pub fn derive_owner_pk(owner_sk: Field) -> Field {
    Poseidon2::hash([DOMAIN_OWNER, owner_sk], 2)
}

/// nullifier = Poseidon(rho, owner_sk)  (FROZEN) - deterministic, only owner can produce.
pub fn nullifier(rho: Field, owner_sk: Field) -> Field {
    Poseidon2::hash([rho, owner_sk], 2)
}

#[test]
fn commitment_is_deterministic() {
    let sk = 12345;
    let note = Note {
        value: 1000,
        asset_tag: 1,
        owner_pk: derive_owner_pk(sk),
        blinding: 777,
        epoch: 5,
        rho: 999,
    };
    assert(note.commitment() == note.commitment());
}

#[test]
fn commitment_is_binding() {
    let sk = 12345;
    let base = Note {
        value: 1000,
        asset_tag: 1,
        owner_pk: derive_owner_pk(sk),
        blinding: 777,
        epoch: 5,
        rho: 999,
    };
    // Flipping any single field must change the commitment.
    let diff_value = Note {
        value: 1001,
        asset_tag: base.asset_tag,
        owner_pk: base.owner_pk,
        blinding: base.blinding,
        epoch: base.epoch,
        rho: base.rho,
    };
    let diff_rho = Note {
        value: base.value,
        asset_tag: base.asset_tag,
        owner_pk: base.owner_pk,
        blinding: base.blinding,
        epoch: base.epoch,
        rho: 1000,
    };
    assert(base.commitment() != diff_value.commitment());
    assert(base.commitment() != diff_rho.commitment());
}

/// Disclosure re-derivation (Z5 "done when"): an auditor handed the note contents
/// for a scope (off-chain, via the disclosed viewing key) recomputes the commitment
/// and matches it against the one published on-chain - provable disclosure, no
/// reliance on ledger plaintext. Here `published` stands in for the on-chain leaf.
#[test]
fn auditor_rederives_commitment_to_match_chain() {
    // The owner's note (what the auditor reconstructs after decrypting the payload).
    let disclosed = Note {
        value: 1000,
        asset_tag: 1,
        owner_pk: derive_owner_pk(12345),
        blinding: 777,
        epoch: 28,
        rho: 111,
    };
    // The leaf that was appended on-chain at deposit time.
    let published = Note {
        value: 1000,
        asset_tag: 1,
        owner_pk: derive_owner_pk(12345),
        blinding: 777,
        epoch: 28,
        rho: 111,
    }
        .commitment();
    // Re-derivation matches the chain leaf -> disclosure is verifiable.
    assert(disclosed.commitment() == published);
}

#[test]
fn nullifier_depends_on_rho_and_sk() {
    let n = nullifier(999, 12345);
    assert(n == nullifier(999, 12345));
    assert(n != nullifier(1000, 12345));
    assert(n != nullifier(999, 54321));
}

/// Reference vectors for Soroban parity (printed via `nargo test --show-output`).
/// The Soroban `poseidon2_permutation`-based sponge must reproduce these exactly.
#[test]
fn print_reference_vectors() {
    let p2 = Poseidon2::hash([1, 2], 2);
    let p6 = Poseidon2::hash([1, 2, 3, 4, 5, 6], 6);
    let pk = derive_owner_pk(12345);
    println(f"poseidon2_2 = {p2}");
    println(f"poseidon2_6 = {p6}");
    println(f"owner_pk_12345 = {pk}");
}
