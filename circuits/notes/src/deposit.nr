//! `deposit` circuit logic (Z1 spec section 6). A deposit is public (amount visible); this
//! proof binds the public `amount` to the hiding output commitment so a depositor
//! cannot mint a note worth more than they locked.
use crate::Note;

pub struct DepositWitness {
    pub domain_sep: Field,
    pub asset_tag: Field,
    pub epoch: Field,
    pub amount: Field,
    pub out_commitment: Field,
    // private
    pub owner_pk: Field,
    pub blinding: Field,
    pub rho: Field,
}

pub fn verify_deposit(
    domain_sep: Field,
    asset_tag: Field,
    epoch: Field,
    amount: Field,
    out_commitment: Field,
    owner_pk: Field,
    blinding: Field,
    rho: Field,
) {
    assert(domain_sep != 0);
    epoch.assert_max_bit_size::<32>();
    amount.assert_max_bit_size::<64>();
    let note = Note { value: amount, asset_tag, owner_pk, blinding, epoch, rho };
    assert(note.commitment() == out_commitment);
}

impl DepositWitness {
    pub fn run(self) {
        verify_deposit(
            self.domain_sep,
            self.asset_tag,
            self.epoch,
            self.amount,
            self.out_commitment,
            self.owner_pk,
            self.blinding,
            self.rho,
        );
    }
}

/// Deterministic deposit of 1000 to a fresh note, parameterized by `epoch` and
/// `domain_sep` so it can bind to a live pool (current epoch + computed domain_sep).
/// Mints exactly the note that `transfer::demo_witness_at` spends, so deposit and
/// transfer compose.
pub fn demo_witness_at(epoch: Field, domain_sep: Field) -> DepositWitness {
    let asset_tag = 1;
    let owner_pk = crate::derive_owner_pk(12345);
    let amount = 1000;
    let blinding = 777;
    let rho = 111;
    let out_commitment =
        Note { value: amount, asset_tag, owner_pk, blinding, epoch, rho }.commitment();
    DepositWitness {
        domain_sep,
        asset_tag,
        epoch,
        amount,
        out_commitment,
        owner_pk,
        blinding,
        rho,
    }
}

/// Deterministic deposit of 1000 to a fresh note (epoch 5, demo domain_sep).
pub fn demo_witness() -> DepositWitness {
    demo_witness_at(5, 0xabc)
}

#[test]
fn deposit_end_to_end() {
    demo_witness().run();
}

#[test(should_fail)]
fn deposit_rejects_inflated_amount() {
    let mut w = demo_witness();
    w.amount = w.amount + 1; // commitment was built with the original amount
    w.run();
}
