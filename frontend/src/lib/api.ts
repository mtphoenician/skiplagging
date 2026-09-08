import type { Airport, Country, HiddenDeal, Offer, SearchQuery, SearchResponse, SourceDef } from './types';

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

export async function refreshOffer(
  offer: Offer,
  intended_destination: string
): Promise<{ offer: Offer | null; valid: boolean; reason: string }> {
  const r = await fetch(`${prefix}/offers/refresh`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ offer, intended_destination })
  });
  if (!r.ok) throw new Error('Refresh failed');
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

export function timeRange(dep: string, arr: string, durationMin = 0): string {
  const a = hm(dep);
  const b = hm(arr);
  if (a !== '—' && a === b) {
    return durationMin > 0 ? `${a} · ${duration(durationMin)}` : a;
  }
  return `${a}–${b}`;
}

export function duration(min: number): string {
  const h = Math.floor(min / 60);
  const m = min % 60;
  return h ? (m ? `${h}h ${m}m` : `${h}h`) : `${m}m`;
}

const AIRLINES: Record<string, string> = {
  AA: 'American Airlines',
  AF: 'Air France',
  AZ: 'ITA Airways',
  BA: 'British Airways',
  D8: 'Norwegian',
  DE: 'Condor',
  DL: 'Delta',
  DY: 'Norwegian',
  EK: 'Emirates',
  FR: 'Ryanair',
  GR: 'Aurigny',
  HR: 'Hahn Air',
  IB: 'Iberia',
  KL: 'KLM',
  LH: 'Lufthansa',
  LX: 'Swiss',
  QR: 'Qatar Airways',
  TK: 'Turkish Airlines',
  UA: 'United',
  U2: 'easyJet',
  UX: 'Air Europa',
  VS: 'Virgin Atlantic',
  WN: 'Southwest'
};

export function airlineName(code: string | null | undefined, names: Record<string, string> = {}): string {
  if (!code) return '';
  const id = code.toUpperCase();
  if (id === 'ZZ' || id === 'XX' || id === 'YY') return 'Duffel Airways (test)';
  return names[id] || AIRLINES[id] || id;
}

export function googleFlightsUrl(origin: string, dest: string, date: string, currency = 'USD'): string {
  const o = origin.toUpperCase();
  const d = dest.toUpperCase();
  const ccy = (currency || 'USD').toUpperCase();
  return `https://www.google.com/travel/flights/search?hl=en&curr=${ccy}#flt=${o}.${d}.${date};c:${ccy};e:1;sd:1;t:f`;
}

export function itineraryBookers(
  origin: string,
  dest: string,
  date: string,
  adults = 1,
  currency = 'USD'
): { id: string; name: string; url: string }[] {
  const o = origin.toUpperCase();
  const d = dest.toUpperCase();
  const yymmdd = date.slice(2).replace(/-/g, '');
  const us = `${date.slice(5, 7)}/${date.slice(8, 10)}/${date.slice(0, 4)}`;
  return [
    { id: 'google-flights', name: 'Google Flights', url: googleFlightsUrl(o, d, date, currency) },
    { id: 'kayak', name: 'Kayak', url: `https://www.kayak.com/flights/${o}-${d}/${date}?sort=price_a` },
    {
      id: 'skyscanner',
      name: 'Skyscanner',
      url: `https://www.skyscanner.com/transport/flights/${o.toLowerCase()}/${d.toLowerCase()}/${yymmdd}/?adultsv2=${adults}&cabinclass=economy&rtn=0`
    },
    {
      id: 'booking-com',
      name: 'Booking.com',
      url: `https://flights.booking.com/flights/${o}.AIRPORT-${d}.AIRPORT/?type=ONEWAY&adults=${adults}&cabinClass=ECONOMY&depart=${date}&sort=BEST`
    },
    {
      id: 'expedia',
      name: 'Expedia',
      url: `https://www.expedia.com/Flights-Search?trip=oneway&leg1=from:${o},to:${d},departure:${us}TANYT&passengers=adults:${adults},children:0,infantinlap:N&mode=search`
    }
  ];
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
