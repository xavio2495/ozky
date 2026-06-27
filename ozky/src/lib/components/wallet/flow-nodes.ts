// Shared types + option lists for the Advanced node-flow editor.
import { ASSETS } from '$lib/assets';

export type Layer = 'shielded' | 'public';

export type SourceData = { token: string; layer: Layer; amount: string };
export type TransformData = { op: 'swap' | 'shield' | 'unshield' | 'consolidate'; toToken: string };
export type DestData = {
	kind: 'self' | 'shielded' | 'public' | 'escrow';
	address: string;
	escrowId: string;
};

export const TOKENS = ASSETS.map((a) => a.code);
