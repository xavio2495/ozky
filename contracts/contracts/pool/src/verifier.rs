//! Cross-contract call into the (vendored) UltraHonk verifier. The pool holds one
//! verifier address per circuit (deposit/transfer/withdraw); each is a deployment
//! of `rs-soroban-ultrahonk` carrying that circuit's frozen VK. `verify_proof`
//! returns `Ok(())` on success and a contract error otherwise — we collapse either
//! failure mode (host invoke error or verification error) into `VerificationFailed`.

use crate::Error;
use soroban_sdk::{Address, Bytes, Env, IntoVal, InvokeError, Symbol, Val, Vec};

/// Call `verify_proof(public_inputs, proof)` on the verifier contract at `verifier`.
pub fn verify(
    env: &Env,
    verifier: &Address,
    public_inputs: Bytes,
    proof: Bytes,
) -> Result<(), Error> {
    let mut args: Vec<Val> = Vec::new(env);
    args.push_back(public_inputs.into_val(env));
    args.push_back(proof.into_val(env));
    env.try_invoke_contract::<(), InvokeError>(verifier, &Symbol::new(env, "verify_proof"), args)
        .map_err(|_| Error::VerificationFailed)?
        .map_err(|_| Error::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::harness;
    use rs_soroban_ultrahonk::UltraHonkVerifierContract;
    use soroban_sdk::{Bytes, Env};

    // ozky's frozen `transfer` circuit artifacts (Noir beta.9 + bb 0.87.0, keccak).
    const VK: &[u8] = include_bytes!("../../rs-soroban-ultrahonk/tests/fixtures/transfer/vk");
    const PROOF: &[u8] = include_bytes!("../../rs-soroban-ultrahonk/tests/fixtures/transfer/proof");
    const PUBLIC_INPUTS: &[u8] =
        include_bytes!("../../rs-soroban-ultrahonk/tests/fixtures/transfer/public_inputs");

    #[test]
    fn pool_verifies_real_transfer_proof_via_verifier_contract() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        // Deploy the verifier with the transfer VK.
        let vk = Bytes::from_slice(&env, VK);
        let verifier = env.register(UltraHonkVerifierContract, (vk,));
        // Drive the pool's cross-contract verify wrapper.
        let pool = harness(&env);
        let public_inputs = Bytes::from_slice(&env, PUBLIC_INPUTS);
        let proof = Bytes::from_slice(&env, PROOF);
        let ok = env.as_contract(&pool, || {
            verify(&env, &verifier, public_inputs.clone(), proof.clone())
        });
        assert_eq!(ok, Ok(()));
    }

    #[test]
    fn pool_rejects_tampered_proof_via_verifier_contract() {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let vk = Bytes::from_slice(&env, VK);
        let verifier = env.register(UltraHonkVerifierContract, (vk,));
        let pool = harness(&env);
        let public_inputs = Bytes::from_slice(&env, PUBLIC_INPUTS);

        let mut bad = PROOF.to_vec();
        bad[200] ^= 0x01;
        let proof = Bytes::from_slice(&env, &bad);
        let err = env.as_contract(&pool, || {
            verify(&env, &verifier, public_inputs.clone(), proof.clone())
        });
        assert_eq!(err, Err(Error::VerificationFailed));
    }
}
