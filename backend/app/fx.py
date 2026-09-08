"""Every priced row this app shows is USD.

Duffel bills in the organisation currency and airlines often quote GBP/EUR.
We convert after the shop so ranking, hidden-city savings and self-transfer
sums never mix units. Live ECB rates when the network works; a static table
otherwise. Never invent a fare — only rescale an already-priced amount.
"""

from __future__ import annotations

import threading
import time

from app.models import Offer, SeparateTicket

# Units of USD per 1 unit of foreign currency. Used when ECB is unreachable.
_FALLBACK: dict[str, float] = {
    "USD": 1.0,
    "GBP": 1.27,
    "EUR": 1.08,
    "AUD": 0.66,
    "CAD": 0.73,
    "AED": 0.272,
    "SGD": 0.74,
    "HKD": 0.128,
    "JPY": 0.0067,
    "CHF": 1.12,
    "INR": 0.012,
    "MYR": 0.23,
    "THB": 0.028,
    "NZD": 0.59,
    "ZAR": 0.056,
    "SAR": 0.267,
    "QAR": 0.275,
    "KWD": 3.27,
    "BHD": 2.65,
    "OMR": 2.60,
    "CNY": 0.14,
    "KRW": 0.00072,
    "SEK": 0.095,
    "NOK": 0.094,
    "DKK": 0.145,
    "MXN": 0.055,
    "BRL": 0.18,
    "TRY": 0.029,
    "PLN": 0.25,
    "CZK": 0.043,
    "ILS": 0.27,
    "EGP": 0.021,
    "IDR": 0.000061,
    "PHP": 0.017,
    "TWD": 0.031,
}

_rates: dict[str, float] = dict(_FALLBACK)
_fetched_at = 0.0
_TTL = 12 * 3600


def _pull_frankfurter() -> None:
    global _rates
    try:
        import httpx

        r = httpx.get("https://api.frankfurter.app/latest?from=USD", timeout=1.5)
        if r.status_code >= 400:
            return
        quoted = (r.json() or {}).get("rates") or {}
        live = {"USD": 1.0}
        for code, per_usd in quoted.items():
            try:
                per = float(per_usd)
            except (TypeError, ValueError):
                continue
            if per <= 0:
                continue
            live[code.upper()] = round(1.0 / per, 6)
        if len(live) > 1:
            _rates = {**_FALLBACK, **live}
    except Exception:
        return


def _refresh_rates() -> None:
    global _fetched_at
    now = time.time()
    if now - _fetched_at < _TTL and _fetched_at:
        return
    _fetched_at = now
    worker = threading.Thread(target=_pull_frankfurter, daemon=True)
    worker.start()
    worker.join(2.0)


def usd_per_unit(currency: str) -> float | None:
    code = (currency or "USD").upper()
    if code == "USD":
        return 1.0
    _refresh_rates()
    return _rates.get(code)


def to_usd(amount: float | None, currency: str | None) -> float | None:
    if amount is None:
        return None
    code = (currency or "USD").upper()
    if code == "USD":
        return round(float(amount), 2)
    rate = usd_per_unit(code)
    if rate is None:
        return None
    return round(float(amount) * rate, 2)


def offer_in_usd(offer: Offer) -> Offer | None:
    """Return a copy priced in USD, or None if the currency cannot be converted."""
    code = (offer.currency or "USD").upper()
    price = to_usd(offer.price, code)
    if offer.price is not None and price is None:
        return None
    tickets: list[SeparateTicket] = []
    for t in offer.separate_tickets:
        p = to_usd(t.price, t.currency)
        if t.price is not None and p is None:
            return None
        tickets.append(t.model_copy(update={"price": p if p is not None else t.price, "currency": "USD"}))
    already = code == "USD" and all((t.currency or "USD").upper() == "USD" for t in offer.separate_tickets)
    if already:
        return offer
    return offer.model_copy(
        update={
            "price": price,
            "base_price": to_usd(offer.base_price, code),
            "taxes": to_usd(offer.taxes, code),
            "currency": "USD",
            "separate_tickets": tickets,
        }
    )


def offers_in_usd(offers: list[Offer]) -> list[Offer]:
    out: list[Offer] = []
    for offer in offers:
        converted = offer_in_usd(offer)
        if converted is not None:
            out.append(converted)
    return out


def money_in_usd(amount: float | None, currency: str | None) -> tuple[float | None, str]:
    converted = to_usd(amount, currency)
    if converted is None:
        return amount, (currency or "USD").upper()
    return converted, "USD"
