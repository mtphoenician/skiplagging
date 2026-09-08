from __future__ import annotations

from cachetools import TTLCache

from app.config import Settings


def build_cache(settings: Settings) -> TTLCache:
    return TTLCache(maxsize=512, ttl=settings.cache_ttl_seconds)


def search_key(
    origin: str,
    dest: str,
    date: str,
    adults: int,
    cabin: str,
    nearby: bool,
    live: bool,
    currency: str = "USD",
) -> tuple:
    return (origin.upper(), dest.upper(), date, adults, cabin, nearby, live, currency.upper(), "v4")
