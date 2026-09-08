"""Index complete priced offers by ticketed destination AND connection airports.

One A→B→C observation is a standard trip to C and a hidden-city candidate for B.
Never assign a price to a single leg by subtraction.
"""

from __future__ import annotations

from app.engines.hidden import detect_hidden_city, is_standard_to, itinerary_fingerprint, ticketed_destination
from app.models import Offer


def observation_meta(offer: Offer) -> dict | None:
    if not offer.segments or offer.price is None:
        return None
    first, last = offer.segments[0], offer.segments[-1]
    return {
        "origin": first.origin.upper(),
        "ticketed": last.dest.upper(),
        "connections": [s.dest.upper() for s in offer.segments[:-1]],
        "fingerprint": itinerary_fingerprint(offer),
        "price": offer.price,
        "currency": offer.currency,
        "source": offer.source,
    }


def classify_for_search(offer: Offer, origins: set[str], intended: set[str]) -> str | None:
    if not offer.segments:
        return None
    if offer.segments[0].origin.upper() not in {o.upper() for o in origins}:
        return None
    if is_standard_to(offer, intended):
        return "standard"
    if detect_hidden_city(offer, intended):
        return "hidden_city"
    return None


def ticketed_dests_through(offers: list[Offer], intended: set[str]) -> set[str]:
    """Ticketed C values we already have as complete A→…→C trips that exit at B."""
    covered: set[str] = set()
    for offer in offers:
        if detect_hidden_city(offer, intended):
            covered.add(ticketed_destination(offer))
    return covered
