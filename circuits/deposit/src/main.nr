//! `deposit` circuit - public-interface wrapper over `notes::deposit::verify_deposit`
//! (Z1 spec section 6). Public inputs in canonical order.
use notes::deposit::verify_deposit;

fn main(
    domain_sep: pub Field,
    asset_tag: pub Field,
    epoch: pub Field,
    amount: pub Field,
    out_commitment: pub Field,
    owner_pk: Field,
    blinding: Field,
    rho: Field,
) {
    verify_deposit(domain_sep, asset_tag, epoch, amount, out_commitment, owner_pk, blinding, rho);
}
