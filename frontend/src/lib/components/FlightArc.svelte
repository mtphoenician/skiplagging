<script lang="ts">
  let { a = 'A', b = 'B', c = 'C' }: { a?: string; b?: string; c?: string } = $props();
</script>

<svg class="arc" viewBox="0 0 680 188" fill="none" aria-hidden="true">
  <defs>
    <linearGradient id="arc-line" x1="70" y1="88" x2="560" y2="88" gradientUnits="userSpaceOnUse">
      <stop stop-color="#c9a36a" />
      <stop offset="0.5" stop-color="#1c1915" />
      <stop offset="1" stop-color="#c9a36a" />
    </linearGradient>
  </defs>
  <path
    class="arc-path"
    d="M80 104 C 190 26, 260 26, 330 84 S 450 154, 560 84"
    stroke="url(#arc-line)"
    stroke-width="1.6"
  />
  <circle class="arc-dot" cx="80" cy="104" r="5.5" fill="#1c1915" />
  <circle class="arc-dot delay" cx="330" cy="84" r="7" fill="#c9a36a" />
  <circle cx="560" cy="84" r="5.5" fill="#fff" stroke="#c9a36a" stroke-width="1.6" />
  <text x="80" y="38" text-anchor="middle">{a}</text>
  <text x="330" y="34" text-anchor="middle">{b}</text>
  <text x="560" y="34" text-anchor="middle">{c}</text>
  <text class="arc-cap" x="330" y="150" text-anchor="middle">your city</text>
  <text class="arc-cap mute" x="560" y="142" text-anchor="middle">ticketed</text>
  <text class="arc-cap mute" x="560" y="158" text-anchor="middle">city</text>
</svg>
