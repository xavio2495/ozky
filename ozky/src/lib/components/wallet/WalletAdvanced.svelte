<script lang="ts">
	// Advanced node-flow editor (xyflow/svelte). Compose Source → Transform → Destination
	// nodes, wire them, preview the linear plan, then execute it as a reviewed sequence of
	// existing API calls. v1: linear chains (one source → steps → one destination).
	import { SvelteFlow, Background, Controls, type Node, type Edge } from '@xyflow/svelte';
	import '@xyflow/svelte/dist/base.css';
	import '@xyflow/svelte/dist/style.css';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import FlowSource from './FlowSource.svelte';
	import FlowTransform from './FlowTransform.svelte';
	import FlowDestination from './FlowDestination.svelte';
	import type { SourceData, TransformData, DestData } from './flow-nodes';
	import { api, errMessage } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { toBaseUnits, assetByCode } from '$lib/assets';
	import { toast } from 'svelte-sonner';

	const SWAP_SLIPPAGE_BPS = 100;
	const PAY_SLIPPAGE_BPS = 100;

	const nodeTypes = {
		source: FlowSource,
		transform: FlowTransform,
		destination: FlowDestination
	};

	const defaultEdgeOptions = { animated: true, style: 'stroke: var(--primary); stroke-width: 2;' };

	let nodes = $state.raw<Node[]>([
		{ id: 's1', type: 'source', position: { x: 30, y: 110 }, data: { token: 'XLM', layer: 'shielded', amount: '' } },
		{ id: 't1', type: 'transform', position: { x: 240, y: 110 }, data: { op: 'swap', toToken: 'USDC' } },
		{ id: 'd1', type: 'destination', position: { x: 450, y: 110 }, data: { kind: 'self', address: '', escrowId: '' } }
	]);
	// Pre-wired Source → Transform → Destination so the flow reads immediately.
	let edges = $state.raw<Edge[]>([
		{ id: 'e-s1-t1', source: 's1', target: 't1', ...defaultEdgeOptions },
		{ id: 'e-t1-d1', source: 't1', target: 'd1', ...defaultEdgeOptions }
	]);
	let seq = 1;

	export function add(type: 'source' | 'transform' | 'destination') {
		const id = `${type[0]}${++seq}${Date.now() % 1000}`;
		const data =
			type === 'source'
				? { token: 'XLM', layer: 'shielded', amount: '' }
				: type === 'transform'
					? { op: 'swap', toToken: 'USDC' }
					: { kind: 'shielded', address: '', escrowId: '' };
		nodes = [...nodes, { id, type, position: { x: 200, y: 260 }, data }];
	}

	function onconnect(conn: { source: string; target: string }) {
		const id = `e-${conn.source}-${conn.target}`;
		if (edges.some((e) => e.id === id)) return;
		edges = [...edges, { id, source: conn.source, target: conn.target, ...defaultEdgeOptions }];
	}

	// Linearize from the source node by following outgoing edges.
	type Step = { label: string };
	let plan = $state<Step[]>([]);
	let planOpen = $state(false);
	let proving = $state(false);
	let provingTitle = $state('Running flow');

	function buildOrder(): Node[] | null {
		const start = nodes.find((n) => n.type === 'source' && !edges.some((e) => e.target === n.id));
		if (!start) return null;
		const order: Node[] = [start];
		let cur = start.id;
		const seen = new Set([cur]);
		while (true) {
			const e = edges.find((x) => x.source === cur);
			if (!e) break;
			const next = nodes.find((n) => n.id === e.target);
			if (!next || seen.has(next.id)) break;
			order.push(next);
			seen.add(next.id);
			cur = next.id;
		}
		return order;
	}

	export function preview() {
		const order = buildOrder();
		if (!order || order.length < 2) {
			toast.error('Wire a Source to a Destination first');
			return;
		}
		const src = order[0].data as SourceData;
		let token = src.token;
		const steps: Step[] = [`Start: ${src.amount || '?'} ${token} (${src.layer})`].map((label) => ({ label }));
		for (const n of order.slice(1)) {
			if (n.type === 'transform') {
				const t = n.data as TransformData;
				if (t.op === 'swap') {
					steps.push({ label: `Swap ${token} → ${t.toToken} (AMM)` });
					token = t.toToken;
				} else if (t.op === 'shield') steps.push({ label: `Shield ${token} (deposit)` });
				else if (t.op === 'unshield') steps.push({ label: `Unshield ${token} (withdraw)` });
				else steps.push({ label: `Consolidate ${token} notes` });
			} else {
				const dd = n.data as DestData;
				steps.push({
					label:
						dd.kind === 'self'
							? `Keep ${token} (self)`
							: dd.kind === 'escrow'
								? `Contribute ${token} → escrow #${dd.escrowId || '?'}`
								: `Send ${token} → ${dd.kind} ${dd.address ? dd.address.slice(0, 10) + '…' : '(no address)'}`
				});
			}
		}
		plan = steps;
		planOpen = true;
	}

	async function execute() {
		planOpen = false;
		const order = buildOrder();
		if (!order) return;
		const src = order[0].data as SourceData;
		let token = src.token;
		const dec = () => assetByCode(token)?.decimals ?? 7;
		let units: number;
		try {
			units = toBaseUnits(src.amount, dec());
		} catch (e) {
			toast.error(errMessage(e));
			return;
		}
		proving = true;
		for (const n of order.slice(1)) {
			let hash: string | undefined;
			if (n.type === 'transform') {
				const t = n.data as TransformData;
				if (t.op === 'swap') {
					provingTitle = `Swapping ${token} → ${t.toToken}`;
					const receipt = await runAction(provingTitle, () => api.swap(token, t.toToken, units, SWAP_SLIPPAGE_BPS), { success: () => 'Swapped' });
					hash = receipt?.tx_hash;
					// Carry the ACTUAL swapped output forward — a stacked step must operate on the
					// USDC (etc.) received, not the original input amount in the new asset's units.
					if (receipt) units = receipt.received;
					token = t.toToken;
				} else if (t.op === 'shield') {
					provingTitle = `Shielding ${token}`;
					hash = await runAction(provingTitle, () => api.deposit(token, units), { success: () => 'Shielded' });
				} else if (t.op === 'unshield') {
					provingTitle = `Unshielding ${token}`;
					const funding = await api.fundingAddress();
					hash = await runAction(provingTitle, () => api.withdraw(token, funding, units), { success: () => 'Unshielded' });
				} else {
					provingTitle = `Consolidating ${token}`;
					hash = await runAction(provingTitle, () => api.consolidate(token), { success: () => 'Consolidated' });
				}
			} else {
				const dd = n.data as DestData;
				if (dd.kind === 'self') continue;
				const addr = dd.address.trim();
				if (dd.kind === 'escrow') {
					provingTitle = `Contributing ${token} to escrow`;
					hash = await runAction(provingTitle, () => api.contributeEscrow(Number(dd.escrowId), addr, units).then(String), { success: () => 'Contributed' });
				} else {
					provingTitle = `Sending ${token}`;
					hash = await runAction(provingTitle, () => (dd.kind === 'public' ? api.withdraw(token, addr, units) : api.send(token, addr, units)), { success: () => 'Sent' });
				}
			}
			if (!hash) {
				proving = false;
				return; // a step failed; stop the chain
			}
		}
		proving = false;
		wallet.log({ kind: 'swap', label: 'Advanced flow executed' });
		await wallet.refreshBalances();
		await wallet.refreshPublicBalances();
		toast.success('Flow complete');
	}
</script>

<div class="adv-wrap">
	<div class="canvas">
		<SvelteFlow bind:nodes bind:edges {nodeTypes} {onconnect} {defaultEdgeOptions} fitView colorMode="dark">
			<Background />
			<Controls />
		</SvelteFlow>
	</div>
	<p class="note">
		Drag from a node's right dot to another's left dot to wire steps. v1 — linear chains; amounts
		carry forward, swap outputs priced live. Ops: swap · shield · unshield · consolidate · send ·
		withdraw · escrow contribution.
	</p>
</div>

<AlertDialog.Root bind:open={planOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Review flow</AlertDialog.Title>
			<AlertDialog.Description>This runs as a sequence of transactions:</AlertDialog.Description>
		</AlertDialog.Header>
		<ol class="plan">
			{#each plan as s, i (i)}<li><span class="n">{i + 1}</span>{s.label}</li>{/each}
		</ol>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={execute}>Execute flow</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title={provingTitle} />

<style>
	.adv-wrap {
		display: flex;
		flex-direction: column;
		gap: 8px;
		flex: 1;
		min-height: 0;
		padding-top: 8px;
	}
	.canvas {
		flex: 1;
		min-height: 420px;
		border: 1px solid var(--border);
		border-radius: var(--radius-2xl);
		overflow: hidden;
		background: color-mix(in oklch, var(--background) 60%, transparent);
	}
	.note {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
	.plan {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.plan li {
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 0.8125rem;
	}
	.plan .n {
		display: grid;
		place-items: center;
		width: 20px;
		height: 20px;
		flex-shrink: 0;
		border-radius: 9999px;
		background: color-mix(in oklch, var(--primary) 16%, transparent);
		color: var(--primary);
		font-size: 0.6875rem;
		font-weight: 600;
	}
	/* themed xyflow surface */
	.canvas :global(.svelte-flow) {
		background: transparent;
	}
	.canvas :global(.svelte-flow__edge-path) {
		stroke: color-mix(in oklch, var(--primary) 60%, transparent);
		stroke-width: 2;
	}
	.canvas :global(.svelte-flow__handle) {
		background: var(--primary);
		border: none;
		width: 8px;
		height: 8px;
	}
	.canvas :global(.svelte-flow__controls-button) {
		background: var(--card);
		border-color: var(--border);
		color: var(--foreground);
		fill: var(--foreground);
	}
</style>
