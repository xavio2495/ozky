<script lang="ts">
	// Deterministic generated avatar for an account — a gold aztec-rune masked on a
	// charcoal tile with a gold ring, keyed off the account address. No faces (design
	// §7): identity is a rune tile. Same address ⇒ same avatar, every render.
	const RUNES = [
		'sym_back_c', 'sym_circle', 'sym_circle_in_circle', 'sym_circle_v_line', 'sym_circle_x_sq',
		'sym_cross_v', 'sym_curved_x_line', 'sym_half_e', 'sym_hash', 'sym_hourglass', 'sym_line_arcs',
		'sym_psi', 'sym_semicircle', 'sym_sq_in_sq', 'sym_square', 'sym_topline_inv_v',
		'sym_v_cross_line', 'sym_v_doublecross_line', 'sym_window'
	];

	let { seed = '', size = 32 }: { seed?: string; size?: number } = $props();

	// FNV-1a — small, stable, dependency-free.
	function hash(s: string): number {
		let h = 2166136261;
		for (let i = 0; i < s.length; i++) {
			h ^= s.charCodeAt(i);
			h = Math.imul(h, 16777619);
		}
		return h >>> 0;
	}

	const h = $derived(hash(seed));
	const rune = $derived(RUNES[h % RUNES.length]);
	const angle = $derived(h % 360); // tint gradient direction
	const stop = $derived(((h >>> 8) % 5) + 1); // gold-ramp stop chart-1..5
</script>

<span
	class="avatar"
	style="--sz:{size}px; --angle:{angle}deg; --gold:var(--chart-{stop});"
	aria-hidden="true"
>
	<span
		class="rune"
		style="-webkit-mask-image:url(/runes/{rune}.svg); mask-image:url(/runes/{rune}.svg);"
	></span>
</span>

<style>
	.avatar {
		position: relative;
		display: inline-grid;
		place-items: center;
		width: var(--sz);
		height: var(--sz);
		flex-shrink: 0;
		border-radius: 9999px;
		overflow: hidden;
		background:
			linear-gradient(var(--angle), color-mix(in oklch, var(--gold) 16%, transparent), transparent 70%),
			var(--card);
		box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--gold) 35%, var(--border));
	}
	.rune {
		width: 56%;
		height: 56%;
		background: var(--gold);
		-webkit-mask-repeat: no-repeat;
		mask-repeat: no-repeat;
		-webkit-mask-position: center;
		mask-position: center;
		-webkit-mask-size: contain;
		mask-size: contain;
	}
</style>
