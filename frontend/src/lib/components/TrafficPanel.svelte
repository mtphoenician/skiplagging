<script lang="ts">
  import { fl, kt } from '$lib/api';
  import type { LiveTraffic } from '$lib/types';

  let { traffic, label }: { traffic: LiveTraffic | null; label: string } = $props();
</script>

{#if traffic}
  <section class="panel">
    <div class="row">
      <div>
        <h3 style="margin:0 0 6px">{label} · {traffic.airport}</h3>
        <div class="book-row">
          {#each traffic.trackers as t}
            <a class="book-btn" href={t.url} target="_blank" rel="noreferrer">{t.name}</a>
          {/each}
        </div>
      </div>
      <div class="note">{traffic.aircraft.length} aircraft</div>
    </div>
    {#if traffic.aircraft.length}
      <table class="mini" style="margin-top:10px">
        <thead>
          <tr>
            <th>Callsign</th>
            <th>Alt</th>
            <th>Speed</th>
          </tr>
        </thead>
        <tbody>
          {#each traffic.aircraft.slice(0, 8) as a}
            <tr>
              <td class="mono">{a.callsign || a.icao24}</td>
              <td>{fl(a.baro_altitude_m)}</td>
              <td>{kt(a.velocity_ms)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="note" style="margin:10px 0 0">No live positions right now.</p>
    {/if}
  </section>
{/if}
