//! ozky wallet — Tauri library entry. `main.rs` calls [`run`]. The product's logic
//! lives in [`core`]; [`commands`] exposes the `invoke` surface to the Svelte UI.

mod commands;
// `pub` so the `ozky-keeper` binary (src/bin/ozky-keeper.rs) can reuse the keeper submit core.
pub mod core;

/// Point the prover at the bundled no-Docker assets when nothing else configured it.
///
/// A packaged install has no dev `ozky.config.json` and no env, so `OZKY_PROVER_BIN` is
/// unset — fall back to the resources bundled by `stage-prover-bundle.mjs` (the SEA
/// binary + WASM under `prover/`, the circuits + frozen VKs under `zk/`). In dev the
/// config file sets these, so this is a no-op and the dev paths win.
fn wire_bundled_prover(app: &tauri::App) {
    use tauri::Manager;
    if core::config::cfg_var("OZKY_PROVER_BIN").is_some() {
        return; // dev config / env already chose a prover
    }
    let Ok(res) = app.path().resource_dir() else { return };
    let prover_dir = res.join("prover-bundle").join("prover");
    let bin = prover_dir.join(if cfg!(windows) { "ozky-prover.exe" } else { "ozky-prover" });
    if !bin.exists() {
        return; // no bundled prover (e.g. `tauri dev` without staging) — Docker fallback
    }
    std::env::set_var("OZKY_PROVER_BIN", &bin);
    std::env::set_var("OZKY_PROVER_ASSETS", &prover_dir);
    std::env::set_var("OZKY_REPO_ROOT", res.join("prover-bundle").join("zk"));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            wire_bundled_prover(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::wallet_status,
            commands::create_wallet,
            commands::restore_wallet,
            commands::unlock,
            commands::lock,
            commands::verify_totp,
            commands::list_accounts,
            commands::create_account,
            commands::import_account,
            commands::switch_account,
            commands::asset_prices,
            commands::price_history,
            commands::public_balances,
            commands::public_history,
            commands::shielded_history,
            commands::record_activity,
            commands::balance,
            commands::spending_key,
            commands::enroll,
            commands::deposit,
            commands::ensure_trustlines,
            commands::send,
            commands::consolidate,
            commands::split,
            commands::list_payrolls,
            commands::save_payroll,
            commands::delete_payroll,
            commands::set_payroll_enabled,
            commands::run_payroll,
            commands::arm_payroll_keeper,
            commands::disarm_payroll_keeper,
            commands::keeper_status,
            commands::keeper_endpoint,
            commands::set_keeper_endpoint,
            commands::set_local_keeper,
            commands::local_keeper_status,
            commands::list_subscriptions,
            commands::save_subscription,
            commands::delete_subscription,
            commands::set_subscription_enabled,
            commands::run_subscription,
            commands::list_escrows,
            commands::open_escrow,
            commands::contribute_escrow,
            commands::release_escrow,
            commands::refund_escrow,
            commands::list_channels,
            commands::open_channel,
            commands::close_channel,
            commands::reclaim_channel,
            commands::import_channel,
            commands::withdraw,
            commands::swap_quote,
            commands::swap,
            commands::pay_quote,
            commands::pay,
            commands::multi_send,
            commands::funding_address,
            commands::receive_address,
            commands::share_with_auditor,
            commands::audit_disclosure,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
