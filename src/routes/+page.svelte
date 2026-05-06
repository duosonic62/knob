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

  type AppInfo = { bundle_id: string; name: string; pid: number };
  type Strip = AppInfo & { volume: number; muted: boolean };

  let apps = $state<Strip[]>([]);
  let routedBundleId = $state<string | null>(null);
  let busy = $state(false);
  let routeError = $state<string | null>(null);

  async function refreshApps() {
    try {
      const list = await invoke<AppInfo[]>("list_audio_apps");
      apps = list
        .filter((a) => a.bundle_id && !a.bundle_id.startsWith("com.apple."))
        .map((a) => ({ ...a, volume: 75, muted: false }));
    } catch (e) {
      routeError = String(e);
    }
  }

  async function onRoute(bundleId: string) {
    busy = true;
    routeError = null;
    try {
      if (routedBundleId) {
        await invoke("stop_routing");
        routedBundleId = null;
      }
      const s = apps.find((a) => a.bundle_id === bundleId)!;
      await invoke("start_routing", {
        bundleId,
        volume: s.volume / 100,
        muted: s.muted,
      });
      routedBundleId = bundleId;
    } catch (e) {
      routeError = String(e);
    } finally {
      busy = false;
    }
  }

  async function onStop() {
    busy = true;
    try {
      await invoke("stop_routing");
      routedBundleId = null;
    } catch (e) {
      routeError = String(e);
    } finally {
      busy = false;
    }
  }

  async function onVolume(bundleId: string, v: number) {
    const s = apps.find((a) => a.bundle_id === bundleId)!;
    s.volume = v;
    if (routedBundleId === bundleId) {
      try {
        await invoke("set_routing_volume", { volume: v / 100 });
      } catch (e) {
        routeError = String(e);
      }
    }
  }

  async function onMute(bundleId: string) {
    const s = apps.find((a) => a.bundle_id === bundleId)!;
    s.muted = !s.muted;
    if (routedBundleId === bundleId) {
      try {
        await invoke("set_routing_mute", { muted: s.muted });
      } catch (e) {
        routeError = String(e);
      }
    }
  }

  $effect(() => {
    if (gate.kind === "ok" && apps.length === 0) refreshApps();
  });
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
    <div class="toolbar">
      <h1>Knob Mixer</h1>
      <button class="refresh" onclick={refreshApps} disabled={busy}>アプリ一覧を更新</button>
    </div>

    {#if routeError}
      <div class="error-toast">
        <span>{routeError}</span>
        <button onclick={() => (routeError = null)}>×</button>
      </div>
    {/if}

    <div class="rack">
      {#each apps as app (app.bundle_id)}
        <ChannelStrip
          name={app.name}
          volume={app.volume}
          muted={app.muted}
          routing={routedBundleId === app.bundle_id}
          disabled={busy}
          onVolumeChange={(v) => onVolume(app.bundle_id, v)}
          onToggleMute={() => onMute(app.bundle_id)}
          onRoute={() => onRoute(app.bundle_id)}
          onStop={onStop}
        />
      {/each}

      {#if apps.length === 0}
        <p class="empty">アプリが見つかりません。「アプリ一覧を更新」を押してください。</p>
      {/if}
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

  .toolbar {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 1.25rem;
  }

  h1 {
    font-size: 1.2rem;
    margin: 0;
  }

  .refresh {
    padding: 0.3rem 0.8rem;
    font-size: 0.82rem;
    border: 1px solid var(--strip-border);
    border-radius: 4px;
    background: var(--btn-bg);
    color: var(--text);
    cursor: pointer;
  }

  .refresh:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }

  .refresh:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .rack {
    display: flex;
    gap: 0.75rem;
    align-items: flex-start;
    flex-wrap: wrap;
  }

  .empty {
    color: var(--text-muted);
    font-size: 0.88rem;
    margin: 0;
  }

  .error-toast {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    background: #fef2f2;
    border: 1px solid #fca5a5;
    border-radius: 6px;
    padding: 0.6rem 0.9rem;
    margin-bottom: 1rem;
    font-size: 0.85rem;
    color: #b91c1c;

    span {
      flex: 1;
      word-break: break-word;
    }

    button {
      background: none;
      border: none;
      cursor: pointer;
      font-size: 1rem;
      color: #b91c1c;
      padding: 0;
      line-height: 1;
    }
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
