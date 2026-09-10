<script lang="ts">
  import { money, timeAgo } from '$lib/api';
  import type { FareWindow } from '$lib/types';

  let {
    window,
    currency = 'USD',
    was = null,
    compact = false
  }: {
    window: FareWindow | null | undefined;
    currency?: string;
    was?: number | null;
    compact?: boolean;
  } = $props();

  const points = $derived(window?.points ?? []);
  const path = $derived.by(() => {
    if (points.length < 2) return '';
    const prices = points.map((p) => p.price);
    const lo = Math.min(...prices);
    const hi = Math.max(...prices);
    const span = Math.max(hi - lo, 1);
    return points
      .map((p, i) => {
        const x = (i / (points.length - 1)) * 100;
        const y = 22 - ((p.price - lo) / span) * 18;
        return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(' ');
  });
  const last = $derived(points[points.length - 1]);
  const low = $derived(window?.low ?? null);
  const lowAt = $derived(window?.low_at ?? null);
  const showLow = $derived(
    low != null && last != null && Math.abs(low - last.price) > 1 && Boolean(lowAt)
  );
  const high = $derived(window?.high ?? was ?? null);
  const showHigh = $derived(high != null && last != null && high > last.price + 1);
</script>

{#if window && (points.length || window.headline)}
  <div class="spark" class:compact>
    <div class="spark-head">
      <span>Price history</span>
      {#if window.trend === 'rising'}
        <span class="spark-trend up">rising</span>
      {:else if window.trend === 'falling'}
        <span class="spark-trend down">falling</span>
      {/if}
    </div>
    <div class="spark-body">
      <div class="spark-chart" aria-hidden="true">
        {#if path}
          <svg viewBox="0 0 100 24" preserveAspectRatio="none">
            <path d={path} fill="none" stroke="currentColor" stroke-width="1.6" />
            {#if last}
              {@const prices = points.map((p) => p.price)}
              {@const lo = Math.min(...prices)}
              {@const hi = Math.max(...prices)}
              {@const span = Math.max(hi - lo, 1)}
              <circle
                cx="100"
                cy={22 - ((last.price - lo) / span) * 18}
                r="1.8"
                fill="currentColor"
              />
            {/if}
          </svg>
        {:else}
          <svg viewBox="0 0 100 24" preserveAspectRatio="none">
            <path d="M4,12 L96,12" fill="none" stroke="currentColor" stroke-width="1.4" />
            <circle cx="96" cy="12" r="1.8" fill="currentColor" />
          </svg>
        {/if}
        {#if showLow && low != null}
          <span class="spark-low">{money(low, currency)} · {timeAgo(lowAt)}</span>
        {/if}
      </div>
      {#if !compact}
        <div class="spark-prices">
          {#if showHigh && high != null}
            <span class="price-was">{money(high, currency)}</span>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .spark {
    min-width: 9.5rem;
    max-width: 14rem;
    color: var(--ink);
  }
  .spark.compact {
    min-width: 7rem;
    max-width: 9rem;
  }
  .spark-head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 4px;
    color: var(--muted);
    font-size: 0.68rem;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .spark-trend.up {
    color: var(--red);
  }
  .spark-trend.down {
    color: var(--green);
  }
  .spark-body {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .spark-chart {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
  }
  .spark-chart svg {
    display: block;
    width: 100%;
    height: 28px;
  }
  .spark-low {
    display: block;
    margin-top: 2px;
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 600;
  }
  .spark-prices {
    flex: 0 0 auto;
    text-align: right;
  }
  .compact .spark-low {
    font-size: 0.64rem;
  }
</style>
