<script lang="ts">
  import ChannelStrip from "$lib/components/ChannelStrip.svelte";

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
</style>
