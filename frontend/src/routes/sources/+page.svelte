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
              {#if s.configured}
                <span class="chip good">On</span>
              {:else}
                <span class="chip">Needs a key</span>
              {/if}
              <h3>{s.name}</h3>
            </div>
            <a class="link" href={s.url} target="_blank" rel="noreferrer">Docs</a>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>
