use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::metros::normalize_place_id;

fn default_usd() -> String {
    "USD".into()
}
fn default_economy() -> String {
    "ECONOMY".into()
}
fn default_adults() -> i32 {
    1
}
fn default_priced_layer() -> String {
    "priced-offer".into()
}
fn default_true() -> bool {
    true
}
fn default_live_track() -> String {
    "live-track".into()
}
fn default_schedule() -> String {
    "schedule-status".into()
}
fn default_hidden_city() -> String {
    "hidden_city".into()
}
fn default_airport_type() -> String {
    "large_airport".into()
}

fn empty_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

fn force_usd<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let _ = String::deserialize(deserializer);
    Ok("USD".into())
}

fn place_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(normalize_place_id(&s))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(deserialize_with = "place_id")]
    pub origin: String,
    #[serde(deserialize_with = "place_id")]
    pub destination: String,
    pub date: String,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub return_date: Option<String>,
    #[serde(default = "default_adults")]
    pub adults: i32,
    #[serde(default = "default_economy")]
    pub cabin: String,
    #[serde(default = "default_usd", deserialize_with = "force_usd")]
    pub currency: String,
    #[serde(default = "default_true")]
    pub include_nearby: bool,
    #[serde(default)]
    pub allow_synthetic: bool,
    /// Request-only: skip the in-memory POST /search cache. Not part of the cache key.
    #[serde(default, skip_serializing)]
    pub refresh: bool,
}

impl SearchQuery {
    pub fn validate(&mut self) -> Result<(), SearchQueryError> {
        self.origin = normalize_place_id(&self.origin);
        self.destination = normalize_place_id(&self.destination);
        self.currency = "USD".into();
        if self.origin.len() < 3 || self.origin.len() > 8 {
            return Err(SearchQueryError::BadField("origin"));
        }
        if self.destination.len() < 3 || self.destination.len() > 8 {
            return Err(SearchQueryError::BadField("destination"));
        }
        if !is_iso_date(&self.date) {
            return Err(SearchQueryError::BadField("date"));
        }
        if let Some(r) = &self.return_date {
            if !is_iso_date(r) {
                return Err(SearchQueryError::BadField("return_date"));
            }
            if r < &self.date {
                return Err(SearchQueryError::ReturnBeforeOutbound);
            }
        }
        if !(1..=9).contains(&self.adults) {
            return Err(SearchQueryError::BadField("adults"));
        }
        self.cabin = self.cabin.trim().to_uppercase();
        if !matches!(
            self.cabin.as_str(),
            "ECONOMY" | "PREMIUM_ECONOMY" | "BUSINESS" | "FIRST"
        ) {
            return Err(SearchQueryError::BadField("cabin"));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum SearchQueryError {
    ReturnBeforeOutbound,
    BadField(&'static str),
}

fn is_iso_date(s: &str) -> bool {
    s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopRequest {
    pub origin: String,
    pub dest: String,
    pub date: String,
    #[serde(default = "default_adults")]
    pub adults: i32,
    #[serde(default = "default_economy")]
    pub cabin: String,
    #[serde(default = "default_usd")]
    pub currency: String,
    #[serde(default)]
    pub nonstop: bool,
    #[serde(default = "default_max_offers")]
    pub max_offers: i32,
    #[serde(default)]
    pub return_date: Option<String>,
}

impl Default for ShopRequest {
    fn default() -> Self {
        Self {
            origin: String::new(),
            dest: String::new(),
            date: String::new(),
            adults: 1,
            cabin: "ECONOMY".into(),
            currency: "USD".into(),
            nonstop: false,
            max_offers: 20,
            return_date: None,
        }
    }
}

fn default_max_offers() -> i32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Country {
    pub iso2: String,
    pub iso3: Option<String>,
    pub name: String,
    pub continent: String,
    #[serde(default)]
    pub continent_name: String,
    #[serde(default)]
    pub capital: String,
    #[serde(default)]
    pub currency_code: String,
    #[serde(default)]
    pub currency_name: String,
    #[serde(default)]
    pub tld: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub languages: String,
    pub population: Option<i32>,
    pub area_km2: Option<f64>,
    pub wikipedia: Option<String>,
    #[serde(default)]
    pub airport_count: i32,
    #[serde(default = "default_ourairports")]
    pub sources: String,
}

fn default_ourairports() -> String {
    "ourairports".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Region {
    pub code: String,
    #[serde(default)]
    pub local_code: String,
    pub name: String,
    pub iso_country: String,
    #[serde(default)]
    pub country_name: String,
    #[serde(default)]
    pub continent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Runway {
    pub id: i32,
    pub length_ft: Option<i32>,
    pub width_ft: Option<i32>,
    #[serde(default)]
    pub surface: String,
    #[serde(default)]
    pub lighted: bool,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub le_ident: String,
    #[serde(default)]
    pub he_ident: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Navaid {
    pub ident: String,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub frequency_khz: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Airport {
    pub iata: String,
    #[serde(default)]
    pub place_id: String,
    pub icao: Option<String>,
    pub ident: Option<String>,
    pub name: String,
    pub city: String,
    pub country: String,
    #[serde(default)]
    pub country_name: String,
    #[serde(default)]
    pub region_code: String,
    #[serde(default)]
    pub region_name: String,
    #[serde(default)]
    pub continent: String,
    #[serde(default)]
    pub continent_name: String,
    #[serde(default)]
    pub currency_code: String,
    pub lat: f64,
    pub lon: f64,
    pub metro: String,
    #[serde(rename = "type", default = "default_airport_type")]
    pub type_: String,
    #[serde(default = "default_true")]
    pub scheduled_service: bool,
    pub elevation_ft: Option<i32>,
    pub wikipedia: Option<String>,
    #[serde(default = "default_ourairports")]
    pub source: String,
    #[serde(default)]
    pub hub_carriers: Vec<String>,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub runways: Vec<Runway>,
    #[serde(default)]
    pub navaids: Vec<Navaid>,
}

impl Default for Airport {
    fn default() -> Self {
        Self {
            iata: String::new(),
            place_id: String::new(),
            icao: None,
            ident: None,
            name: String::new(),
            city: String::new(),
            country: String::new(),
            country_name: String::new(),
            region_code: String::new(),
            region_name: String::new(),
            continent: String::new(),
            continent_name: String::new(),
            currency_code: String::new(),
            lat: 0.0,
            lon: 0.0,
            metro: String::new(),
            type_: "large_airport".into(),
            scheduled_service: true,
            elevation_ft: None,
            wikipedia: None,
            source: "ourairports".into(),
            hub_carriers: vec![],
            members: vec![],
            runways: vec![],
            navaids: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Segment {
    pub origin: String,
    pub dest: String,
    pub carrier: String,
    pub operating_carrier: Option<String>,
    pub flight_number: String,
    pub dep: String,
    pub arr: String,
    pub duration_min: i32,
    #[serde(default)]
    pub rbd: String,
    pub fare_basis: Option<String>,
    #[serde(default)]
    pub aircraft: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SeparateTicket {
    pub origin: String,
    pub dest: String,
    pub date: String,
    pub price: f64,
    pub currency: String,
    pub carrier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Offer {
    pub id: String,
    pub kind: String,
    pub channel: String,
    pub source: String,
    #[serde(default = "default_priced_layer")]
    pub layer: String,
    pub segments: Vec<Segment>,
    pub price: Option<f64>,
    pub base_price: Option<f64>,
    pub taxes: Option<f64>,
    pub currency: String,
    /// Duffel's own USD total when the offer JSON includes one. Ranking uses
    /// this without waiting for Frankfurter and without FALLBACK rates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quoted_usd: Option<f64>,
    pub cabin: String,
    #[serde(default = "default_adults")]
    pub adults: i32,
    #[serde(default)]
    pub fare_basis: String,
    pub seats: Option<i32>,
    pub last_ticketing_date: Option<String>,
    pub validating_airline: Option<String>,
    pub instant_ticketing: Option<bool>,
    #[serde(default)]
    pub refundable: bool,
    #[serde(default)]
    pub bags_included: i32,
    pub carrier: String,
    pub duration_min: i32,
    pub stops: i32,
    pub first_flight: String,
    pub retrieved_at: Option<String>,
    pub expires_at: Option<String>,
    pub note: Option<String>,
    pub live: Option<bool>,
    pub return_date: Option<String>,
    pub outbound_end: Option<i32>,
    #[serde(default)]
    pub self_transfer_airports: Vec<String>,
    #[serde(default)]
    pub separate_tickets: Vec<SeparateTicket>,
}

impl Default for Offer {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: "nonstop".into(),
            channel: "ndc".into(),
            source: String::new(),
            layer: "priced-offer".into(),
            segments: vec![],
            price: None,
            base_price: None,
            taxes: None,
            currency: "USD".into(),
            quoted_usd: None,
            cabin: "ECONOMY".into(),
            adults: 1,
            fare_basis: String::new(),
            seats: None,
            last_ticketing_date: None,
            validating_airline: None,
            instant_ticketing: None,
            refundable: false,
            bags_included: 0,
            carrier: String::new(),
            duration_min: 0,
            stops: 0,
            first_flight: String::new(),
            retrieved_at: None,
            expires_at: None,
            note: None,
            live: None,
            return_date: None,
            outbound_end: None,
            self_transfer_airports: vec![],
            separate_tickets: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskItem {
    pub id: String,
    pub label: String,
    pub severity: String,
    pub predictability: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub score: i32,
    pub headline: String,
    pub one_way_only: bool,
    pub carry_on_only: bool,
    pub items: Vec<RiskItem>,
    pub net_saving_estimate: f64,
    pub expected_disruption_cost: f64,
    pub expected_enforcement_cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenCityMatch {
    pub id: String,
    pub hidden_city: String,
    pub hidden_city_name: String,
    pub local_offer: Offer,
    pub through_offer: Offer,
    pub first_flight_match: bool,
    pub gross_saving: f64,
    pub saving_pct: f64,
    pub currency: String,
    pub risk: RiskAssessment,
    #[serde(default)]
    pub bookers: Vec<BookerLink>,
    #[serde(default)]
    pub ticketed_destination: String,
    #[serde(default)]
    pub intended_destination: String,
    #[serde(default)]
    pub exit_segment_index: i32,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default = "default_hidden_city")]
    pub result_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferRefreshRequest {
    pub offer: Offer,
    pub intended_destination: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferRefreshResponse {
    pub offer: Option<Offer>,
    pub valid: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CandidateTrace {
    pub code: String,
    pub score: f64,
    pub source: String,
    #[serde(default)]
    pub observations: i32,
    #[serde(default)]
    pub successful_connections: i32,
    #[serde(default)]
    pub cheaper_than_direct_count: i32,
    #[serde(default)]
    pub median_saving: f64,
    #[serde(default)]
    pub parts: HashMap<String, f64>,
    #[serde(default)]
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchDebug {
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub standard_query: String,
    #[serde(default = "default_fast")]
    pub mode: String,
    #[serde(default)]
    pub expanded_destinations: Vec<String>,
    #[serde(default)]
    pub pending_candidates: Vec<String>,
    #[serde(default)]
    pub skipped_expansion: Vec<String>,
    #[serde(default)]
    pub candidates: Vec<CandidateTrace>,
    #[serde(default)]
    pub provider_calls: i32,
    #[serde(default)]
    pub provider_cost_usd: f64,
    #[serde(default)]
    pub reused_from_index: i32,
    #[serde(default)]
    pub reused_honest: bool,
    #[serde(default)]
    pub fx_live: bool,
    #[serde(default)]
    pub rejected: Vec<String>,
    #[serde(default = "default_miss")]
    pub cache: String,
    #[serde(default)]
    pub pending_self_transfer: bool,
}

fn default_fast() -> String {
    "fast".into()
}
fn default_miss() -> String {
    "miss".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchExpandRequest {
    pub query: SearchQuery,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HonestPick {
    pub kind: String,
    pub reason: String,
    pub offer: Offer,
    #[serde(default)]
    pub layover_min: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelGroup {
    pub kind: String,
    pub label: String,
    pub blurb: String,
    pub offers: Vec<Offer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookerLink {
    pub id: String,
    pub name: String,
    pub layer: String,
    pub role: String,
    pub url: String,
    pub issues_ticket: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerLink {
    pub id: String,
    pub name: String,
    pub layer: String,
    pub role: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedAircraft {
    pub icao24: String,
    pub callsign: Option<String>,
    pub origin_country: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub baro_altitude_m: Option<f64>,
    pub on_ground: bool,
    pub velocity_ms: Option<f64>,
    pub true_track: Option<f64>,
    pub position_source: Option<i32>,
    pub position_source_name: String,
    pub airline_iata: Option<String>,
    pub airline_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTraffic {
    pub airport: String,
    pub source: String,
    #[serde(default = "default_live_track")]
    pub layer: String,
    pub api_time: Option<i64>,
    pub note: String,
    pub aircraft: Vec<TrackedAircraft>,
    pub trackers: Vec<TrackerLink>,
}

impl Default for LiveTraffic {
    fn default() -> Self {
        Self {
            airport: String::new(),
            source: "opensky".into(),
            layer: "live-track".into(),
            api_time: None,
            note: String::new(),
            aircraft: vec![],
            trackers: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardFlight {
    pub flight_number: String,
    pub carrier: Option<String>,
    pub origin: Option<String>,
    pub dest: Option<String>,
    pub scheduled: Option<String>,
    pub estimated: Option<String>,
    pub status: Option<String>,
    pub terminal: Option<String>,
    pub gate: Option<String>,
    pub source: String,
    #[serde(default = "default_schedule")]
    pub layer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionHint {
    pub dest: String,
    pub dest_name: String,
    pub evidence: String,
    pub source: String,
    pub layer: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDef {
    pub id: String,
    pub name: String,
    pub layer: String,
    pub role: String,
    pub is_not: String,
    pub freshness: String,
    pub url: String,
    pub can_price: bool,
    pub can_book: bool,
    pub can_track: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenDeal {
    pub id: i32,
    pub origin: String,
    pub origin_city: String,
    pub dest: String,
    pub dest_city: String,
    pub hidden_city: String,
    pub hidden_city_name: String,
    pub date: String,
    pub honest_price: f64,
    pub through_price: f64,
    pub currency: String,
    pub saving: f64,
    pub saving_pct: f64,
    pub first_flight: String,
    pub source: String,
    #[serde(default)]
    pub bookers: Vec<BookerLink>,
    pub local_offer: Option<Offer>,
    pub through_offer: Option<Offer>,
    pub risk: Option<RiskAssessment>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: SearchQuery,
    pub origin: Airport,
    pub destination: Airport,
    pub elapsed_ms: f64,
    pub search_id: Option<i32>,
    pub sources_used: Vec<String>,
    pub data_gaps: Vec<String>,
    pub cheapest_local: Option<f64>,
    pub cheapest_any: Option<f64>,
    pub honest_pick: Option<HonestPick>,
    pub best_pick: Option<HonestPick>,
    pub hidden_if_cheaper: Option<HiddenCityMatch>,
    pub channels: Vec<ChannelGroup>,
    pub hidden_city: Vec<HiddenCityMatch>,
    pub connection_hints: Vec<ConnectionHint>,
    pub bookers: Vec<BookerLink>,
    #[serde(default)]
    pub airline_names: HashMap<String, String>,
    pub traffic_origin: Option<LiveTraffic>,
    pub traffic_destination: Option<LiveTraffic>,
    #[serde(default)]
    pub board_origin: Vec<BoardFlight>,
    pub notes: Vec<String>,
    pub search_debug: Option<SearchDebug>,
}

/// JSON value used when a handler returns an untyped object.
pub type JsonMap = Value;
