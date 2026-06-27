# ui_archive — retired UI scaffold

These are the original Tauri/SvelteKit **test-harness routes**, superseded by the redesigned UI and
moved here on 2026-06-27. They live **outside `src/`** so SvelteKit no longer registers them as routes
and `svelte-check` does not scan them. They are **unlinked** — nothing in the app navigates to them
(no nav item, `href`, `goto`, or import references).

Where each feature now lives:

| Archived route | Replaced by |
| --- | --- |
| `deposit` `withdraw` `send` `receive` `split` | `/wallet` (Self · Send · Multi-send · QR · Advanced) |
| `subscriptions` `escrow` | `/payroll` (subscriptions · escrow · channels) |
| `login-04` `otp-04` `signup-04` | Onboarding / SignIn components in `src/lib/components/onboarding` |

Kept (not deleted) for reference. Safe to remove entirely once the redesign has been validated in
production.
