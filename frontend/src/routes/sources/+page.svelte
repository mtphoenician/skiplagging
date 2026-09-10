<script lang="ts">
  import { fetchSources } from '$lib/api';
  import type { SourceDef } from '$lib/types';

  let sources = $state<SourceDef[]>([]);
  let error = $state('');

  $effect(() => {
    fetchSources()
      .then((r) => {
        sources = r.sources;
      })
      .catch((e: Error) => {
        error = e.message;
      });
  });

  function chipClass(s: SourceDef): string {
    switch (s.status) {
      case 'live':
      case 'oauth':
      case 'on':
        return 'chip good';
      case 'sandbox':
        return 'chip warn';
      case 'anonymous':
      case 'off':
        return 'chip';
      default:
        return s.configured ? 'chip good' : 'chip';
    }
  }

  function chipText(s: SourceDef): string {
    if (s.status_label) return s.status_label;
    if (s.id === 'duffel') {
      if (s.status === 'live') return 'Live';
      if (s.status === 'sandbox') return 'Sandbox — offers dropped';
      return 'Needs a key';
    }
    if (s.id === 'opensky') {
      if (s.status === 'oauth') return 'OAuth';
      return 'Anonymous';
    }
    if (s.status === 'on' || s.configured) return 'On';
    if (s.status === 'off' || s.can_price || s.layer === 'schedule-status') return 'Needs a key';
    return 'On';
  }
</script>

<svelte:head>
  <title>Data — Skiplagging</title>
</svelte:head>

<div class="wrap page">
  <header class="page-head">
    <p class="eyebrow">Sources</p>
    <h1>Where the numbers come from</h1>
    <p class="lede">Airports, prices, and live planes are different systems.</p>
  </header>

  {#if error}
    <p class="empty">{error}</p>
  {:else}
    <div class="source-grid">
      {#each sources as s}
        <article class="offer">
          <div class="row">
            <div>
              <span class="chip">{s.layer}</span>
              <span class={chipClass(s)}>{chipText(s)}</span>
              <h3>{s.name}</h3>
            </div>
            <a class="link" href={s.url} target="_blank" rel="noreferrer">Docs</a>
          </div>
          <p class="source-role">{s.role}</p>
          <p class="source-is-not">{s.is_not}</p>
          {#if s.freshness}
            <p class="source-fresh">{s.freshness}</p>
          {/if}
        </article>
      {/each}
    </div>
  {/if}
</div>
