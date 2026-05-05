<script lang="ts">
  type Props = {
    name: string;
    volume: number;
    muted: boolean;
    onVolumeChange: (v: number) => void;
    onToggleMute: () => void;
  };
  let { name, volume, muted, onVolumeChange, onToggleMute }: Props = $props();
</script>

<div class="strip" class:muted>
  <div class="value">{volume}</div>
  <input
    type="range"
    min="0"
    max="100"
    step="1"
    value={volume}
    oninput={(e) => onVolumeChange(Number(e.currentTarget.value))}
    aria-label="{name} volume"
  />
  <button
    class="mute"
    aria-pressed={muted}
    onclick={onToggleMute}
  >{muted ? "Unmute" : "Mute"}</button>
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

  .value {
    font-variant-numeric: tabular-nums;
    font-size: 0.85rem;
    min-width: 2.5em;
    text-align: center;
    color: var(--text-muted);
  }

  input[type="range"] {
    writing-mode: vertical-lr;
    direction: rtl;
    width: 24px;
    height: 180px;
    cursor: pointer;
    accent-color: var(--accent);
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

  .name {
    font-size: 0.78rem;
    max-width: 70px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
