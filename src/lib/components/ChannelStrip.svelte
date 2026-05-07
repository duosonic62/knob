<script lang="ts">
  type Props = {
    name: string;
    volume: number;
    muted: boolean;
    routing: boolean;
    disabled: boolean;
    level: number;
    showMeter: boolean;
    onVolumeChange: (v: number) => void;
    onToggleMute: () => void;
    onRoute: () => void;
    onStop: () => void;
  };
  let { name, volume, muted, routing, disabled, level, showMeter, onVolumeChange, onToggleMute, onRoute, onStop }: Props = $props();

  const meterRatio = $derived.by(() => {
    if (!showMeter) return 0;
    const db = 20 * Math.log10(Math.max(level, 1e-6));
    return Math.max(0, Math.min(1, (db + 60) / 60));
  });
</script>

<div class="strip" class:muted class:routing>
  <div class="value">{volume}</div>
  <div class="slider-row">
    <input
      type="range"
      min="0"
      max="100"
      step="1"
      value={volume}
      {disabled}
      oninput={(e) => onVolumeChange(Number(e.currentTarget.value))}
      aria-label="{name} volume"
    />
    {#if showMeter}
      <div class="meter" aria-label="audio level">
        <div class="meter-mask" style:height="{(1 - meterRatio) * 100}%"></div>
      </div>
    {/if}
  </div>
  <button
    class="mute"
    aria-pressed={muted}
    {disabled}
    onclick={onToggleMute}
  >{muted ? "Unmute" : "Mute"}</button>
  <button
    class="route"
    class:active={routing}
    {disabled}
    onclick={routing ? onStop : onRoute}
  >{routing ? "Stop" : "Route"}</button>
  <div class="name" title={name}>{name}</div>
</div>

<style>
  .strip {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    width: 80px;
    padding: 0.75rem 0.5rem;
    border: 1px solid var(--strip-border);
    border-radius: 6px;
    background: var(--strip-bg);
  }

  .strip.muted {
    opacity: 0.55;
  }

  .strip.routing {
    border-color: var(--accent);
  }

  .value {
    font-variant-numeric: tabular-nums;
    font-size: 0.85rem;
    min-width: 2.5em;
    text-align: center;
    color: var(--text-muted);
  }

  .slider-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  input[type="range"] {
    writing-mode: vertical-lr;
    direction: rtl;
    width: 24px;
    height: 180px;
    cursor: pointer;
    accent-color: var(--accent);
  }

  .meter {
    width: 6px;
    height: 180px;
    border: 1px solid var(--strip-border);
    border-radius: 3px;
    overflow: hidden;
    position: relative;
    background: linear-gradient(to top, #22c55e 0%, #22c55e 60%, #eab308 75%, #ef4444 95%);
  }

  .meter-mask {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    background: var(--strip-bg);
    transition: height 50ms linear;
  }

  .mute {
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
    border: 1px solid var(--strip-border);
    border-radius: 4px;
    background: var(--btn-bg);
    color: inherit;
    cursor: pointer;
  }

  .mute[aria-pressed="true"] {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }

  .route {
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
    border: 1px solid var(--strip-border);
    border-radius: 4px;
    background: var(--btn-bg);
    color: inherit;
    cursor: pointer;
  }

  .route.active {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }

  .route:disabled,
  .mute:disabled,
  input[type="range"]:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .name {
    font-size: 0.78rem;
    max-width: 70px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
