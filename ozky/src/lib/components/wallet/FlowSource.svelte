<script lang="ts">
	import { onMount } from 'svelte';
	import { Handle, Position, useSvelteFlow, useUpdateNodeInternals, type NodeProps } from '@xyflow/svelte';
	import * as Select from '$lib/components/ui/select';
	import { Input } from '$lib/components/ui/input';
	import XIcon from '@lucide/svelte/icons/x';
	import { TOKENS, type SourceData } from './flow-nodes';

	// Re-measure handle bounds after the node's controls have laid out, so the edge
	// anchors to the handle's final position (not its pre-hydration position).
	let { id, data }: NodeProps = $props();
	const d = $derived(data as SourceData);
	const { updateNodeData, deleteElements } = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();
	onMount(() => requestAnimationFrame(() => updateNodeInternals(id)));
</script>

<div class="node">
	<Handle type="source" position={Position.Right} />
	<div class="node-head">
		<span>Source</span>
		<button class="del nodrag" aria-label="Delete node" onclick={() => deleteElements({ nodes: [{ id }] })}>
			<XIcon class="size-3" />
		</button>
	</div>
	<div class="node-body">
		<Select.Root type="single" value={d.token} onValueChange={(v) => updateNodeData(id, { token: v })}>
			<Select.Trigger class="h-7 bg-popover text-xs">{d.token}</Select.Trigger>
			<Select.Content class="bg-popover">
				{#each TOKENS as t (t)}<Select.Item value={t}>{t}</Select.Item>{/each}
			</Select.Content>
		</Select.Root>
		<Select.Root type="single" value={d.layer} onValueChange={(v) => updateNodeData(id, { layer: v })}>
			<Select.Trigger class="h-7 bg-popover text-xs">{d.layer === 'shielded' ? 'Shielded' : 'Public'}</Select.Trigger>
			<Select.Content class="bg-popover">
				<Select.Item value="shielded">Shielded</Select.Item>
				<Select.Item value="public">Public</Select.Item>
			</Select.Content>
		</Select.Root>
		<Input
			value={d.amount}
			oninput={(e) => updateNodeData(id, { amount: e.currentTarget.value })}
			placeholder="0.00"
			inputmode="decimal"
			class="h-7 font-mono text-xs"
		/>
	</div>
	<!-- <Handle type="source" position={Position.Right} /> -->
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
