"""Search budget engine. Every paid shop request is timed and recorded.

The ledger lives in a context variable so concurrent searches each keep their own
list, and the search persists it once at the end. Cost is per-request; Duffel bills
excess searches beyond a search:order ratio, so we treat every request as costing
`settings.search_cost_usd` and let the planner decide if a candidate is worth it.
"""

from __future__ import annotations

import time
from collections.abc import Awaitable
from contextvars import ContextVar
from dataclasses import dataclass, field

from app.models import Offer

Purpose = str  # "direct" | "expand" | "nearby" | "discover" | "interline"


@dataclass(slots=True)
class ProviderCall:
    provider: str
    origin: str
    dest: str
    date: str
    purpose: Purpose
    ok: bool
    offers: int
    latency_ms: float
    cost_usd: float


@dataclass
class Ledger:
    cost_per_call: float = 0.005
    max_paid: int = 12
    reserved: int = 0
    calls: list[ProviderCall] = field(default_factory=list)

    @property
    def total_cost(self) -> float:
        return round(sum(c.cost_usd for c in self.calls), 4)

    def paid(self) -> list[ProviderCall]:
        return [c for c in self.calls if c.cost_usd > 0]

    def remaining(self) -> int:
        return max(0, self.max_paid - len(self.paid()) - self.reserved)

    def reserve(self, n: int = 1) -> bool:
        if n <= 0:
            return True
        if self.remaining() < n:
            return False
        self.reserved += n
        return True

    def release_reserve(self, n: int = 1) -> None:
        self.reserved = max(0, self.reserved - n)


_LEDGER: ContextVar[Ledger | None] = ContextVar("search_ledger", default=None)


def start_ledger(cost_per_call: float, max_paid: int = 12) -> Ledger:
    ledger = Ledger(cost_per_call=cost_per_call, max_paid=max_paid)
    _LEDGER.set(ledger)
    return ledger


def current_ledger() -> Ledger | None:
    return _LEDGER.get()


FREE_PROVIDERS = {"mock"}


async def timed_shop(
    provider: str,
    origin: str,
    dest: str,
    date: str,
    purpose: Purpose,
    coro: Awaitable[list[Offer]],
) -> list[Offer]:
    """Await a provider shop, record it in the current ledger, never raise."""
    ledger = _LEDGER.get()
    paid = provider not in FREE_PROVIDERS
    if paid and ledger is not None and not ledger.reserve():
        close = getattr(coro, "close", None)
        if close:
            close()
        return []
    t0 = time.perf_counter()
    ok = True
    offers: list[Offer] = []
    try:
        offers = await coro
        if not isinstance(offers, list):
            offers = []
    except Exception:
        ok = False
        offers = []
    finally:
        if paid and ledger is not None:
            ledger.release_reserve()
    if ledger is not None:
        ledger.calls.append(
            ProviderCall(
                provider=provider,
                origin=origin.upper(),
                dest=dest.upper(),
                date=date,
                purpose=purpose,
                ok=ok,
                offers=len(offers),
                latency_ms=round((time.perf_counter() - t0) * 1000, 1),
                cost_usd=0.0 if provider in FREE_PROVIDERS else ledger.cost_per_call,
            )
        )
    return offers


def provider_score(ok_count: int, total: int, avg_latency_ms: float) -> float:
    """ProviderScore(route, provider): hit rate with a mild latency penalty. 0.5 when unknown."""
    if total <= 0:
        return 0.5
    hit = (ok_count + 0.5) / (total + 1)
    latency_penalty = min(max(avg_latency_ms, 0.0) / 10_000.0, 0.3)
    return round(max(0.0, min(1.0, hit - latency_penalty)), 4)
