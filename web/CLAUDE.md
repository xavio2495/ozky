# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`web/` is the **marketing site** for the ozky ZK shielded stablecoin wallet. It is a separate app from the wallet itself:

- `../ozky/` — the actual product: a Tauri desktop ZK wallet (Stellar/Soroban). See `../CLAUDE.md` and `../claude-docs/`.
- `web/` (this dir) — the public marketing site, deployed to **Vercel**. **Scheduled to be built last**, after the wallet. Currently an unmodified `sv create` minimal scaffold (default "Welcome to SvelteKit" page); no real content yet.

## Stack

SvelteKit 2 + **Svelte 5 (runes mode forced on** project-wide via `vite.config.ts`, except `node_modules`), TypeScript (strict), **Tailwind CSS 4** (`@tailwindcss/vite`, with `forms` + `typography` plugins), **mdsvex** (`.md` and `.svx` files compile as Svelte components — useful for content/blog pages), **Vercel adapter**.

## Commands

```bash
npm run dev       # Vite dev server
npm run build     # production build (Vercel adapter)
npm run preview   # preview the production build
npm run check     # svelte-kit sync + svelte-check (typecheck gate)
npm run lint      # prettier --check . && eslint .
npm run format    # prettier --write .
```

`npm run check` and `npm run lint` are the static gates; run both after changes. No test runner is configured.

**Windows gotcha:** `npm run build` compiles fine but the **Vercel adapter fails at the end with `EPERM ... symlink`** — creating symlinks on Windows needs elevated privileges. This is local-only; Vercel builds on Linux. To verify a full build locally, enable Windows Developer Mode (allows non-admin symlinks) or run the terminal as administrator. For routine checks, rely on `npm run check` instead.

## Svelte MCP server (use during the build)

`.mcp.json` wires up the Svelte MCP server (`https://mcp.svelte.dev/mcp`). Use it when writing Svelte/SvelteKit code:

- **`list-sections`** — call FIRST when working on any Svelte/SvelteKit topic to discover available docs sections (titles, use_cases, paths).
- **`get-documentation`** — fetch full content for the relevant sections surfaced by `list-sections` (analyze the `use_cases` field to pick all that apply).
- **`svelte-autofixer`** — run on Svelte code before presenting it; keep calling until it returns no issues or suggestions.
- **`playground-link`** — generate a Playground link from code. Only after user confirmation, and NEVER for code already written to project files.
