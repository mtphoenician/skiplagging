import type { Airport, Country, HiddenDeal, SearchQuery, SearchResponse, SourceDef } from './types';

const prefix = '/api';

export async function fetchAirport(iata: string): Promise<Airport | null> {
  const r = await fetch(`${prefix}/airports/${encodeURIComponent(iata)}`);
  if (r.status === 404) return null;
  if (!r.ok) throw new Error('Airport lookup failed');
  return r.json();
}

export async function searchAirports(q: string): Promise<Airport[]> {
  const r = await fetch(`${prefix}/airports?q=${encodeURIComponent(q)}`);
  if (!r.ok) {
    const body = await r.json().catch(() => ({ detail: r.statusText }));
    throw new Error(body.detail || 'Airport lookup failed');
  }
  return r.json();
}

export async function searchFares(query: SearchQuery): Promise<SearchResponse> {
  const r = await fetch(`${prefix}/search`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(query)
  });
  if (!r.ok) {
    const body = await r.json().catch(() => ({ detail: r.statusText }));
    throw new Error(body.detail || 'Search failed');
  }
  return r.json();
}

export async function fetchDeals(opts: { limit?: number; origin?: string; dest?: string } = {}): Promise<HiddenDeal[]> {
  const q = new URLSearchParams();
  if (opts.limit) q.set('limit', String(opts.limit));
  if (opts.origin) q.set('origin', opts.origin);
  if (opts.dest) q.set('dest', opts.dest);
  const r = await fetch(`${prefix}/deals?${q.toString()}`);
  if (!r.ok) throw new Error('Deals failed');
  return r.json();
}

export async function fetchDefaults(): Promise<{ origin: Airport; destination: Airport }> {
  const r = await fetch(`${prefix}/defaults`);
  if (!r.ok) throw new Error('No default city pair in the route table');
  return r.json();
}

export async function fetchCountries(q = ''): Promise<Country[]> {
  const r = await fetch(`${prefix}/countries?q=${encodeURIComponent(q)}`);
  if (!r.ok) {
    const body = await r.json().catch(() => ({ detail: r.statusText }));
    const detail = body.detail;
    throw new Error(typeof detail === 'string' ? detail : 'Country lookup failed');
  }
  return r.json();
}

export async function fetchSources(): Promise<{ layers: string[]; sources: SourceDef[] }> {
  const r = await fetch(`${prefix}/sources`);
  if (!r.ok) throw new Error('Sources failed');
  return r.json();
}

export function money(n: number | null | undefined, currency = 'USD'): string {
  if (n == null) return '—';
  return new Intl.NumberFormat('en-US', { style: 'currency', currency, maximumFractionDigits: 0 }).format(n);
}

export function hm(iso: string): string {
  if (!iso) return '—';
  const t = iso.includes('T') ? iso.split('T')[1] : iso;
  return t.slice(0, 5);
}

export function duration(min: number): string {
  const h = Math.floor(min / 60);
  const m = min % 60;
  return h ? `${h}h ${m}m` : `${m}m`;
}

export function sameItinerary(
  a: { segments: { origin: string; dest: string; flight_number: string }[] },
  b: { segments: { origin: string; dest: string; flight_number: string }[] }
): boolean {
  if (a.segments.length !== b.segments.length) return false;
  return a.segments.every(
    (s, i) =>
      s.flight_number === b.segments[i].flight_number &&
      s.origin === b.segments[i].origin &&
      s.dest === b.segments[i].dest
  );
}

export function layoverMinutes(offer: { segments: { arr: string; dep: string }[] }): number {
  let total = 0;
  for (let i = 0; i < offer.segments.length - 1; i++) {
    const arr = Date.parse(offer.segments[i].arr);
    const dep = Date.parse(offer.segments[i + 1].dep);
    if (Number.isNaN(arr) || Number.isNaN(dep) || dep <= arr) continue;
    total += Math.round((dep - arr) / 60000);
  }
  return total;
}

export function defaultDate(): string {
  const d = new Date();
  d.setDate(d.getDate() + 11);
  return d.toISOString().slice(0, 10);
}

export function kt(ms: number | null): string {
  if (ms == null) return '—';
  return `${Math.round(ms * 1.94384)} kt`;
}

export function fl(meters: number | null): string {
  if (meters == null) return '—';
  return `FL${String(Math.round((meters * 3.28084) / 100)).padStart(3, '0')}`;
}
