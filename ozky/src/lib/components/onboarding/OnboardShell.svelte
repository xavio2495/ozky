<script lang="ts">
  // Branded 2-column onboarding card (adapted from the login-04 / signup-04 / otp-04
  // blocks): form content on the left, a gold brand panel on the right replacing the
  // blocks' placeholder image.
  import * as Card from "$lib/components/ui/card";
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<div class="w-full min-w-[750px]">
  <Card.Root class="overflow-hidden p-0 shadow-2xl">
    <Card.Content class="grid h-[600px] p-0 md:grid-cols-2">
      <div class="flex flex-col overflow-hidden p-8">
        <!-- Fixed-size frame: m-auto vertically centers each page's content and
             overflow-hidden keeps the card from ever growing a scrollbar. Pages are
             kept short enough (multi-step flow) to fit without clipping. -->
        <div class="m-auto w-full">
          {@render children()}
        </div>
      </div>
      <div class="brand-panel hidden md:flex items-center">
        <img src="/brand/icon_nobg.svg" alt="" class="mb-11 size-36" />
        <p
          class="font-heading text-2xl font-semibold tracking-tight text-background"
        >
          Private by default.
        </p>
        <p class="mt-2 max-w-[15rem] text-sm text-background/80">
          Shielded payments on Stellar through ZK proofs
        </p>
      </div>
    </Card.Content>
  </Card.Root>
</div>

<style>
  .brand-panel {
    position: relative;
    flex-direction: column;
    justify-content: center;
    padding: 40px;
    overflow: hidden;
    background: radial-gradient(
      120% 120% at 100% 0%,
      color-mix(in oklch, var(--primary) 80%, white),
      var(--primary)
    );
  }
  .brand-panel::after {
    content: "";
    position: absolute;
    inset: 0;
    background:
      url("/runes/sym_circle_in_circle.svg") -30px 220px / 160px no-repeat,
      url("/runes/sym_hash.svg") 210px -20px / 120px no-repeat,
      url("/runes/sym_psi.svg") 240px 280px / 90px no-repeat;
    opacity: 0.12;
    mix-blend-mode: multiply;
    pointer-events: none;
  }
</style>
