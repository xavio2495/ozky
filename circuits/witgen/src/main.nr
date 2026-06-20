//! Interim witness generator (until the A2 Rust core produces real witnesses).
//! Given `(epoch, pool_id, network_id)`, prints a Prover.toml for each of the
//! deposit/transfer/withdraw circuits, with `domain_sep` computed the same way the
//! pool contract does: `Poseidon2([pool_id, network_id, selector], 3)`. Each
//! circuit's section sits between ###<NAME>_BEGIN###/###<NAME>_END### markers; a
//! sed pass (`s/0x[0-9a-f]+/"&"/g`) quotes hex into valid TOML.
//!
//! The deposit note == the transfer/withdraw input note, so deposit->transfer and
//! deposit->withdraw compose on a live pool (selectors DEPOSIT=1/TRANSFER=2/WITHDRAW=3).
use notes::deposit::{demo_witness_at as deposit_witness, DepositWitness};
use notes::transfer::{demo_witness_at as transfer_witness, InputWitness, OutputWitness, TransferWitness};
use notes::withdraw::{demo_witness_at as withdraw_witness, WithdrawWitness};
use poseidon::poseidon2::Poseidon2;

fn dsep(pool_id: Field, network_id: Field, selector: Field) -> Field {
    Poseidon2::hash([pool_id, network_id, selector], 3)
}

fn print_input(w: InputWitness) {
    println("");
    println("[[inputs]]");
    let value = w.value;
    println(f"value = {value}");
    let blinding = w.blinding;
    println(f"blinding = {blinding}");
    let epoch = w.epoch;
    println(f"epoch = {epoch}");
    let rho = w.rho;
    println(f"rho = {rho}");
    let is_dummy = w.is_dummy;
    println(f"is_dummy = {is_dummy}");
    let cm_is_right = w.cm_is_right;
    println(f"cm_is_right = {cm_is_right}");
    let cm_siblings = w.cm_siblings;
    println(f"cm_siblings = {cm_siblings}");
    let asp_is_right = w.asp_is_right;
    println(f"asp_is_right = {asp_is_right}");
    let asp_siblings = w.asp_siblings;
    println(f"asp_siblings = {asp_siblings}");
    let nf_low_is_right = w.nf_low_is_right;
    println(f"nf_low_is_right = {nf_low_is_right}");
    let nf_low_siblings = w.nf_low_siblings;
    println(f"nf_low_siblings = {nf_low_siblings}");
    let nf_new_is_right = w.nf_new_is_right;
    println(f"nf_new_is_right = {nf_new_is_right}");
    let nf_new_siblings = w.nf_new_siblings;
    println(f"nf_new_siblings = {nf_new_siblings}");
    let nf_new_index = w.nf_new_index;
    println(f"nf_new_index = {nf_new_index}");
    println("[inputs.nf_low]");
    let nlv = w.nf_low.value;
    println(f"value = {nlv}");
    let nli = w.nf_low.next_index;
    println(f"next_index = {nli}");
    let nlnv = w.nf_low.next_value;
    println(f"next_value = {nlnv}");
}

fn print_deposit(w: DepositWitness) {
    let domain_sep = w.domain_sep;
    println(f"domain_sep = {domain_sep}");
    let asset_tag = w.asset_tag;
    println(f"asset_tag = {asset_tag}");
    let epoch = w.epoch;
    println(f"epoch = {epoch}");
    let amount = w.amount;
    println(f"amount = {amount}");
    let out_commitment = w.out_commitment;
    println(f"out_commitment = {out_commitment}");
    let owner_pk = w.owner_pk;
    println(f"owner_pk = {owner_pk}");
    let blinding = w.blinding;
    println(f"blinding = {blinding}");
    let rho = w.rho;
    println(f"rho = {rho}");
}

fn print_transfer(w: TransferWitness) {
    let domain_sep = w.domain_sep;
    println(f"domain_sep = {domain_sep}");
    let asset_tag = w.asset_tag;
    println(f"asset_tag = {asset_tag}");
    let epoch = w.epoch;
    println(f"epoch = {epoch}");
    let commitment_root = w.commitment_root;
    println(f"commitment_root = {commitment_root}");
    let nullifier_old_root = w.nullifier_old_root;
    println(f"nullifier_old_root = {nullifier_old_root}");
    let nullifier_new_root = w.nullifier_new_root;
    println(f"nullifier_new_root = {nullifier_new_root}");
    let nullifiers = w.nullifiers;
    println(f"nullifiers = {nullifiers}");
    let out_commitments = w.out_commitments;
    println(f"out_commitments = {out_commitments}");
    let asp_root = w.asp_root;
    println(f"asp_root = {asp_root}");
    let owner_sk = w.owner_sk;
    println(f"owner_sk = {owner_sk}");
    print_input(w.inputs[0]);
    print_input(w.inputs[1]);
    print_output(w.outputs[0]);
    print_output(w.outputs[1]);
}

fn print_output(w: OutputWitness) {
    println("");
    println("[[outputs]]");
    let value = w.value;
    println(f"value = {value}");
    let owner_pk = w.owner_pk;
    println(f"owner_pk = {owner_pk}");
    let blinding = w.blinding;
    println(f"blinding = {blinding}");
    let rho = w.rho;
    println(f"rho = {rho}");
}

fn print_withdraw(w: WithdrawWitness) {
    let domain_sep = w.domain_sep;
    println(f"domain_sep = {domain_sep}");
    let asset_tag = w.asset_tag;
    println(f"asset_tag = {asset_tag}");
    let epoch = w.epoch;
    println(f"epoch = {epoch}");
    let commitment_root = w.commitment_root;
    println(f"commitment_root = {commitment_root}");
    let nullifier_old_root = w.nullifier_old_root;
    println(f"nullifier_old_root = {nullifier_old_root}");
    let nullifier_new_root = w.nullifier_new_root;
    println(f"nullifier_new_root = {nullifier_new_root}");
    let nullifiers = w.nullifiers;
    println(f"nullifiers = {nullifiers}");
    let change_commitment = w.change_commitment;
    println(f"change_commitment = {change_commitment}");
    let asp_root = w.asp_root;
    println(f"asp_root = {asp_root}");
    let amount = w.amount;
    println(f"amount = {amount}");
    let dest_bind = w.dest_bind;
    println(f"dest_bind = {dest_bind}");
    let owner_sk = w.owner_sk;
    println(f"owner_sk = {owner_sk}");
    print_input(w.inputs[0]);
    print_input(w.inputs[1]);
    // `change` is a single OutputWitness - emit as a [change] table.
    println("");
    println("[change]");
    let cv = w.change.value;
    println(f"value = {cv}");
    let cpk = w.change.owner_pk;
    println(f"owner_pk = {cpk}");
    let cb = w.change.blinding;
    println(f"blinding = {cb}");
    let cr = w.change.rho;
    println(f"rho = {cr}");
}

fn main(epoch: Field, pool_id: Field, network_id: Field) {
    let dd = dsep(pool_id, network_id, 1);
    let dt = dsep(pool_id, network_id, 2);
    let dw = dsep(pool_id, network_id, 3);

    println("###DEPOSIT_BEGIN###");
    print_deposit(deposit_witness(epoch, dd));
    println("###DEPOSIT_END###");

    println("###TRANSFER_BEGIN###");
    print_transfer(transfer_witness(epoch, dt));
    println("###TRANSFER_END###");

    println("###WITHDRAW_BEGIN###");
    print_withdraw(withdraw_witness(epoch, dw));
    println("###WITHDRAW_END###");
}
