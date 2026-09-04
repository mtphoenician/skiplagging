<script lang="ts">
  let { a = 'A', b = 'B', c = 'C' }: { a?: string; b?: string; c?: string } = $props();
</script>

<svg class="diagram" viewBox="0 0 520 120" fill="none" aria-hidden="true">
  <path d="M40 70 C 140 20, 240 20, 260 70 S 400 120, 480 70" stroke="#3a3326" stroke-width="2" />
  <circle cx="40" cy="70" r="7" fill="#e3a53a" />
  <circle cx="260" cy="70" r="9" fill="#f3ece1" />
  <circle cx="480" cy="70" r="7" stroke="#8b93a0" stroke-width="2" />
  <text x="40" y="28" text-anchor="middle" fill="#f7f4ee" font-size="14" font-family="IBM Plex Mono">{a}</text>
  <text x="260" y="28" text-anchor="middle" fill="#f7f4ee" font-size="14" font-family="IBM Plex Mono">{b}</text>
  <text x="480" y="28" text-anchor="middle" fill="#8b93a0" font-size="14" font-family="IBM Plex Mono">{c}</text>
  <text x="260" y="108" text-anchor="middle" fill="#e3a53a" font-size="11" font-family="IBM Plex Sans">true destination</text>
  <text x="480" y="108" text-anchor="middle" fill="#8b93a0" font-size="11" font-family="IBM Plex Sans">ticketed C</text>
</svg>
