<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";
  import * as Card from "$lib/components/ui/card";

  type WalletStatus = { initialized: boolean; network: string };

  let status = $state<WalletStatus | null>(null);
  let address = $state<string | null>(null);
  let newMnemonic = $state<string | null>(null);
  let restorePhrase = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  async function run<T>(fn: () => Promise<T>): Promise<T | undefined> {
    busy = true;
    error = null;
    try {
      return await fn();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function refresh() {
    status = (await run(() => invoke<WalletStatus>("wallet_status"))) ?? status;
    if (status?.initialized) {
      address = (await run(() => invoke<string>("receive_address"))) ?? null;
    }
  }

  async function createWallet() {
    const phrase = await run(() => invoke<string>("create_wallet"));
    if (phrase) {
      newMnemonic = phrase;
      await refresh();
    }
  }

  async function restoreWallet() {
    const ok = await run(() => invoke("restore_wallet", { phrase: restorePhrase }));
    if (ok !== undefined) {
      restorePhrase = "";
      newMnemonic = null;
      await refresh();
    }
  }

  $effect(() => {
    refresh();
  });
</script>

<main class="flex min-h-screen items-center justify-center p-6">
  <Card.Root class="w-full max-w-md">
    <Card.Header>
      <Card.Title class="font-heading text-2xl">ozky</Card.Title>
      <Card.Description>
        Shielded stablecoin wallet
        {#if status}· {status.network}{/if}
      </Card.Description>
    </Card.Header>

    <Card.Content class="flex flex-col gap-4">
      {#if newMnemonic}
        <div class="flex flex-col gap-2 rounded-md border border-border bg-muted p-3">
          <p class="text-sm font-medium">Recovery phrase — write this down</p>
          <p class="font-mono text-sm">{newMnemonic}</p>
          <p class="text-xs text-muted-foreground">
            This is shown once. Anyone with it controls the wallet.
          </p>
        </div>
      {/if}

      {#if status?.initialized}
        <div class="flex flex-col gap-1 text-sm">
          <span class="text-muted-foreground">Receive address</span>
          <span class="font-mono break-all">{address ?? "…"}</span>
        </div>
      {:else}
        <p class="text-sm text-muted-foreground">No wallet yet. Create one, or restore from a phrase.</p>
        <Textarea
          bind:value={restorePhrase}
          placeholder="Enter your 12-word recovery phrase to restore…"
          rows={2}
        />
      {/if}

      {#if error}
        <p class="text-sm text-destructive">{error}</p>
      {/if}
    </Card.Content>

    <Card.Footer class="flex gap-2">
      {#if status?.initialized}
        <Button variant="outline" onclick={refresh} disabled={busy}>Refresh</Button>
      {:else}
        <Button onclick={createWallet} disabled={busy}>Create wallet</Button>
        <Button variant="outline" onclick={restoreWallet} disabled={busy || !restorePhrase.trim()}>
          Restore
        </Button>
      {/if}
    </Card.Footer>
  </Card.Root>
</main>
