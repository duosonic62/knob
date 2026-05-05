<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import ChannelStrip from "$lib/components/ChannelStrip.svelte";
  import BlackHoleGuide from "$lib/components/BlackHoleGuide.svelte";

  type AudioDeviceInfo = { id: number; name: string };
  type BlackHoleStatus = { installed: boolean; devices: AudioDeviceInfo[] };
  type GateState =
    | { kind: "checking" }
    | { kind: "error"; message: string }
    | { kind: "not-installed" }
    | { kind: "ok" };

  let gate = $state<GateState>({ kind: "checking" });

  async function recheck() {
    gate = { kind: "checking" };
    try {
      const status = await invoke<BlackHoleStatus>("check_blackhole");
      gate = status.installed ? { kind: "ok" } : { kind: "not-installed" };
    } catch (e) {
      gate = { kind: "error", message: String(e) };
    }
  }
  onMount(recheck);

  type Channel = {
    id: string;
    name: string;
    volume: number;
    muted: boolean;
  };

  let channels: Channel[] = $state([
    { id: "spotify",  name: "Spotify",       volume: 75, muted: false },
    { id: "chrome",   name: "Google Chrome", volume: 60, muted: false },
    { id: "discord",  name: "Discord",       volume: 80, muted: false },
    { id: "zoom",     name: "Zoom",          volume: 50, muted: true  },
    { id: "system",   name: "System Audio",  volume: 90, muted: false },
  ]);

  function setVolume(id: string, v: number) {
    const ch = channels.find((c) => c.id === id);
    if (ch) ch.volume = v;
  }

  function toggleMute(id: string) {
    const ch = channels.find((c) => c.id === id);
    if (ch) ch.muted = !ch.muted;
  }
</script>

{#if gate.kind === "checking"}
  <div class="checking">
    <p>確認中…</p>
  </div>
{:else if gate.kind === "error"}
  <div class="gate-error">
    <h2>オーディオデバイスの確認に失敗しました</h2>
    <pre>{gate.message}</pre>
    <button onclick={recheck}>再試行</button>
  </div>
{:else if gate.kind === "not-installed"}
  <BlackHoleGuide onRecheck={recheck} />
{:else}
  <main>
    <h1>Knob Mixer</h1>
    <div class="rack">
      {#each channels as ch (ch.id)}
        <ChannelStrip
          name={ch.name}
          volume={ch.volume}
          muted={ch.muted}
          onVolumeChange={(v) => setVolume(ch.id, v)}
          onToggleMute={() => toggleMute(ch.id)}
        />
      {/each}
    </div>
  </main>
{/if}

<style>
  :global {
    :root {
      --bg: #f5f5f5;
      --text: #1a1a1a;
      --text-muted: #666;
      --strip-bg: #ffffff;
      --strip-border: #d0d0d0;
      --btn-bg: #ffffff;
      --accent: #2a7fff;
    }

    @media (prefers-color-scheme: dark) {
      :root {
        --bg: #1e1e1e;
        --text: #eeeeee;
        --text-muted: #aaaaaa;
        --strip-bg: #2a2a2a;
        --strip-border: #444444;
        --btn-bg: #333333;
        --accent: #4a9bff;
      }
    }

    body {
      margin: 0;
      background: var(--bg);
      color: var(--text);
      font-family: system-ui, sans-serif;
      font-size: 14px;
    }
  }

  main {
    padding: 1.5rem;
  }

  h1 {
    font-size: 1.2rem;
    margin: 0 0 1.25rem;
  }

  .rack {
    display: flex;
    gap: 0.75rem;
    align-items: flex-start;
  }

  .checking {
    display: flex;
    justify-content: center;
    align-items: center;
    height: 100vh;
    color: var(--text-muted);
    font-size: 0.9rem;
  }

  .gate-error {
    max-width: 560px;
    margin: 4rem auto;
    padding: 0 1.5rem;

    h2 {
      font-size: 1rem;
      color: #c00;
      margin: 0 0 0.75rem;
    }

    pre {
      font-family: monospace;
      font-size: 0.82rem;
      background: var(--strip-bg);
      border: 1px solid var(--strip-border);
      border-radius: 6px;
      padding: 0.6rem;
      white-space: pre-wrap;
      word-break: break-all;
      margin: 0 0 1rem;
    }

    button {
      padding: 0.4rem 1rem;
      font-size: 0.9rem;
      border: 1px solid var(--strip-border);
      border-radius: 4px;
      background: var(--btn-bg);
      color: var(--text);
      cursor: pointer;
    }
  }
</style>
