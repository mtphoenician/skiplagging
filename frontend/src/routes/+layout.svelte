<script lang="ts">
  import '../app.css';
  import { page } from '$app/state';

  let { children } = $props();
  const path = $derived(page.url.pathname);
</script>

<div class="atmosphere" aria-hidden="true"></div>
<div class="app">
  <div class="wrap">
    <nav class="nav">
      <a class="brand" href="/">
        <i aria-hidden="true">
          <svg viewBox="0 0 24 24" width="16" height="16">
            <path d="M3 17 C 8 7, 11 7, 12 12 S 19 21, 21 12" fill="none" stroke="currentColor" stroke-width="1.7" />
          </svg>
        </i>
        Skiplagging
      </a>
      <div class="nav-links">
        <a href="/" class:active={path === '/'}>Search</a>
        <a href="/hidden" class:active={path.startsWith('/hidden')}>Hidden city</a>
        <a href="/anatomy" class:active={path.startsWith('/anatomy')}>How it works</a>
        <a href="/places" class:active={path.startsWith('/places')}>Places</a>
        <a href="/sources" class:active={path.startsWith('/sources')}>Data</a>
      </div>
    </nav>
  </div>
  <main class="main">
    {@render children()}
  </main>
  <div class="wrap">
    <footer class="footer">We find fares. We don’t book tickets. Airlines prohibit hidden-city travel.</footer>
  </div>
</div>
