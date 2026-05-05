<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";

  type Props = {
    onRecheck: () => void;
  };
  let { onRecheck }: Props = $props();

  function openDownloadPage() {
    openUrl("https://existential.audio/blackhole/");
  }
</script>

<main class="guide">
  <h1>BlackHole が検出されませんでした</h1>
  <p class="desc">
    Knob はアプリ別音量制御に <strong>BlackHole 16ch</strong> 仮想オーディオデバイスを使用します。
    以下の手順でインストールしてください。
  </p>

  <section class="step">
    <h2>方法 1 — Homebrew でインストール</h2>
    <pre class="code">brew install blackhole-16ch</pre>
  </section>

  <section class="step">
    <h2>方法 2 — 公式サイトからダウンロード</h2>
    <button class="link-btn" onclick={openDownloadPage}>
      existential.audio/blackhole/ を開く
    </button>
  </section>

  <p class="note">インストール後、「再チェック」ボタンを押してください。</p>
  <button class="recheck" onclick={onRecheck}>再チェック</button>
</main>

<style>
  .guide {
    max-width: 560px;
    margin: 4rem auto;
    padding: 0 1.5rem;
    font-family: system-ui, sans-serif;
    color: var(--text);
  }

  h1 {
    font-size: 1.25rem;
    margin: 0 0 0.75rem;
  }

  h2 {
    font-size: 0.95rem;
    margin: 0 0 0.5rem;
    font-weight: 600;
  }

  .desc {
    font-size: 0.9rem;
    color: var(--text-muted);
    margin: 0 0 1.5rem;
    line-height: 1.5;
  }

  .step {
    margin-bottom: 1.25rem;
  }

  .code {
    font-family: monospace;
    font-size: 0.9rem;
    background: var(--strip-bg);
    border: 1px solid var(--strip-border);
    border-radius: 6px;
    padding: 0.6rem 0.9rem;
    margin: 0;
    user-select: all;
  }

  .link-btn {
    padding: 0.4rem 0.9rem;
    font-size: 0.88rem;
    border: 1px solid var(--accent);
    border-radius: 4px;
    background: transparent;
    color: var(--accent);
    cursor: pointer;
  }

  .link-btn:hover {
    background: var(--accent);
    color: #fff;
  }

  .note {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: 1.5rem 0 0.75rem;
  }

  .recheck {
    padding: 0.45rem 1.2rem;
    font-size: 0.9rem;
    border: 1px solid var(--strip-border);
    border-radius: 4px;
    background: var(--btn-bg);
    color: var(--text);
    cursor: pointer;
  }

  .recheck:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
