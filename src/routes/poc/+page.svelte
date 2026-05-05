<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface AppInfo {
    bundle_id: string;
    name: string;
    pid: number;
  }

  let apps: AppInfo[] = $state([]);
  let capturing: string | null = $state(null); // bundle_id being captured
  let savedWavPath: string | null = $state(null);
  let errorMsg: string | null = $state(null);

  async function loadApps() {
    errorMsg = null;
    try {
      apps = await invoke<AppInfo[]>("list_audio_apps");
    } catch (e) {
      errorMsg = String(e);
    }
  }

  async function startCapture(bundleId: string) {
    errorMsg = null;
    savedWavPath = null;
    try {
      await invoke("start_capture", { bundleId });
      capturing = bundleId;
      console.log("[knob] capture started:", bundleId);
    } catch (e) {
      errorMsg = String(e);
    }
  }

  async function stopCapture() {
    errorMsg = null;
    try {
      const path = await invoke<string>("stop_capture");
      savedWavPath = path;
      capturing = null;
      console.log("[knob] capture stopped, WAV at:", path);
    } catch (e) {
      errorMsg = String(e);
    }
  }
</script>

<main>
  <h1>Knob — ScreenCaptureKit PoC</h1>

  <section class="controls">
    <button onclick={loadApps}>アプリ一覧を取得</button>
    {#if capturing}
      <button class="stop" onclick={stopCapture}>停止して WAV 保存</button>
      <span class="capturing">録音中: {capturing}</span>
    {/if}
  </section>

  {#if errorMsg}
    <p class="error">{errorMsg}</p>
  {/if}

  {#if savedWavPath}
    <p class="saved">WAV 保存先: <code>{savedWavPath}</code></p>
  {/if}

  {#if apps.length > 0}
    <table>
      <thead>
        <tr>
          <th>アプリ名</th>
          <th>Bundle ID</th>
          <th>PID</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each apps as app}
          <tr class={capturing === app.bundle_id ? "active" : ""}>
            <td>{app.name}</td>
            <td class="mono">{app.bundle_id}</td>
            <td>{app.pid}</td>
            <td>
              {#if capturing === app.bundle_id}
                <button class="stop" onclick={stopCapture}>停止</button>
              {:else}
                <button
                  disabled={capturing !== null}
                  onclick={() => startCapture(app.bundle_id)}
                >キャプチャ</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</main>

<style>
  :root {
    font-family: system-ui, sans-serif;
    font-size: 14px;
    color: #1a1a1a;
    background: #f5f5f5;
  }

  @media (prefers-color-scheme: dark) {
    :root { color: #eee; background: #1e1e1e; }
    table { border-color: #444; }
    th { background: #2a2a2a; }
    tr:hover td { background: #2a2a2a; }
  }

  main {
    max-width: 900px;
    margin: 2rem auto;
    padding: 0 1rem;
  }

  h1 { font-size: 1.4rem; margin-bottom: 1.2rem; }

  .controls {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 1rem;
  }

  button {
    padding: 0.4rem 0.9rem;
    border: 1px solid #888;
    border-radius: 4px;
    background: #fff;
    cursor: pointer;
    font-size: 0.9rem;
  }

  button:hover:not(:disabled) { background: #e8e8e8; }
  button:disabled { opacity: 0.4; cursor: default; }
  button.stop { border-color: #c00; color: #c00; }
  button.stop:hover { background: #fee; }

  .capturing { font-size: 0.85rem; color: #c00; }

  .error { color: #c00; font-size: 0.85rem; }
  .saved { font-size: 0.85rem; }
  code { font-family: monospace; background: #e8e8e8; padding: 0.1rem 0.3rem; border-radius: 3px; }

  table { width: 100%; border-collapse: collapse; border: 1px solid #ddd; }
  th { background: #f0f0f0; text-align: left; padding: 0.5rem 0.75rem; font-weight: 600; }
  td { padding: 0.45rem 0.75rem; border-top: 1px solid #eee; }
  tr:hover td { background: #fafafa; }
  tr.active td { background: #fff5f5; }
  .mono { font-family: monospace; font-size: 0.82rem; }
</style>
