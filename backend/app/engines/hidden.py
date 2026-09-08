"""Classify complete priced itineraries. Never invent a shorter ticket."""

from __future__ import annotations

from dataclasses import dataclass

from app.models import Offer, Segment

HIDDEN_CITY_WARNINGS = [
    "Hidden-city itineraries carry special restrictions.",
    "Do not check baggage unless your baggage is confirmed to be delivered at your intended stop.",
    "Skipping a segment may cancel all subsequent segments on the same ticket.",
    "Airline contracts and policies may restrict hidden-city ticketing.",
    "Schedule changes may alter the connection airport.",
    "This prototype does not provide legal or contractual advice.",
]


@dataclass(frozen=True)
class HiddenCityHit:
    exit_airport: str
    exit_segment_index: int
    unused_segments: list[Segment]


def _intended_set(intended: str | set[str]) -> set[str]:
    if isinstance(intended, str):
        return {intended.upper()}
    return {code.upper() for code in intended}


def ticketed_destination(offer: Offer) -> str:
    if not offer.segments:
        return ""
    return offer.segments[-1].dest.upper()


def detect_hidden_city(offer: Offer, intended: str | set[str]) -> HiddenCityHit | None:
    """A hidden-city candidate is a complete ticket that continues past the intended stop.

    The priced itinerary is never rewritten into a fake A→B fare.
    """
    segs = offer.segments
    if len(segs) < 2:
        return None
    dests = _intended_set(intended)
    if ticketed_destination(offer) in dests:
        return None
    for index, segment in enumerate(segs[:-1]):
        if segment.dest.upper() in dests:
            return HiddenCityHit(
                exit_airport=segment.dest.upper(),
                exit_segment_index=index,
                unused_segments=list(segs[index + 1 :]),
            )
    return None


def is_standard_to(offer: Offer, intended: str | set[str]) -> bool:
    return bool(offer.segments) and ticketed_destination(offer) in _intended_set(intended)


def hidden_city_savings(best_standard: float | None, through_price: float | None) -> float | None:
    if best_standard is None or through_price is None:
        return None
    return round(best_standard - through_price, 2)


def meaningful_saving(saving: float | None, floor: float | None = None) -> bool:
    if floor is None:
        from app.config import get_settings

        floor = get_settings().min_hidden_saving
    return saving is not None and saving >= floor


def itinerary_fingerprint(offer: Offer) -> str:
    parts: list[str] = []
    for segment in offer.segments:
        parts.append(
            f"{segment.flight_number}:{segment.origin.upper()}:{segment.dest.upper()}:{(segment.dep or '')[:16]}"
        )
    return "|".join(parts)


def refresh_keeps_hidden_city(original: Offer, refreshed: Offer, intended: str | set[str]) -> bool:
    old = detect_hidden_city(original, intended)
    new = detect_hidden_city(refreshed, intended)
    if old is None or new is None:
        return False
    return old.exit_airport == new.exit_airport
