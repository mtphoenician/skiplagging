"""Drop supplier quotes that have already expired.

Indexed reuse and homepage deals must not outlive Duffel's expires_at.
If the field is missing, the observation TTL still applies.
"""

from __future__ import annotations

from datetime import datetime, timezone

from app.models import Offer


def _parse_expiry(raw: str | None) -> datetime | None:
    if not raw:
        return None
    text = raw.strip()
    if not text:
        return None
    try:
        exp = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        return None
    if exp.tzinfo is None:
        exp = exp.replace(tzinfo=timezone.utc)
    return exp


def offer_unexpired(offer: Offer, now: datetime | None = None) -> bool:
    exp = _parse_expiry(offer.expires_at)
    if exp is None:
        return True
    moment = now or datetime.now(timezone.utc)
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=timezone.utc)
    return exp > moment


def unexpired(offers: list[Offer], now: datetime | None = None) -> list[Offer]:
    moment = now or datetime.now(timezone.utc)
    return [o for o in offers if offer_unexpired(o, moment)]
