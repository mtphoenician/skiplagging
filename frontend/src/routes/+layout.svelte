<script lang="ts">
  import '../app.css';
  import { page } from '$app/state';

  let { children } = $props();
  const paper = $derived(page.url.pathname.startsWith('/anatomy'));
</script>

<div class:paper>
  <div class="wrap">
    <nav class="nav">
      <a class="brand" href="/">
        <strong>Skiplagging</strong>
        <span>hidden-city anatomy</span>
      </a>
      <div class="nav-links">
        <a href="/">Search</a>
        <a href="/anatomy">Anatomy</a>
      </div>
    </nav>
  </div>
  {@render children()}
  <div class="wrap">
    <footer class="footer">
      Fare discovery and systems explanation — not booking, legal advice, or a method for evading carrier rules.
      Airlines treat hidden-city / point-beyond ticketing as a prohibited practice. Sequential coupons, baggage, and
      irregular operations can erase the displayed saving.
    </footer>
  </div>
</div>
