<script lang="ts">
  let {
    value = $bindable(''),
    label = 'Date',
    clearable = false,
    min = ''
  }: { value: string; label?: string; clearable?: boolean; min?: string } = $props();

  let open = $state(false);
  let cursor = $state(new Date());
  let root: HTMLDivElement | undefined = $state();

  const week = ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su'];

  const pretty = $derived(format(value));
  const title = $derived(
    cursor.toLocaleString('en-GB', { month: 'long', year: 'numeric' })
  );
  const grid = $derived(open ? cells() : []);

  function format(iso: string) {
    if (!iso) return 'Choose date';
    const [y, m, d] = iso.split('-').map(Number);
    return new Date(y, m - 1, d).toLocaleDateString('en-GB', {
      day: 'numeric',
      month: 'short',
      year: 'numeric'
    });
  }

  function startMonth() {
    if (value) {
      const [y, m] = value.split('-').map(Number);
      cursor = new Date(y, m - 1, 1);
    } else {
      const n = new Date();
      cursor = new Date(n.getFullYear(), n.getMonth(), 1);
    }
  }

  function toggle() {
    open = !open;
    if (open) startMonth();
  }

  function shift(delta: number) {
    cursor = new Date(cursor.getFullYear(), cursor.getMonth() + delta, 1);
  }

  function cells() {
    const y = cursor.getFullYear();
    const m = cursor.getMonth();
    const first = new Date(y, m, 1);
    const pad = (first.getDay() + 6) % 7;
    const dim = new Date(y, m + 1, 0).getDate();
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const out: { iso: string; day: number; muted: boolean; past: boolean; on: boolean }[] = [];
    for (let i = 0; i < pad; i++) {
      const dt = new Date(y, m, i - pad + 1);
      out.push(cell(dt, true, today));
    }
    for (let d = 1; d <= dim; d++) out.push(cell(new Date(y, m, d), false, today));
    while (out.length % 7) {
      const last = out[out.length - 1];
      const [yy, mm, dd] = last.iso.split('-').map(Number);
      out.push(cell(new Date(yy, mm - 1, dd + 1), true, today));
    }
    return out;
  }

  function cell(dt: Date, muted: boolean, today: Date) {
    const iso = `${dt.getFullYear()}-${pad(dt.getMonth() + 1)}-${pad(dt.getDate())}`;
    const beforeMin = Boolean(min && iso < min);
    return { iso, day: dt.getDate(), muted, past: dt < today || beforeMin, on: iso === value };
  }

  function pad(n: number) {
    return String(n).padStart(2, '0');
  }

  function pick(iso: string, past: boolean) {
    if (past) return;
    value = iso;
    open = false;
  }

  $effect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) open = false;
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') open = false;
    };
    window.addEventListener('click', onDoc);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('click', onDoc);
      window.removeEventListener('keydown', onKey);
    };
  });
</script>

<div class="field picker" bind:this={root}>
  <span class="field-label">{label}</span>
  <button class="ctrl" type="button" aria-expanded={open} aria-haspopup="dialog" onclick={toggle}>
    <span>{pretty}</span>
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <rect x="3" y="4.5" width="14" height="12" rx="2" fill="none" stroke="currentColor" stroke-width="1.4" />
      <path d="M3 8h14M7 3v3M13 3v3" fill="none" stroke="currentColor" stroke-width="1.4" />
    </svg>
  </button>
  {#if open}
    <div class="pop cal" role="dialog" aria-label="Choose date">
      <div class="cal-head">
        <button class="icon-btn" type="button" aria-label="Previous month" onclick={() => shift(-1)}>
          <svg viewBox="0 0 20 20" width="16" height="16"><path d="M12 4 6 10l6 6" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
        </button>
        <strong>{title}</strong>
        <button class="icon-btn" type="button" aria-label="Next month" onclick={() => shift(1)}>
          <svg viewBox="0 0 20 20" width="16" height="16"><path d="M8 4l6 6-6 6" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
        </button>
      </div>
      {#if clearable && value}
        <button class="ghost" type="button" onclick={() => { value = ''; open = false; }}>
          Clear
        </button>
      {/if}
      <div class="cal-week">
        {#each week as w}
          <span>{w}</span>
        {/each}
      </div>
      <div class="cal-grid">
        {#each grid as c (c.iso)}
          <button
            class="cal-day"
            class:muted={c.muted}
            class:past={c.past}
            class:on={c.on}
            type="button"
            disabled={c.past}
            onclick={() => pick(c.iso, c.past)}
          >
            {c.day}
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>
