"""Re-price a stored offer. Mock fixtures and live Duffel GET /air/offers/{id}."""

from __future__ import annotations

import httpx

from app.config import Settings
from app.fx import offer_in_usd
from app.models import Offer
from app.providers.duffel import DuffelProvider
from app.providers.mock import MockProvider


def _sandbox(offer: Offer) -> bool:
    if offer.live is False:
        return True
    note = offer.note or ""
    return offer.source == "duffel" and "live_mode=False" in note


async def refresh_priced_offer(
    offer: Offer,
    settings: Settings,
    client: httpx.AsyncClient | None = None,
) -> Offer | None:
    """Latest supplier price in USD, or None if the fare cannot be confirmed."""
    source = (offer.source or "").lower()
    oid = offer.id or ""
    fresh: Offer | None = None
    if source == "mock" or oid.startswith("mock-"):
        fresh = await MockProvider().refresh_offer(offer)
    elif source == "duffel" or oid.startswith("duffel-"):
        if not settings.duffel_enabled or client is None:
            return None
        fresh = await DuffelProvider(settings, client).refresh_offer(offer)
        if fresh and _sandbox(fresh):
            fresh = None
    else:
        return None
    if fresh is None:
        return None
    return offer_in_usd(fresh)
