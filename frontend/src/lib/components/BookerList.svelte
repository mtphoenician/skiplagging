<script lang="ts">
  import type { BookerLink } from '$lib/types';

  let {
    links,
    title = 'Compare and book',
    compact = false
  }: { links: BookerLink[]; title?: string; compact?: boolean } = $props();
</script>

{#if compact}
  <div class="book-row">
    {#each links as b}
      <a class="book-btn" href={b.url} target="_blank" rel="noreferrer">{b.name}</a>
    {/each}
  </div>
{:else}
  <section>
    <h2 class="section-title">{title}</h2>
    <p class="note" style="margin-top:0">Opens their search. You finish booking there.</p>
    <div class="book-grid">
      {#each links as b}
        <a class="book-tile" href={b.url} target="_blank" rel="noreferrer">
          <strong>{b.name}</strong>
          <p>{b.issues_ticket ? 'Can issue a ticket' : 'Compares, then hands off'}</p>
        </a>
      {/each}
    </div>
  </section>
{/if}
