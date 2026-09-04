import type { Airport, SearchQuery, SearchResponse } from './types';

const prefix = '/api';

export async function searchAirports(q: string): Promise<Airport[]> {
  const r = await fetch(`${prefix}/airports?q=${encodeURIComponent(q)}`);
  if (!r.ok) throw new Error('Airport lookup failed');
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

export function money(n: number, currency = 'USD'): string {
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

export function defaultDate(): string {
  const d = new Date();
  d.setDate(d.getDate() + 11);
  return d.toISOString().slice(0, 10);
}
