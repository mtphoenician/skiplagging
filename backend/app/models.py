from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field

Cabin = Literal["ECONOMY", "PREMIUM_ECONOMY", "BUSINESS", "FIRST"]
Channel = Literal["gds", "airline-direct", "ota", "ndc", "demo"]
ItineraryKind = Literal["nonstop", "connecting", "nearby", "hidden-city"]


class SearchQuery(BaseModel):
    origin: str = Field(..., min_length=3, max_length=3)
    destination: str = Field(..., min_length=3, max_length=3)
    date: str = Field(..., pattern=r"^\d{4}-\d{2}-\d{2}$")
    adults: int = Field(1, ge=1, le=9)
    cabin: Cabin = "ECONOMY"
    currency: str = "USD"
    include_nearby: bool = True
    live: bool = False


class Airport(BaseModel):
    iata: str
    name: str
    city: str
    country: str
    lat: float
    lon: float
    metro: str
    hub_carriers: list[str] = Field(default_factory=list)


class Segment(BaseModel):
    origin: str
    dest: str
    carrier: str
    flight_number: str
    dep: str
    arr: str
    duration_min: int
    rbd: str
    aircraft: str = "32N"


class Offer(BaseModel):
    id: str
    kind: ItineraryKind
    channel: Channel
    source: str
    segments: list[Segment]
    price: float
    currency: str
    cabin: Cabin
    fare_basis: str
    seats: int = 4
    refundable: bool = False
    bags_included: int = 0
    carrier: str
    duration_min: int
    stops: int
    first_flight: str


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


class ChannelGroup(BaseModel):
    kind: ItineraryKind
    label: str
    blurb: str
    offers: list[Offer]


class SearchResponse(BaseModel):
    query: SearchQuery
    origin: Airport
    destination: Airport
    elapsed_ms: float
    sources_used: list[str]
    cheapest_local: float | None
    cheapest_any: float | None
    channels: list[ChannelGroup]
    hidden_city: list[HiddenCityMatch]
    notes: list[str]
