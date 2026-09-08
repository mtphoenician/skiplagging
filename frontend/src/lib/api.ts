import type { Airport, ChannelGroup, Country, HiddenDeal, Offer, SearchQuery, SearchResponse, SourceDef } from './types';

const prefix = '/api';

function apiDetail(body: unknown, fallback: string): string {
  if (!body || typeof body !== 'object') return fallback;
  const detail = (body as { detail?: unknown }).detail;
  if (typeof detail === 'string' && detail) return detail;
  if (Array.isArray(detail) && detail.length) {
    const first = detail[0];
    if (typeof first === 'string' && first) return first;
    if (first && typeof first === 'object' && 'msg' in first) {
      const msg = (first as { msg: unknown }).msg;
      if (typeof msg === 'string' && msg) return msg;
    }
  }
  return fallback;
}

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
    throw new Error(apiDetail(body, 'Airport lookup failed'));
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
    throw new Error(apiDetail(body, 'Search failed'));
  }
  return r.json();
}

/** Deep pass: shop the ranked candidates the fast pass left pending. */
export async function expandSearch(query: SearchQuery, exclude: string[]): Promise<SearchResponse> {
  const r = await fetch(`${prefix}/search/expand`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, exclude })
  });
  if (!r.ok) {
    const body = await r.json().catch(() => ({ detail: r.statusText }));
    throw new Error(apiDetail(body, 'Deep search failed'));
  }
  return r.json();
}

/** Merge a deep response over a fast one. Hidden-city picks come from the deep
 *  pass only: if a cheaper honest fare appeared, a stale fast match must not win. */
function offerKey(offer: Offer): string {
  return offer.segments
    .map((s) => `${s.origin}|${s.dest}|${s.flight_number}|${(s.dep || '').slice(0, 16)}`)
    .join('>');
}

function mergeSelfTransfer(fast?: ChannelGroup, deep?: ChannelGroup): ChannelGroup | undefined {
  const offers = [...(fast?.offers ?? []), ...(deep?.offers ?? [])].sort(
    (a, b) => (a.price ?? 1e12) - (b.price ?? 1e12)
  );
  const seen = new Set<string>();
  const uniq: Offer[] = [];
  for (const offer of offers) {
    const key = offerKey(offer);
    if (seen.has(key)) continue;
    seen.add(key);
    uniq.push(offer);
    if (uniq.length >= 8) break;
  }
  const base = deep?.offers.length ? deep : fast;
  if (!base) return undefined;
  return { ...base, offers: uniq };
}

export function mergeSearch(fast: SearchResponse, deep: SearchResponse): SearchResponse {
  const fastMap = new Map(fast.channels.map((c) => [c.kind, c]));
  const deepMap = new Map(deep.channels.map((c) => [c.kind, c]));
  const kinds = [...new Set([...deep.channels.map((c) => c.kind), ...fast.channels.map((c) => c.kind)])];
  const channels = kinds
    .map((kind) => {
      const d = deepMap.get(kind);
      const f = fastMap.get(kind);
      if (kind === 'hidden-city') return d ?? f;
      if (kind === 'self-transfer') return mergeSelfTransfer(f, d);
      if (d && d.offers.length) return d;
      return f ?? d;
    })
    .filter((c): c is ChannelGroup => Boolean(c));
  return {
    ...deep,
    honest_pick: deep.honest_pick ?? fast.honest_pick,
    best_pick: deep.best_pick ?? fast.best_pick,
    hidden_if_cheaper: deep.hidden_if_cheaper,
    hidden_city: deep.hidden_city,
    channels,
    bookers: deep.bookers?.length ? deep.bookers : fast.bookers,
    airline_names: { ...(fast.airline_names ?? {}), ...(deep.airline_names ?? {}) },
    traffic_origin: deep.traffic_origin?.aircraft?.length ? deep.traffic_origin : fast.traffic_origin,
    traffic_destination: deep.traffic_destination?.aircraft?.length
      ? deep.traffic_destination
      : fast.traffic_destination
  };
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
    throw new Error(apiDetail(body, 'Country lookup failed'));
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
  VA: 'Virgin Australia',
  AK: 'AirAsia',
  G9: 'Air Arabia',
  QF: 'Qantas',
  WN: 'Southwest'
};

export function airlineName(code: string | null | undefined, names: Record<string, string> = {}): string {
  if (!code) return '';
  const id = code.toUpperCase();
  if (id === 'ZZ' || id === 'XX' || id === 'YY') return 'Duffel Airways (test)';
  return names[id] || AIRLINES[id] || id;
}

function varint(n: number | bigint): number[] {
  let x = BigInt(n);
  if (x < 0n) x += 1n << 64n;
  const out: number[] = [];
  while (true) {
    const bit = Number(x & 0x7fn);
    x >>= 7n;
    if (x) out.push(bit | 0x80);
    else {
      out.push(bit);
      break;
    }
  }
  return out;
}

function key(field: number, wire: number): number[] {
  return varint((field << 3) | wire);
}

function ld(field: number, payload: number[]): number[] {
  return [...key(field, 2), ...varint(payload.length), ...payload];
}

function vint(field: number, n: number | bigint): number[] {
  return [...key(field, 0), ...varint(n)];
}

function pstr(field: number, value: string): number[] {
  return ld(field, [...new TextEncoder().encode(value)]);
}

function place(code: string): number[] {
  return [...vint(1, 1), ...pstr(2, code.toUpperCase())];
}

const GOOGLE_CABIN: Record<string, number> = {
  ECONOMY: 1,
  PREMIUM_ECONOMY: 2,
  BUSINESS: 3,
  FIRST: 4
};

export function googleTfs(
  origin: string,
  dest: string,
  date: string,
  adults = 1,
  cabin = 'ECONOMY'
): string {
  const leg = [...pstr(2, date), ...ld(13, place(origin)), ...ld(14, place(dest))];
  const body: number[] = [...vint(1, 28), ...vint(2, 2), ...ld(3, leg)];
  for (let i = 0; i < Math.max(1, Math.min(adults, 9)); i++) body.push(...vint(8, 1));
  body.push(...vint(9, GOOGLE_CABIN[cabin] ?? 1), ...vint(14, 1), ...ld(16, vint(1, -1)), ...vint(19, 2));
  const b64 = btoa(String.fromCharCode(...body));
  return b64.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}

export function googleFlightsUrl(
  origin: string,
  dest: string,
  date: string,
  currency = 'USD',
  adults = 1,
  cabin = 'ECONOMY'
): string {
  const tfs = googleTfs(origin, dest, date, adults, cabin);
  const ccy = (currency || 'USD').toUpperCase();
  return `https://www.google.com/travel/flights/search?tfs=${tfs}&hl=en&curr=${ccy}`;
}

export function itineraryBookers(
  origin: string,
  dest: string,
  date: string,
  adults = 1,
  currency = 'USD',
  cabin = 'ECONOMY'
): { id: string; name: string; url: string }[] {
  const o = origin.toUpperCase();
  const d = dest.toUpperCase();
  const yymmdd = date.slice(2).replace(/-/g, '');
  const us = `${date.slice(5, 7)}/${date.slice(8, 10)}/${date.slice(0, 4)}`;
  const adultsPath = adults > 1 ? `/${adults}adults` : '';
  const skyCabin =
    cabin === 'PREMIUM_ECONOMY'
      ? 'premiumeconomy'
      : cabin === 'BUSINESS'
        ? 'business'
        : cabin === 'FIRST'
          ? 'first'
          : 'economy';
  return [
    { id: 'google-flights', name: 'Google Flights', url: googleFlightsUrl(o, d, date, currency, adults, cabin) },
    {
      id: 'kayak',
      name: 'Kayak',
      url: `https://www.kayak.com/flights/${o}-${d}/${date}${adultsPath}?sort=bestflight_a`
    },
    {
      id: 'skyscanner',
      name: 'Skyscanner',
      url: `https://www.skyscanner.com/transport/flights/${o.toLowerCase()}/${d.toLowerCase()}/${yymmdd}/?adultsv2=${adults}&cabinclass=${skyCabin}&rtn=0&preferdirects=false`
    },
    {
      id: 'booking-com',
      name: 'Booking.com',
      url: `https://flights.booking.com/flights/${o}.AIRPORT-${d}.AIRPORT/?type=ONEWAY&adults=${adults}&cabinClass=${cabin}&depart=${date}&from=${o}&to=${d}&sort=BEST`
    },
    {
      id: 'expedia',
      name: 'Expedia',
      url: `https://www.expedia.com/Flights-Search?flight-type=on&mode=search&trip=oneway&leg1=from:${o},to:${d},departure:${us}TANYT&passengers=adults:${adults},children:0,infantinlap:N`
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

export function plusDays(dep: string, arr: string): string {
  const a = dep.slice(0, 10);
  const b = arr.slice(0, 10);
  if (!a || !b || a === b) return '';
  const n = Math.round((Date.parse(b) - Date.parse(a)) / 86400000);
  return n > 0 ? `+${n}` : '';
}

export function stopovers(
  offer: Offer
): { code: string; minutes: number; selfTransfer: boolean }[] {
  const marked = new Set((offer.self_transfer_airports ?? []).map((c) => c.toUpperCase()));
  const out: { code: string; minutes: number; selfTransfer: boolean }[] = [];
  for (let i = 0; i < offer.segments.length - 1; i++) {
    const arr = Date.parse(offer.segments[i].arr);
    const dep = Date.parse(offer.segments[i + 1].dep);
    const minutes =
      Number.isNaN(arr) || Number.isNaN(dep) || dep <= arr ? 0 : Math.round((dep - arr) / 60000);
    out.push({
      code: offer.segments[i].dest,
      minutes,
      selfTransfer: marked.has(offer.segments[i].dest.toUpperCase())
    });
  }
  return out;
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
