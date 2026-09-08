from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field, field_validator

Cabin = Literal["ECONOMY", "PREMIUM_ECONOMY", "BUSINESS", "FIRST"]
Channel = Literal["gds", "airline-direct", "ota", "ndc", "meta-search", "specialist-meta"]
ItineraryKind = Literal["nonstop", "connecting", "nearby", "hidden-city"]
DataLayer = Literal[
    "reference",
    "historical-route-map",
    "schedule-status",
    "priced-offer",
    "order-pnr",
    "live-track",
    "live-track-link",
    "meta-search",
    "ota",
    "airline-direct",
    "specialist-meta",
]


class SearchQuery(BaseModel):
    origin: str = Field(..., min_length=3, max_length=8)
    destination: str = Field(..., min_length=3, max_length=8)
    date: str = Field(..., pattern=r"^\d{4}-\d{2}-\d{2}$")
    adults: int = Field(1, ge=1, le=9)
    cabin: Cabin = "ECONOMY"
    currency: str = "USD"
    include_nearby: bool = True
    allow_synthetic: bool = False

    @field_validator("origin", "destination")
    @classmethod
    def _place_id(cls, v: str) -> str:
        from app.metros import normalize_place_id

        return normalize_place_id(v)


class Country(BaseModel):
    iso2: str
    iso3: str | None = None
    name: str
    continent: str
    continent_name: str = ""
    capital: str = ""
    currency_code: str = ""
    currency_name: str = ""
    tld: str = ""
    phone: str = ""
    languages: str = ""
    population: int | None = None
    area_km2: float | None = None
    wikipedia: str | None = None
    airport_count: int = 0
    sources: str = "ourairports"


class Region(BaseModel):
    code: str
    local_code: str = ""
    name: str
    iso_country: str
    country_name: str = ""
    continent: str = ""


class Runway(BaseModel):
    id: int
    length_ft: int | None = None
    width_ft: int | None = None
    surface: str = ""
    lighted: bool = False
    closed: bool = False
    le_ident: str = ""
    he_ident: str = ""


class Navaid(BaseModel):
    ident: str
    name: str
    type: str
    frequency_khz: int | None = None


class Airport(BaseModel):
    iata: str
    place_id: str = ""
    icao: str | None = None
    ident: str | None = None
    name: str
    city: str
    country: str
    country_name: str = ""
    region_code: str = ""
    region_name: str = ""
    continent: str = ""
    continent_name: str = ""
    currency_code: str = ""
    lat: float
    lon: float
    metro: str
    type: str = "large_airport"
    scheduled_service: bool = True
    elevation_ft: int | None = None
    wikipedia: str | None = None
    source: str = "ourairports"
    hub_carriers: list[str] = Field(default_factory=list)
    members: list[str] = Field(default_factory=list)
    runways: list[Runway] = Field(default_factory=list)
    navaids: list[Navaid] = Field(default_factory=list)


class Segment(BaseModel):
    origin: str
    dest: str
    carrier: str
    operating_carrier: str | None = None
    flight_number: str
    dep: str
    arr: str
    duration_min: int
    rbd: str
    fare_basis: str | None = None
    aircraft: str = ""


class Offer(BaseModel):
    id: str
    kind: ItineraryKind
    channel: Channel
    source: str
    layer: DataLayer = "priced-offer"
    segments: list[Segment]
    price: float | None = None
    base_price: float | None = None
    taxes: float | None = None
    currency: str
    cabin: Cabin
    fare_basis: str
    seats: int | None = None
    last_ticketing_date: str | None = None
    validating_airline: str | None = None
    instant_ticketing: bool | None = None
    refundable: bool = False
    bags_included: int = 0
    carrier: str
    duration_min: int
    stops: int
    first_flight: str
    retrieved_at: str | None = None
    note: str | None = None
    live: bool | None = None


class RiskItem(BaseModel):
    id: str
    label: str
    severity: Literal["low", "medium", "high", "very-high"]
    predictability: Literal["low", "medium", "high", "inherent"]
    why: str


class RiskAssessment(BaseModel):
    score: int = Field(..., ge=0, le=100)
    headline: str
    one_way_only: bool
    carry_on_only: bool
    items: list[RiskItem]
    net_saving_estimate: float
    expected_disruption_cost: float
    expected_enforcement_cost: float


class HiddenCityMatch(BaseModel):
    id: str
    hidden_city: str
    hidden_city_name: str
    local_offer: Offer
    through_offer: Offer
    first_flight_match: bool
    gross_saving: float
    saving_pct: float
    currency: str
    risk: RiskAssessment
    bookers: list["BookerLink"] = Field(default_factory=list)
    ticketed_destination: str = ""
    intended_destination: str = ""
    exit_segment_index: int = 0
    warnings: list[str] = Field(default_factory=list)
    result_type: Literal["hidden_city"] = "hidden_city"


class OfferRefreshRequest(BaseModel):
    offer: Offer
    intended_destination: str


class OfferRefreshResponse(BaseModel):
    offer: Offer | None = None
    valid: bool
    reason: str


class SearchDebug(BaseModel):
    providers: list[str] = Field(default_factory=list)
    standard_query: str = ""
    expanded_destinations: list[str] = Field(default_factory=list)
    skipped_expansion: list[str] = Field(default_factory=list)
    reused_from_index: int = 0
    rejected: list[str] = Field(default_factory=list)
    cache: str = "miss"


class HonestPick(BaseModel):
    kind: Literal["nonstop", "connecting"]
    reason: str
    offer: Offer
    layover_min: int = 0


class ChannelGroup(BaseModel):
    kind: ItineraryKind
    label: str
    blurb: str
    offers: list[Offer]


class BookerLink(BaseModel):
    id: str
    name: str
    layer: DataLayer
    role: str
    url: str
    issues_ticket: bool


class TrackerLink(BaseModel):
    id: str
    name: str
    layer: DataLayer
    role: str
    url: str


class TrackedAircraft(BaseModel):
    icao24: str
    callsign: str | None
    origin_country: str | None
    lat: float | None
    lon: float | None
    baro_altitude_m: float | None
    on_ground: bool
    velocity_ms: float | None
    true_track: float | None
    position_source: int | None
    position_source_name: str
    airline_iata: str | None = None
    airline_name: str | None = None


class LiveTraffic(BaseModel):
    airport: str
    source: str
    layer: DataLayer = "live-track"
    api_time: int | None
    note: str
    aircraft: list[TrackedAircraft]
    trackers: list[TrackerLink]


class BoardFlight(BaseModel):
    flight_number: str
    carrier: str | None
    origin: str | None
    dest: str | None
    scheduled: str | None
    estimated: str | None
    status: str | None
    terminal: str | None
    gate: str | None
    source: str
    layer: DataLayer = "schedule-status"


class ConnectionHint(BaseModel):
    dest: str
    dest_name: str
    evidence: str
    source: str
    layer: DataLayer
    note: str


class SourceDef(BaseModel):
    id: str
    name: str
    layer: DataLayer | str
    role: str
    is_not: str
    freshness: str
    url: str
    can_price: bool
    can_book: bool
    can_track: bool


class HiddenDeal(BaseModel):
    id: int
    origin: str
    origin_city: str
    dest: str
    dest_city: str
    hidden_city: str
    hidden_city_name: str
    date: str
    honest_price: float
    through_price: float
    currency: str
    saving: float
    saving_pct: float
    first_flight: str
    source: str
    bookers: list[BookerLink] = Field(default_factory=list)
    local_offer: Offer | None = None
    through_offer: Offer | None = None


class SearchResponse(BaseModel):
    query: SearchQuery
    origin: Airport
    destination: Airport
    elapsed_ms: float
    search_id: int | None = None
    sources_used: list[str]
    data_gaps: list[str]
    cheapest_local: float | None
    cheapest_any: float | None
    honest_pick: HonestPick | None = None
    best_pick: HonestPick | None = None
    hidden_if_cheaper: HiddenCityMatch | None = None
    channels: list[ChannelGroup]
    hidden_city: list[HiddenCityMatch]
    connection_hints: list[ConnectionHint]
    bookers: list[BookerLink]
    airline_names: dict[str, str] = Field(default_factory=dict)
    traffic_origin: LiveTraffic | None = None
    traffic_destination: LiveTraffic | None = None
    board_origin: list[BoardFlight] = Field(default_factory=list)
    notes: list[str]
    search_debug: SearchDebug | None = None
