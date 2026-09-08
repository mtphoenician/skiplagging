from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

from app.models import Cabin, Offer


@dataclass(slots=True, frozen=True)
class ShopRequest:
    origin: str
    dest: str
    date: str
    adults: int = 1
    cabin: Cabin = "ECONOMY"
    currency: str = "USD"
    nonstop: bool = False
    max_offers: int = 20
    return_date: str | None = None


class FareProvider(Protocol):
    name: str

    async def shop(self, req: ShopRequest) -> list[Offer]:
        ...
