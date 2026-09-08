"""Supplier sandbox inventory — not real airlines.

Duffel test tokens issue ZZ / XX / YY (Duffel Airways). Drop those whenever
any real carrier exists. This is a supplier quirk, not a geography list.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.models import Offer

TEST_CARRIERS = frozenset({"ZZ", "XX", "YY"})


def is_sandbox_offer(offer: Offer) -> bool:
    if offer.live is False:
        return True
    note = offer.note or ""
    return offer.source == "duffel" and "live_mode=False" in note


def _carrier_codes(offer: Offer) -> set[str]:
    codes = {offer.carrier, offer.validating_airline}
    for segment in offer.segments:
        codes.add(segment.carrier)
        if segment.operating_carrier:
            codes.add(segment.operating_carrier)
    return {c.upper() for c in codes if c}


def is_live_fare(offer: Offer) -> bool:
    """True when the row is real published inventory, not mock or Duffel test."""
    if is_sandbox_offer(offer):
        return False
    if (offer.source or "").lower() == "mock":
        return False
    codes = _carrier_codes(offer)
    if codes and codes <= TEST_CARRIERS:
        return False
    if (offer.source or "").lower() == "duffel":
        return offer.live is True
    return True
