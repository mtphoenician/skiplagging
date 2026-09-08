from __future__ import annotations

from datetime import datetime

from sqlalchemy import (
    Boolean,
    DateTime,
    Float,
    ForeignKey,
    Index,
    Integer,
    String,
    Text,
    UniqueConstraint,
    func,
)
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


class Base(DeclarativeBase):
    pass


class ContinentRow(Base):
    __tablename__ = "continents"

    code: Mapped[str] = mapped_column(String(2), primary_key=True)
    name: Mapped[str] = mapped_column(Text)
    source: Mapped[str] = mapped_column(String(32), default="ourairports")


class CountryRow(Base):
    __tablename__ = "countries"

    iso2: Mapped[str] = mapped_column(String(2), primary_key=True)
    iso3: Mapped[str | None] = mapped_column(String(3), index=True)
    iso_numeric: Mapped[str | None] = mapped_column(String(8))
    name: Mapped[str] = mapped_column(Text, index=True)
    continent: Mapped[str] = mapped_column(String(2), default="", index=True)
    capital: Mapped[str] = mapped_column(Text, default="")
    currency_code: Mapped[str] = mapped_column(String(8), default="")
    currency_name: Mapped[str] = mapped_column(Text, default="")
    tld: Mapped[str] = mapped_column(String(16), default="")
    phone: Mapped[str] = mapped_column(String(32), default="")
    languages: Mapped[str] = mapped_column(Text, default="")
    population: Mapped[int | None] = mapped_column(Integer)
    area_km2: Mapped[float | None] = mapped_column(Float)
    geoname_id: Mapped[int | None] = mapped_column(Integer)
    wikipedia: Mapped[str | None] = mapped_column(Text)
    sources: Mapped[str] = mapped_column(Text, default="ourairports")
    ingested_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class RegionRow(Base):
    __tablename__ = "regions"

    code: Mapped[str] = mapped_column(String(16), primary_key=True)
    local_code: Mapped[str] = mapped_column(String(16), default="")
    name: Mapped[str] = mapped_column(Text, index=True)
    iso_country: Mapped[str] = mapped_column(String(2), index=True)
    continent: Mapped[str] = mapped_column(String(2), default="")
    wikipedia: Mapped[str | None] = mapped_column(Text)
    source: Mapped[str] = mapped_column(String(32), default="ourairports")


class AirportRow(Base):
    __tablename__ = "airports"

    iata: Mapped[str] = mapped_column(String(3), primary_key=True)
    icao: Mapped[str | None] = mapped_column(String(8), index=True)
    ident: Mapped[str | None] = mapped_column(String(16), index=True)
    name: Mapped[str] = mapped_column(Text)
    municipality: Mapped[str] = mapped_column(Text, default="", index=True)
    iso_country: Mapped[str] = mapped_column(String(8), default="", index=True)
    iso_region: Mapped[str] = mapped_column(String(16), default="", index=True)
    continent: Mapped[str] = mapped_column(String(8), default="")
    country_name: Mapped[str] = mapped_column(Text, default="", index=True)
    region_name: Mapped[str] = mapped_column(Text, default="")
    lat: Mapped[float] = mapped_column(Float)
    lon: Mapped[float] = mapped_column(Float)
    elevation_ft: Mapped[int | None] = mapped_column(Integer)
    type: Mapped[str] = mapped_column(String(32), default="large_airport")
    scheduled_service: Mapped[bool] = mapped_column(Boolean, default=True)
    wikipedia: Mapped[str | None] = mapped_column(Text)
    home_link: Mapped[str | None] = mapped_column(Text)
    gps_code: Mapped[str | None] = mapped_column(String(16))
    local_code: Mapped[str | None] = mapped_column(String(16))
    keywords: Mapped[str | None] = mapped_column(Text)
    source: Mapped[str] = mapped_column(String(32), default="ourairports")
    ingested_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class RunwayRow(Base):
    __tablename__ = "runways"

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    airport_iata: Mapped[str] = mapped_column(String(3), index=True)
    airport_ident: Mapped[str] = mapped_column(String(16), index=True)
    length_ft: Mapped[int | None] = mapped_column(Integer)
    width_ft: Mapped[int | None] = mapped_column(Integer)
    surface: Mapped[str] = mapped_column(String(64), default="")
    lighted: Mapped[bool] = mapped_column(Boolean, default=False)
    closed: Mapped[bool] = mapped_column(Boolean, default=False)
    le_ident: Mapped[str] = mapped_column(String(16), default="")
    he_ident: Mapped[str] = mapped_column(String(16), default="")
    source: Mapped[str] = mapped_column(String(32), default="ourairports")


class NavaidRow(Base):
    __tablename__ = "navaids"

    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    ident: Mapped[str] = mapped_column(String(16), index=True)
    name: Mapped[str] = mapped_column(Text)
    type: Mapped[str] = mapped_column(String(16), default="")
    frequency_khz: Mapped[int | None] = mapped_column(Integer)
    iso_country: Mapped[str] = mapped_column(String(2), default="", index=True)
    associated_airport: Mapped[str | None] = mapped_column(String(16), index=True)
    lat: Mapped[float | None] = mapped_column(Float)
    lon: Mapped[float | None] = mapped_column(Float)
    source: Mapped[str] = mapped_column(String(32), default="ourairports")


class AirlineRow(Base):
    __tablename__ = "airlines"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    iata: Mapped[str | None] = mapped_column(String(8), index=True)
    icao: Mapped[str | None] = mapped_column(String(8), index=True)
    name: Mapped[str] = mapped_column(Text)
    country: Mapped[str] = mapped_column(Text, default="")
    active: Mapped[bool] = mapped_column(Boolean, default=True)
    source: Mapped[str] = mapped_column(String(32), default="openflights")
    source_note: Mapped[str] = mapped_column(Text, default="")


class RouteRow(Base):
    __tablename__ = "routes"
    __table_args__ = (UniqueConstraint("airline_iata", "origin_iata", "dest_iata", name="uq_route"),)

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    airline_iata: Mapped[str] = mapped_column(String(8), index=True)
    origin_iata: Mapped[str] = mapped_column(String(3), index=True)
    dest_iata: Mapped[str] = mapped_column(String(3), index=True)
    codeshare: Mapped[bool] = mapped_column(Boolean, default=False)
    stops: Mapped[int] = mapped_column(Integer, default=0)
    equipment: Mapped[str] = mapped_column(String(64), default="")
    source: Mapped[str] = mapped_column(String(32), default="openflights")
    source_note: Mapped[str] = mapped_column(Text, default="")


class SearchRow(Base):
    __tablename__ = "searches"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    origin: Mapped[str] = mapped_column(String(3), index=True)
    destination: Mapped[str] = mapped_column(String(3), index=True)
    date: Mapped[str] = mapped_column(String(10), index=True)
    adults: Mapped[int] = mapped_column(Integer, default=1)
    cabin: Mapped[str] = mapped_column(String(24), default="ECONOMY")
    currency: Mapped[str] = mapped_column(String(8), default="USD")
    elapsed_ms: Mapped[float] = mapped_column(Float, default=0)
    sources: Mapped[list] = mapped_column(JSONB, default=list)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class OfferRow(Base):
    __tablename__ = "offers"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    search_id: Mapped[int] = mapped_column(ForeignKey("searches.id", ondelete="CASCADE"), index=True)
    offer_uid: Mapped[str] = mapped_column(String(80), index=True)
    source: Mapped[str] = mapped_column(String(32))
    layer: Mapped[str] = mapped_column(String(24))
    kind: Mapped[str] = mapped_column(String(24))
    price: Mapped[float | None] = mapped_column(Float)
    currency: Mapped[str | None] = mapped_column(String(8))
    payload: Mapped[dict] = mapped_column(JSONB, default=dict)
    retrieved_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class OfferObservationRow(Base):
    """Reusable complete itinerary. Indexed by origin, ticketed dest, and connections."""

    __tablename__ = "offer_observations"
    __table_args__ = (
        UniqueConstraint("fingerprint", "date", "adults", "cabin", "source", name="uq_offer_obs"),
        Index("ix_offer_obs_lookup", "origin", "date", "adults", "cabin", "observed_at"),
    )

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    origin: Mapped[str] = mapped_column(String(3), index=True)
    ticketed: Mapped[str] = mapped_column(String(3), index=True)
    connections: Mapped[list] = mapped_column(JSONB, default=list)
    date: Mapped[str] = mapped_column(String(10), index=True)
    adults: Mapped[int] = mapped_column(Integer, default=1)
    cabin: Mapped[str] = mapped_column(String(24), default="ECONOMY")
    currency: Mapped[str] = mapped_column(String(8), default="USD")
    source: Mapped[str] = mapped_column(String(32), default="")
    fingerprint: Mapped[str] = mapped_column(String(240), index=True)
    price: Mapped[float] = mapped_column(Float)
    payload: Mapped[dict] = mapped_column(JSONB, default=dict)
    observed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class FareObservationRow(Base):
    """Append-only price history. One row per priced offer per time we saw it. Never overwritten."""

    __tablename__ = "fare_observations"
    __table_args__ = (
        Index("ix_fare_obs_itin", "fingerprint", "observed_at"),
        Index("ix_fare_obs_pair", "origin", "ticketed", "date"),
    )

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    offer_uid: Mapped[str] = mapped_column(String(80), index=True)
    fingerprint: Mapped[str] = mapped_column(String(240))
    origin: Mapped[str] = mapped_column(String(3))
    ticketed: Mapped[str] = mapped_column(String(3))
    connections: Mapped[list] = mapped_column(JSONB, default=list)
    stops: Mapped[int] = mapped_column(Integer, default=0)
    carrier: Mapped[str] = mapped_column(String(8), default="")
    provider: Mapped[str] = mapped_column(String(32), index=True)
    price: Mapped[float] = mapped_column(Float)
    currency: Mapped[str] = mapped_column(String(8), default="USD")
    date: Mapped[str] = mapped_column(String(10))
    adults: Mapped[int] = mapped_column(Integer, default=1)
    cabin: Mapped[str] = mapped_column(String(24), default="ECONOMY")
    fare_brand: Mapped[str | None] = mapped_column(String(64))
    expires_at: Mapped[str | None] = mapped_column(String(40))
    observed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class RouteEdgeRow(Base):
    """Flights we have actually seen inside priced itineraries. Learned, not licensed schedule data."""

    __tablename__ = "route_edges"
    __table_args__ = (
        UniqueConstraint("origin", "dest", "carrier", "flight_number", name="uq_route_edge"),
        Index("ix_route_edge_pair", "origin", "dest"),
    )

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    origin: Mapped[str] = mapped_column(String(3))
    dest: Mapped[str] = mapped_column(String(3))
    carrier: Mapped[str] = mapped_column(String(8), default="")
    flight_number: Mapped[str] = mapped_column(String(16), default="")
    observation_count: Mapped[int] = mapped_column(Integer, default=0)
    days_seen: Mapped[int] = mapped_column(Integer, default=0)
    travel_dates: Mapped[list] = mapped_column(JSONB, default=list)
    first_seen: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
    last_seen: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class HiddenCityRouteStatRow(Base):
    """The planner's memory: for A→B, how often a ticket to C routes via B and undercuts A→B."""

    __tablename__ = "hidden_city_route_stats"
    __table_args__ = (
        UniqueConstraint("origin", "intended", "ticketed", name="uq_hc_route_stat"),
        Index("ix_hc_stat_lookup", "origin", "intended", "score"),
    )

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    origin: Mapped[str] = mapped_column(String(3))
    intended: Mapped[str] = mapped_column(String(3))
    ticketed: Mapped[str] = mapped_column(String(3))
    observations: Mapped[int] = mapped_column(Integer, default=0)
    successful_connections: Mapped[int] = mapped_column(Integer, default=0)
    cheaper_than_direct_count: Mapped[int] = mapped_column(Integer, default=0)
    success_rate: Mapped[float] = mapped_column(Float, default=0)
    average_saving: Mapped[float] = mapped_column(Float, default=0)
    median_saving: Mapped[float] = mapped_column(Float, default=0)
    maximum_saving: Mapped[float] = mapped_column(Float, default=0)
    average_saving_percent: Mapped[float] = mapped_column(Float, default=0)
    savings: Mapped[list] = mapped_column(JSONB, default=list)
    currency: Mapped[str] = mapped_column(String(8), default="")
    best_through_price: Mapped[float | None] = mapped_column(Float)
    score: Mapped[float] = mapped_column(Float, default=0, index=True)
    last_success_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    last_cheaper_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    last_checked_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class ProviderCallRow(Base):
    """Search budget ledger. Every paid shop request, what it was for, and what came back."""

    __tablename__ = "provider_calls"
    __table_args__ = (Index("ix_provider_call_route", "provider", "origin", "dest"),)

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    provider: Mapped[str] = mapped_column(String(32))
    origin: Mapped[str] = mapped_column(String(3))
    dest: Mapped[str] = mapped_column(String(3))
    date: Mapped[str] = mapped_column(String(10), default="")
    purpose: Mapped[str] = mapped_column(String(16), default="direct")
    ok: Mapped[bool] = mapped_column(Boolean, default=True)
    offers: Mapped[int] = mapped_column(Integer, default=0)
    latency_ms: Mapped[float] = mapped_column(Float, default=0)
    cost_usd: Mapped[float] = mapped_column(Float, default=0)
    called_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now(), index=True)


class HiddenDealRow(Base):
    __tablename__ = "hidden_deals"
    __table_args__ = (
        UniqueConstraint("origin", "destination", "hidden_city", "date", "first_flight", name="uq_hidden_deal"),
    )

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    origin: Mapped[str] = mapped_column(String(3), index=True)
    destination: Mapped[str] = mapped_column(String(3), index=True)
    hidden_city: Mapped[str] = mapped_column(String(3), index=True)
    origin_city: Mapped[str] = mapped_column(Text, default="")
    dest_city: Mapped[str] = mapped_column(Text, default="")
    hidden_city_name: Mapped[str] = mapped_column(Text, default="")
    date: Mapped[str] = mapped_column(String(10), index=True)
    honest_price: Mapped[float] = mapped_column(Float)
    through_price: Mapped[float] = mapped_column(Float)
    currency: Mapped[str] = mapped_column(String(8), default="USD")
    saving: Mapped[float] = mapped_column(Float, default=0)
    saving_pct: Mapped[float] = mapped_column(Float, default=0)
    first_flight: Mapped[str] = mapped_column(String(16), default="")
    source: Mapped[str] = mapped_column(String(32), default="")
    bookers: Mapped[list] = mapped_column(JSONB, default=list)
    local_payload: Mapped[dict] = mapped_column(JSONB, default=dict)
    through_payload: Mapped[dict] = mapped_column(JSONB, default=dict)
    retrieved_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class TrackRow(Base):
    __tablename__ = "track_snapshots"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    airport_iata: Mapped[str] = mapped_column(String(3), index=True)
    icao24: Mapped[str] = mapped_column(String(16), index=True)
    callsign: Mapped[str | None] = mapped_column(String(16))
    origin_country: Mapped[str | None] = mapped_column(String(64))
    lat: Mapped[float | None] = mapped_column(Float)
    lon: Mapped[float | None] = mapped_column(Float)
    baro_altitude_m: Mapped[float | None] = mapped_column(Float)
    on_ground: Mapped[bool] = mapped_column(Boolean, default=False)
    velocity_ms: Mapped[float | None] = mapped_column(Float)
    true_track: Mapped[float | None] = mapped_column(Float)
    position_source: Mapped[int | None] = mapped_column(Integer)
    api_time: Mapped[int | None] = mapped_column(Integer)
    observed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
