//! In-VM integration tests for the vendored UltraHonk verifier contract,
//! exercised against ozky's production `transfer` circuit (the 2-in/2-out
//! confidential-transfer core, Z1 spec section 5).
//!
//! Fixtures in `tests/fixtures/transfer/` are produced by `circuits/transfer`
//! with `bb prove/write_vk --scheme ultra_honk --oracle_hash keccak` (Noir
//! beta.9 + bb 0.87.0) over the witness in `circuits/transfer/Prover.toml`.
//! `public_inputs` is 352 bytes = the 11 spec public inputs (the 16 pairing
//! points live in the proof). Regeneration documented in `contracts/VENDOR.md`.

use rs_soroban_ultrahonk::{Error, UltraHonkVerifierContract, UltraHonkVerifierContractClient};
use soroban_sdk::{Bytes, Env};
use ultrahonk_soroban_verifier::PROOF_BYTES;
use ultrahonk_test_utils::{mutate_byte, truncate};

const VK: &[u8] = include_bytes!("fixtures/transfer/vk");
const PROOF: &[u8] = include_bytes!("fixtures/transfer/proof");
const PUBLIC_INPUTS: &[u8] = include_bytes!("fixtures/transfer/public_inputs");

fn register_client<'a>(env: &'a Env, vk_bytes: &Bytes) -> UltraHonkVerifierContractClient<'a> {
    let contract_id = env.register(UltraHonkVerifierContract, (vk_bytes.clone(),));
    UltraHonkVerifierContractClient::new(env, &contract_id)
}

#[test]
fn verify_transfer_proof_succeeds() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    assert_eq!(PROOF.len(), PROOF_BYTES);

    let vk_bytes = Bytes::from_slice(&env, VK);
    let proof_bytes: Bytes = Bytes::from_slice(&env, PROOF);
    let public_inputs: Bytes = Bytes::from_slice(&env, PUBLIC_INPUTS);

    let client = register_client(&env, &vk_bytes);
    client.verify_proof(&public_inputs, &proof_bytes);
}

#[test]
fn print_budget_for_deploy_and_verify() {
    let env = Env::default();

    // Measure deploy budget usage.
    env.cost_estimate().budget().reset_unlimited();
    let vk_bytes = Bytes::from_slice(&env, VK);
    let client = register_client(&env, &vk_bytes);

    println!("=== Deploy budget usage ===");
    env.cost_estimate().budget().print();

    // Prepare proof inputs
    assert_eq!(PROOF.len(), PROOF_BYTES);
    let proof_bytes: Bytes = Bytes::from_slice(&env, PROOF);
    let public_inputs: Bytes = Bytes::from_slice(&env, PUBLIC_INPUTS);

    // Measure verify_proof invocation budget usage in isolation.
    env.cost_estimate().budget().reset_unlimited();
    client.verify_proof(&public_inputs, &proof_bytes);
    println!("=== verify_proof budget usage ===");
    env.cost_estimate().budget().print();
}

// =========================================================================
// Constructor negative tests
// =========================================================================

#[test]
fn constructor_rejects_empty_vk() {
    let result = std::panic::catch_unwind(|| {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let empty_vk = Bytes::new(&env);
        let _ = env.register(UltraHonkVerifierContract, (empty_vk,));
    });
    let panic = result.expect_err("expected constructor to panic");
    let msg = panic
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .unwrap_or("");
    assert!(
        msg.contains("Error(Contract, #1)"),
        "constructor should fail with VkInvalidLength (#1), got: {msg}"
    );
}

#[test]
fn constructor_rejects_truncated_vk() {
    let result = std::panic::catch_unwind(|| {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let truncated = truncate(VK, VK.len() - 1);
        let bad_vk = Bytes::from_slice(&env, &truncated);
        let _ = env.register(UltraHonkVerifierContract, (bad_vk,));
    });
    let panic = result.expect_err("expected constructor to panic");
    let msg = panic
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .unwrap_or("");
    assert!(
        msg.contains("Error(Contract, #1)"),
        "constructor should fail with VkInvalidLength (#1), got: {msg}"
    );
}

#[test]
fn constructor_rejects_invalid_parameters() {
    let result = std::panic::catch_unwind(|| {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let mut bad_vk = VK.to_vec();
        // log_circuit_size is the second u64 at bytes 8..16.
        bad_vk[15] = 29;
        let bad_vk = Bytes::from_slice(&env, &bad_vk);
        let _ = env.register(UltraHonkVerifierContract, (bad_vk,));
    });
    let panic = result.expect_err("expected constructor to panic");
    let msg = panic
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .unwrap_or("");
    assert!(
        msg.contains("Error(Contract, #2)"),
        "constructor should fail with VkInvalidParameters (#2), got: {msg}"
    );
}

#[test]
fn constructor_rejects_double_initialization() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let vk = Bytes::from_slice(&env, VK);

    let contract_id = env.register(UltraHonkVerifierContract, (vk.clone(),));

    let err = env
        .as_contract(&contract_id, || {
            UltraHonkVerifierContract::__constructor(env.clone(), vk.clone())
        })
        .expect_err("expected AlreadyInitialized");
    assert_eq!(err as u32, Error::AlreadyInitialized as u32);
}

// =========================================================================
// Verify-method negative tests
// =========================================================================

#[test]
fn verify_proof_with_bad_proof_length_fails() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let vk = Bytes::from_slice(&env, VK);
    let public_inputs = Bytes::from_slice(&env, PUBLIC_INPUTS);

    let contract_id = env.register(UltraHonkVerifierContract, (vk.clone(),));

    let bad_proof = Bytes::from_slice(&env, &[0u8; 10]);
    let err = env
        .as_contract(&contract_id, || {
            UltraHonkVerifierContract::verify_proof(
                env.clone(),
                public_inputs.clone(),
                bad_proof.clone(),
            )
        })
        .expect_err("expected ProofParseError");
    assert_eq!(err as u32, Error::ProofParseError as u32);
}

#[test]
fn verify_proof_with_mutated_proof_fails() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let vk = Bytes::from_slice(&env, VK);
    let proof = Bytes::from_slice(&env, PROOF);
    let public_inputs = Bytes::from_slice(&env, PUBLIC_INPUTS);

    let contract_id = env.register(UltraHonkVerifierContract, (vk.clone(),));

    let bad_proof = Bytes::from_slice(&env, &mutate_byte(&proof.to_alloc_vec(), 100, 0x01));
    let err = env
        .as_contract(&contract_id, || {
            UltraHonkVerifierContract::verify_proof(
                env.clone(),
                public_inputs.clone(),
                bad_proof.clone(),
            )
        })
        .expect_err("expected VerificationFailed");
    assert_eq!(err as u32, Error::VerificationFailed as u32);
}

#[test]
fn verify_proof_with_mutated_public_inputs_fails() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let vk = Bytes::from_slice(&env, VK);
    let proof = Bytes::from_slice(&env, PROOF);

    let contract_id = env.register(UltraHonkVerifierContract, (vk.clone(),));

    // Flip a byte of one public input: the proof no longer matches the claimed inputs.
    let bad_pi = Bytes::from_slice(&env, &mutate_byte(PUBLIC_INPUTS, 31, 0x01));
    let err = env
        .as_contract(&contract_id, || {
            UltraHonkVerifierContract::verify_proof(env.clone(), bad_pi.clone(), proof.clone())
        })
        .expect_err("expected VerificationFailed");
    assert_eq!(err as u32, Error::VerificationFailed as u32);
}
