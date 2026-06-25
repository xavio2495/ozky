//! ozky wallet — Tauri library entry. `main.rs` calls [`run`]. The product's logic
//! lives in [`core`]; [`commands`] exposes the `invoke` surface to the Svelte UI.

mod commands;
mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            commands::send,
            commands::consolidate,
            commands::split,
            commands::list_payrolls,
            commands::save_payroll,
            commands::delete_payroll,
            commands::set_payroll_enabled,
            commands::run_payroll,
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
