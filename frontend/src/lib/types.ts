export type Cabin = 'ECONOMY' | 'PREMIUM_ECONOMY' | 'BUSINESS' | 'FIRST';
export type ItineraryKind = 'nonstop' | 'connecting' | 'nearby' | 'hidden-city' | 'self-transfer';

export interface Country {
  iso2: string;
  iso3?: string | null;
  name: string;
  continent: string;
  continent_name: string;
  capital: string;
  currency_code: string;
  currency_name: string;
  tld: string;
  phone: string;
  languages: string;
  population: number | null;
  area_km2: number | null;
  wikipedia?: string | null;
  airport_count: number;
  sources: string;
}

export interface Region {
  code: string;
  local_code: string;
  name: string;
  iso_country: string;
  country_name: string;
  continent: string;
}

export interface Runway {
  id: number;
  length_ft: number | null;
  width_ft: number | null;
  surface: string;
  lighted: boolean;
  closed: boolean;
  le_ident: string;
  he_ident: string;
}

export interface Airport {
  iata: string;
  place_id?: string;
  icao?: string | null;
  ident?: string | null;
  name: string;
  city: string;
  country: string;
  country_name?: string;
  region_code?: string;
  region_name?: string;
  continent?: string;
  continent_name?: string;
  currency_code?: string;
  lat: number;
  lon: number;
  metro: string;
  type?: string;
  scheduled_service?: boolean;
  elevation_ft?: number | null;
  wikipedia?: string | null;
  source?: string;
  hub_carriers: string[];
  members?: string[];
  runways?: Runway[];
}

export interface Segment {
  origin: string;
  dest: string;
  carrier: string;
  operating_carrier?: string | null;
  flight_number: string;
  dep: string;
  arr: string;
  duration_min: number;
  rbd: string;
  fare_basis?: string | null;
  aircraft: string;
}

export interface Offer {
  id: string;
  kind: ItineraryKind;
  channel: string;
  source: string;
  layer: string;
  segments: Segment[];
  price: number | null;
  base_price?: number | null;
  taxes?: number | null;
  currency: string;
  cabin: Cabin;
  fare_basis: string;
  seats: number | null;
  last_ticketing_date?: string | null;
  validating_airline?: string | null;
  note?: string | null;
  bags_included: number;
  carrier: string;
  duration_min: number;
  stops: number;
  first_flight: string;
  retrieved_at?: string | null;
  live?: boolean | null;
  self_transfer_airports?: string[];
  separate_tickets?: { origin: string; dest: string; date: string; price: number; currency: string; carrier: string }[];
}

export interface RiskItem {
  id: string;
  label: string;
  severity: 'low' | 'medium' | 'high' | 'very-high';
  predictability: string;
  why: string;
}

export interface RiskAssessment {
  score: number;
  headline: string;
  one_way_only: boolean;
  carry_on_only: boolean;
  items: RiskItem[];
  net_saving_estimate: number;
  expected_disruption_cost: number;
  expected_enforcement_cost: number;
}

export interface BookerLink {
  id: string;
  name: string;
  layer: string;
  role: string;
  url: string;
  issues_ticket: boolean;
}

export interface TrackerLink {
  id: string;
  name: string;
  layer: string;
  role: string;
  url: string;
}

export interface HiddenCityMatch {
  id: string;
  hidden_city: string;
  hidden_city_name: string;
  local_offer: Offer;
  through_offer: Offer;
  first_flight_match: boolean;
  gross_saving: number;
  saving_pct: number;
  currency: string;
  risk: RiskAssessment;
  bookers: BookerLink[];
  ticketed_destination?: string;
  intended_destination?: string;
  exit_segment_index?: number;
  warnings?: string[];
  result_type?: 'hidden_city';
}

export interface CandidateTrace {
  code: string;
  score: number;
  source: string;
  observations: number;
  successful_connections: number;
  cheaper_than_direct_count: number;
  median_saving: number;
  parts: Record<string, number>;
  selected: boolean;
}

export interface SearchDebug {
  providers: string[];
  standard_query: string;
  mode?: 'fast' | 'deep';
  expanded_destinations: string[];
  pending_candidates?: string[];
  skipped_expansion?: string[];
  candidates?: CandidateTrace[];
  provider_calls?: number;
  provider_cost_usd?: number;
  reused_from_index?: number;
  rejected: string[];
  cache: string;
  pending_self_transfer?: boolean;
}

export interface HonestPick {
  kind: 'nonstop' | 'connecting';
  reason: string;
  offer: Offer;
  layover_min: number;
}

export interface ChannelGroup {
  kind: ItineraryKind;
  label: string;
  blurb: string;
  offers: Offer[];
}

export interface TrackedAircraft {
  icao24: string;
  callsign: string | null;
  origin_country: string | null;
  lat: number | null;
  lon: number | null;
  baro_altitude_m: number | null;
  on_ground: boolean;
  velocity_ms: number | null;
  true_track: number | null;
  position_source_name: string;
}

export interface LiveTraffic {
  airport: string;
  source: string;
  layer: string;
  api_time: number | null;
  note: string;
  aircraft: TrackedAircraft[];
  trackers: TrackerLink[];
}

export interface BoardFlight {
  flight_number: string;
  carrier: string | null;
  origin: string | null;
  dest: string | null;
  scheduled: string | null;
  estimated: string | null;
  status: string | null;
  terminal: string | null;
  gate: string | null;
  source: string;
  layer: string;
}

export interface ConnectionHint {
  dest: string;
  dest_name: string;
  evidence: string;
  source: string;
  layer: string;
  note: string;
}

export interface SourceDef {
  id: string;
  name: string;
  layer: string;
  role: string;
  is_not: string;
  freshness: string;
  url: string;
  can_price: boolean;
  can_book: boolean;
  can_track: boolean;
  configured?: boolean;
}

export interface SearchQuery {
  origin: string;
  destination: string;
  date: string;
  adults: number;
  cabin: Cabin;
  currency: string;
  include_nearby: boolean;
  allow_synthetic: boolean;
}

export interface HiddenDeal {
  id: number;
  origin: string;
  origin_city: string;
  dest: string;
  dest_city: string;
  hidden_city: string;
  hidden_city_name: string;
  date: string;
  honest_price: number;
  through_price: number;
  currency: string;
  saving: number;
  saving_pct: number;
  first_flight: string;
  source: string;
  bookers: BookerLink[];
  local_offer?: Offer | null;
  through_offer?: Offer | null;
}

export interface SearchResponse {
  query: SearchQuery;
  origin: Airport;
  destination: Airport;
  elapsed_ms: number;
  search_id: number | null;
  sources_used: string[];
  data_gaps: string[];
  cheapest_local: number | null;
  cheapest_any: number | null;
  honest_pick: HonestPick | null;
  best_pick?: HonestPick | null;
  hidden_if_cheaper: HiddenCityMatch | null;
  channels: ChannelGroup[];
  hidden_city: HiddenCityMatch[];
  connection_hints: ConnectionHint[];
  bookers: BookerLink[];
  airline_names?: Record<string, string>;
  traffic_origin: LiveTraffic | null;
  traffic_destination: LiveTraffic | null;
  board_origin: BoardFlight[];
  notes: string[];
  search_debug?: SearchDebug | null;
}
