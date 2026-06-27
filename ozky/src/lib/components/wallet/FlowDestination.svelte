<script lang="ts">
	import { onMount } from 'svelte';
	import { Handle, Position, useSvelteFlow, useUpdateNodeInternals, type NodeProps } from '@xyflow/svelte';
	import * as Select from '$lib/components/ui/select';
	import { Input } from '$lib/components/ui/input';
	import XIcon from '@lucide/svelte/icons/x';
	import type { DestData } from './flow-nodes';

	let { id, data }: NodeProps = $props();
	const d = $derived(data as DestData);
	const { updateNodeData, deleteElements } = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();
	onMount(() => requestAnimationFrame(() => updateNodeInternals(id)));
	const kindLabel = { self: 'Self', shielded: 'Shielded', public: 'Public', escrow: 'Escrow' };
</script>

<div class="node">
	<Handle type="target" position={Position.Left} />
	<div class="node-head">
		<span>Destination</span>
		<button class="del nodrag" aria-label="Delete node" onclick={() => deleteElements({ nodes: [{ id }] })}>
			<XIcon class="size-3" />
		</button>
	</div>
	<div class="node-body">
		<Select.Root type="single" value={d.kind} onValueChange={(v) => updateNodeData(id, { kind: v })}>
			<Select.Trigger class="h-7 bg-popover text-xs">{kindLabel[d.kind]}</Select.Trigger>
			<Select.Content class="bg-popover">
				<Select.Item value="self">Self</Select.Item>
				<Select.Item value="shielded">Shielded recipient</Select.Item>
				<Select.Item value="public">Public address</Select.Item>
				<Select.Item value="escrow">Escrow contribution</Select.Item>
			</Select.Content>
		</Select.Root>
		{#if d.kind === 'escrow'}
			<Input
				value={d.escrowId}
				oninput={(e) => updateNodeData(id, { escrowId: e.currentTarget.value })}
				placeholder="escrow #id"
				class="h-7 font-mono text-xs"
			/>
			<Input
				value={d.address}
				oninput={(e) => updateNodeData(id, { address: e.currentTarget.value })}
				placeholder="payee ozky…"
				class="h-7 font-mono text-xs"
			/>
		{:else if d.kind !== 'self'}
			<Input
				value={d.address}
				oninput={(e) => updateNodeData(id, { address: e.currentTarget.value })}
				placeholder={d.kind === 'shielded' ? 'ozky…' : 'G…'}
				class="h-7 font-mono text-xs"
			/>
		{/if}
	</div>
</div>

<style>
	.node {
		position: relative;
		width: 150px;
		border: 1px solid color-mix(in oklch, var(--primary) 30%, var(--border));
		border-radius: var(--radius-xl);
		background: oklch(0.2 0 0);
		font-size: 0.75rem;
	}
	.node-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 5px 10px;
		font-weight: 600;
		color: var(--primary);
		background: color-mix(in oklch, var(--primary) 12%, transparent);
		border-radius: var(--radius-xl) var(--radius-xl) 0 0;
	}
	.del {
		display: grid;
		place-items: center;
		width: 18px;
		height: 18px;
		border-radius: 9999px;
		color: var(--muted-foreground);
	}
	.del:hover {
		color: var(--destructive);
		background: color-mix(in oklch, var(--destructive) 14%, transparent);
	}
	.node-body {
		display: flex;
		flex-direction: column;
		gap: 5px;
		padding: 8px;
	}
</style>
