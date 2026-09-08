from __future__ import annotations

import base64
from urllib.parse import quote

from app.metros import METROS
from app.models import BookerLink
from app.providers.sandbox import TEST_CARRIERS


def _varint(n: int) -> bytes:
    if n < 0:
        n += 1 << 64
    out = bytearray()
    while True:
        bit = n & 0x7F
        n >>= 7
        if n:
            out.append(bit | 0x80)
        else:
            out.append(bit)
            break
    return bytes(out)


def _key(field: int, wire: int) -> bytes:
    return _varint((field << 3) | wire)


def _ld(field: int, payload: bytes) -> bytes:
    return _key(field, 2) + _varint(len(payload)) + payload


def _var(field: int, n: int) -> bytes:
    return _key(field, 0) + _varint(n)


def _str(field: int, value: str) -> bytes:
    raw = value.encode()
    return _ld(field, raw)


def _place(code: str) -> bytes:
    return _var(1, 1) + _str(2, code.upper())


def google_tfs(
    origin: str,
    dest: str,
    date: str,
    adults: int = 1,
    cabin: int = 1,
) -> str:
    """URL-safe protobuf `tfs` for a one-way Google Flights search."""
    leg = _str(2, date) + _ld(13, _place(origin)) + _ld(14, _place(dest))
    body = bytearray()
    body += _var(1, 28)
    body += _var(2, 2)
    body += _ld(3, leg)
    for _ in range(max(1, min(adults, 9))):
        body += _var(8, 1)
    body += _var(9, cabin)
    body += _var(14, 1)
    body += _ld(16, _var(1, -1))
    body += _var(19, 2)
    return base64.urlsafe_b64encode(bytes(body)).decode().rstrip("=")


def google_flights_url(
    origin: str,
    dest: str,
    date: str,
    currency: str = "USD",
    adults: int = 1,
    cabin: str = "ECONOMY",
) -> str:
    o, d, ccy = origin.upper(), dest.upper(), (currency or "USD").upper()
    tfs = google_tfs(o, d, date, adults, _google_cabin(cabin))
    return f"https://www.google.com/travel/flights/search?tfs={tfs}&hl=en&curr={ccy}"


def _google_cabin(cabin: str) -> int:
    return {
        "ECONOMY": 1,
        "PREMIUM_ECONOMY": 2,
        "BUSINESS": 3,
        "FIRST": 4,
    }.get((cabin or "ECONOMY").upper(), 1)


def _booking_point(code: str) -> str:
    return f"{code}.CITY" if code in METROS else f"{code}.AIRPORT"


# Public search URL schemes only. No airline.com roster — those rot.
# Skipped on purpose: Orbitz/Travelocity/Hotwire (Expedia twins), Opodo/GoVoyages
# (eDreams twins), Hopper (app), ITA Matrix (no stable GET deep-link), deal-alert clubs.
_META = (
    (
        "google-flights",
        "Google Flights",
        "meta-search",
        "Metasearch: compares and hands off. Does not issue the e-ticket.",
        False,
        "",
    ),
    (
        "kayak",
        "Kayak",
        "meta-search",
        "Metasearch. Booking completes on an airline or OTA, not on this link alone.",
        False,
        "https://www.kayak.com/flights/{o}-{d}/{date}{kayak_adults}?sort=bestflight_a",
    ),
    (
        "skyscanner",
        "Skyscanner",
        "meta-search",
        "Metasearch. Date path is YYMMDD. Not the validating carrier.",
        False,
        "https://www.skyscanner.com/transport/flights/{ol}/{dl}/{yymmdd}/?adultsv2={adults}&cabinclass={sky_cabin}&rtn=0&preferdirects=false",
    ),
    (
        "momondo",
        "Momondo",
        "meta-search",
        "Kayak-family metasearch. Often wider international OTA coverage.",
        False,
        "https://www.momondo.com/flight-search/{o}-{d}/{date}{kayak_adults}?sort=bestflight_a",
    ),
    (
        "cheapflights",
        "Cheapflights",
        "meta-search",
        "Kayak-family metasearch. US-facing comparison.",
        False,
        "https://www.cheapflights.com/flight-search/{o}-{d}/{date}{kayak_adults}?sort=bestflight_a",
    ),
    (
        "wego",
        "Wego",
        "meta-search",
        "Metasearch strong in the Middle East and Asia.",
        False,
        "https://www.wego.com/flights/searches/{o}-{d}-{dmon}:ow/economy/{adults}a",
    ),
    (
        "expedia",
        "Expedia",
        "ota",
        "OTA. Typically issues via GDS as merchant of record if you finish checkout there.",
        True,
        "https://www.expedia.com/Flights-Search?flight-type=on&mode=search&trip=oneway&leg1=from:{o},to:{d},departure:{us}TANYT&passengers=adults:{adults},children:0,infantinlap:N",
    ),
    (
        "booking-com",
        "Booking.com",
        "ota",
        "OTA. Flight checkout on Booking.com can issue the ticket if you finish there.",
        True,
        "https://flights.booking.com/flights/{ob}-{db}/?type=ONEWAY&adults={adults}&cabinClass={book_cabin}&depart={date}&from={o}&to={d}&sort=BEST",
    ),
    (
        "trip-com",
        "Trip.com",
        "ota",
        "OTA. Trip.com is often merchant of record if you finish checkout there.",
        True,
        "https://www.trip.com/flights/{ol}-to-{dl}/?dcity={o}&acity={d}&ddate={date}&flighttype=ow&class=ys&quantity={adults}",
    ),
    (
        "priceline",
        "Priceline",
        "ota",
        "OTA. Booking Holdings shop. Can issue if you finish checkout there.",
        True,
        "https://www.priceline.com/m/fly/search/{o}-{d}-{ymd}/?cabin-class=ECO&no-of-adults={adults}",
    ),
    (
        "kiwi",
        "Kiwi.com",
        "ota",
        "OTA. Virtual interlining / self-transfer specialist. Issues if you finish there.",
        True,
        "https://www.kiwi.com/en/search/results/{ol}/{dl}/{date}/no-return?adults={adults}",
    ),
    (
        "cheapoair",
        "CheapOair",
        "ota",
        "US OTA. Can issue if you finish checkout there.",
        True,
        "https://www.cheapoair.com/flights/results?from={o}&to={d}&fromDt={us}&tripType=ONEWAY&adults={adults}",
    ),
    (
        "edreams",
        "eDreams",
        "ota",
        "European OTA. Can issue if you finish checkout there.",
        True,
        "https://www.edreams.com/flights/{ol}-{dl}/{date}/{adults}-0-0/",
    ),
    (
        "traveloka",
        "Traveloka",
        "ota",
        "Southeast Asia OTA. Can issue if you finish checkout there.",
        True,
        "https://www.traveloka.com/en-en/flight/fullsearch?ap={o}.{d}&dt={tvldt}.NA&ps={adults}.0.0",
    ),
    (
        "makemytrip",
        "MakeMyTrip",
        "ota",
        "India OTA. Can issue if you finish checkout there.",
        True,
        "https://www.makemytrip.com/flight/search?itinerary={o}-{d}-{mmt}&tripType=O&paxType=A-{adults}_C-0_I-0&cabinClass=E",
    ),
    (
        "despegar",
        "Despegar",
        "ota",
        "Latin America OTA. Can issue if you finish checkout there.",
        True,
        "https://www.despegar.com/shop/flights/results/oneway/{o}/{d}/{date}/{adults}/0/0",
    ),
    (
        "skiplagged-com",
        "Skiplagged.com",
        "specialist-meta",
        "Specialist hidden-city search. Independent of this repo.",
        False,
        "https://skiplagged.com/flights/{o}/{d}/{date}",
    ),
)


def booker_links(
    origin: str,
    dest: str,
    date: str,
    adults: int = 1,
    airlines: list[tuple[str, str]] | None = None,
    currency: str = "USD",
    cabin: str = "ECONOMY",
) -> list[BookerLink]:
    o, d = origin.upper(), dest.upper()
    ccy = (currency or "USD").upper()
    cabin_u = (cabin or "ECONOMY").upper()
    ctx = {
        "o": o,
        "d": d,
        "ol": o.lower(),
        "dl": d.lower(),
        "ob": _booking_point(o),
        "db": _booking_point(d),
        "date": date,
        "yymmdd": date[2:].replace("-", ""),
        "ymd": date.replace("-", ""),
        "dmon": _dmon(date),
        "mmt": _mmt(date),
        "tvldt": _tvldt(date),
        "adults": adults,
        "kayak_adults": "" if adults <= 1 else f"/{adults}adults",
        "sky_cabin": {
            "ECONOMY": "economy",
            "PREMIUM_ECONOMY": "premiumeconomy",
            "BUSINESS": "business",
            "FIRST": "first",
        }.get(cabin_u, "economy"),
        "book_cabin": cabin_u,
        "us": _us(date),
        "q": quote(f"one way flights from {o} to {d} on {date}"),
    }
    links: list[BookerLink] = []
    for sid, name, layer, role, issues, tmpl in _META:
        url = (
            google_flights_url(o, d, date, ccy, adults, cabin_u)
            if sid == "google-flights"
            else tmpl.format(**ctx)
        )
        links.append(
            BookerLink(
                id=sid,
                name=name,
                layer=layer,  # type: ignore[arg-type]
                role=role,
                url=url,
                issues_ticket=issues,
            )
        )
    google = google_flights_url(o, d, date, ccy, adults, cabin_u)
    for iata, name in airlines or []:
        iata = iata.upper()
        if len(iata) < 2 or iata in TEST_CARRIERS:
            continue
        links.append(
            BookerLink(
                id=f"carrier-{iata}",
                name=f"{name} ({iata})",
                layer="airline-direct",
                role="Carrier name comes from the airline table. Link is a metasearch prefilter, not a PNR on the airline host.",
                url=google,
                issues_ticket=False,
            )
        )
    return links


def _us(date: str) -> str:
    y, m, d = date.split("-")
    return f"{m}/{d}/{y}"


def _dmon(date: str) -> str:
    y, m, d = date.split("-")
    months = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split()
    return f"{int(d):02d}{months[int(m) - 1]}{y}"


def _mmt(date: str) -> str:
    y, m, d = date.split("-")
    months = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split()
    return f"{int(d):02d}{months[int(m) - 1]}{y[2:]}"


def _tvldt(date: str) -> str:
    y, m, d = date.split("-")
    return f"{d}-{m}-{y}"
