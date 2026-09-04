export type Cabin = 'ECONOMY' | 'PREMIUM_ECONOMY' | 'BUSINESS' | 'FIRST';
export type ItineraryKind = 'nonstop' | 'connecting' | 'nearby' | 'hidden-city';

export interface Airport {
  iata: string;
  name: string;
  city: string;
  country: string;
  lat: number;
  lon: number;
  metro: string;
  hub_carriers: string[];
}

export interface Segment {
  origin: string;
  dest: string;
  carrier: string;
  flight_number: string;
  dep: string;
  arr: string;
  duration_min: number;
  rbd: string;
  aircraft: string;
}

export interface Offer {
  id: string;
  kind: ItineraryKind;
  channel: string;
  source: string;
  segments: Segment[];
  price: number;
  currency: string;
  cabin: Cabin;
  fare_basis: string;
  seats: number;
  refundable: boolean;
  bags_included: number;
  carrier: string;
  duration_min: number;
  stops: number;
  first_flight: string;
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
}

export interface ChannelGroup {
  kind: ItineraryKind;
  label: string;
  blurb: string;
  offers: Offer[];
}

export interface SearchQuery {
  origin: string;
  destination: string;
  date: string;
  adults: number;
  cabin: Cabin;
  currency: string;
  include_nearby: boolean;
  live: boolean;
}

export interface SearchResponse {
  query: SearchQuery;
  origin: Airport;
  destination: Airport;
  elapsed_ms: number;
  sources_used: string[];
  cheapest_local: number | null;
  cheapest_any: number | null;
  channels: ChannelGroup[];
  hidden_city: HiddenCityMatch[];
  notes: string[];
}
