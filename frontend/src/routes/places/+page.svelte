<script lang="ts">
  import { fetchCountries } from '$lib/api';
  import type { Country } from '$lib/types';

  let q = $state('');
  let rows = $state<Country[]>([]);
  let error = $state('');
  let timer: ReturnType<typeof setTimeout>;

  function load(query: string) {
    fetchCountries(query)
      .then((r) => {
        rows = r;
        error = '';
      })
      .catch((e: Error) => {
        error = e.message;
      });
  }

  $effect(() => {
    load('');
  });

  function onInput(e: Event) {
    q = (e.target as HTMLInputElement).value;
    clearTimeout(timer);
    timer = setTimeout(() => load(q), 160);
  }
</script>

<svelte:head>
  <title>Places — Skiplagging</title>
</svelte:head>

<div class="wrap page">
  <header class="page-head">
    <p class="eyebrow">Reference</p>
    <h1>Countries</h1>
    <p class="lede">Search a country, code, or capital.</p>
  </header>

  <div class="field places-search">
    <label for="cq">Search</label>
    <input id="cq" value={q} oninput={onInput} placeholder="United States, FR, Tokyo…" />
  </div>

  {#if error}
    <p class="empty">{error}</p>
  {:else}
    <p class="note">{rows.length} countries</p>
    <div class="panel table-scroll">
      <table class="mini">
        <thead>
          <tr>
            <th>Code</th>
            <th>Country</th>
            <th>Capital</th>
            <th>Currency</th>
            <th>Airports</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as c}
            <tr>
              <td class="mono">{c.iso2}</td>
              <td>{c.name}</td>
              <td>{c.capital}</td>
              <td>{c.currency_code}</td>
              <td>{c.airport_count}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
