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
          {#each traffic.trackers as t (t.url)}
            <a class="book-btn" href={t.url} target="_blank" rel="noreferrer">{t.name}</a>
          {/each}
        </div>
      </div>
      <div class="note">{traffic.aircraft.length} aircraft</div>
    </div>
    {#if traffic.note}
      <p class="note" style="margin:10px 0 0">{traffic.note}</p>
    {/if}
    {#if traffic.aircraft.length}
      <div class="table-scroll">
        <table class="mini">
          <thead>
            <tr>
              <th>Callsign</th>
              <th>Alt</th>
              <th>Speed</th>
            </tr>
          </thead>
          <tbody>
            {#each traffic.aircraft.slice(0, 8) as a (a.icao24 || a.callsign)}
              <tr>
                <td class="mono">{a.callsign || a.icao24}</td>
                <td>{fl(a.baro_altitude_m)}</td>
                <td>{kt(a.velocity_ms)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <p class="note" style="margin:10px 0 0">No live positions right now. That is not a missing fare.</p>
    {/if}
  </section>
{/if}
