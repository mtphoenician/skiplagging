from __future__ import annotations

from app.graph.airports import AIRPORTS, airport, haversine

# Fortress / connecting hubs where O-D inventory most often diverges
# from local-segment availability (Luttmann–Gaggero: AA, DL, UA networks).
HUBS: frozenset[str] = frozenset(
    {
        "ATL",
        "DFW",
        "ORD",
        "DEN",
        "CLT",
        "IAH",
        "LAX",
        "SFO",
        "SEA",
        "PHX",
        "MSP",
        "DTW",
        "SLC",
        "EWR",
        "JFK",
        "PHL",
        "IAD",
        "MIA",
        "LHR",
        "CDG",
        "AMS",
        "FRA",
        "MUC",
        "MAD",
        "DXB",
        "DOH",
        "IST",
        "SIN",
        "YYZ",
    }
)

# Documented / historically cited inversions used as high-priority C seeds.
# (A, B) -> preferred C list. Academic + platform origin examples.
SEEDED: dict[tuple[str, str], tuple[str, ...]] = {
    ("ORD", "DCA"): ("BOS", "LGA", "JFK", "PWM", "BDL"),
    ("ORD", "IAD"): ("BOS", "LGA", "PHL", "BDL"),
    ("JFK", "SFO"): ("SEA", "PDX", "SJC", "SMF", "RNO"),
    ("EWR", "SFO"): ("SEA", "PDX", "SMF"),
    ("LGA", "ORD"): ("DEN", "MSP", "SLC", "SEA"),
    ("BOS", "CLT"): ("MIA", "MCO", "TPA", "MSY", "JAX"),
    ("GNV", "CLT"): ("LGA", "JFK", "BOS", "PHL", "EWR"),
    ("DCA", "CLT"): ("MIA", "ATL", "MCO", "MSY"),
    ("BOS", "ORD"): ("DEN", "SFO", "LAX", "SEA", "PHX"),
    ("LGA", "ATL"): ("MIA", "MCO", "MSY", "FLL", "TPA"),
    ("DCA", "ATL"): ("MIA", "FLL", "MSY", "MCO"),
    ("BOS", "DTW"): ("MSP", "ORD", "DEN"),
    ("LGA", "DTW"): ("MSP", "ORD", "SLC"),
    ("JFK", "ATL"): ("MIA", "MCO", "FLL", "MSY"),
    ("SFO", "DEN"): ("ORD", "DFW", "ATL", "CLT"),
    ("SEA", "DEN"): ("ORD", "DFW", "ATL"),
    ("AUS", "DFW"): ("ORD", "LGA", "BOS", "MIA"),
    ("SAT", "DFW"): ("ORD", "LGA", "CLT"),
    ("OKC", "DFW"): ("ORD", "LGA", "MIA"),
    ("TUS", "PHX"): ("DEN", "ORD", "LAX"),
    ("ABQ", "DEN"): ("ORD", "DFW", "ATL"),
    ("PDX", "SEA"): ("ANC", "SFO", "LAX"),
    ("SMF", "SFO"): ("SEA", "PDX", "LAX"),
    ("RDU", "CLT"): ("MIA", "ATL", "LGA"),
    ("RIC", "CLT"): ("ATL", "MIA", "LGA"),
    ("PIT", "CLT"): ("MIA", "ATL", "LGA"),
    ("CMH", "CLT"): ("MIA", "ATL", "LGA"),
    ("IND", "ORD"): ("DEN", "SFO", "LAX"),
    ("MKE", "ORD"): ("DEN", "SFO", "LAX"),
    ("BUF", "CLT"): ("MIA", "ATL", "LGA"),
    ("SYR", "CLT"): ("MIA", "ATL", "LGA"),
    ("PWM", "CLT"): ("MIA", "ATL", "LGA"),
    ("LHR", "CDG"): ("FCO", "MAD", "BCN", "AMS"),
    ("LHR", "FRA"): ("MUC", "VIE", "ZRH", "WAW"),
    ("CDG", "AMS"): ("CPH", "OSL", "HEL", "ARN"),
    ("BCN", "MAD"): ("LIS", "FCO", "CDG"),
    ("DUB", "LHR"): ("CDG", "AMS", "FRA"),
    ("MAN", "LHR"): ("CDG", "AMS", "FRA"),
}


def spokes_from(hub: str) -> list[str]:
    """Airports typically reached after connecting at hub, ranked by size/distance."""
    h = hub.upper()
    if h not in AIRPORTS:
        return []
    out: list[tuple[float, str]] = []
    for code, ap in AIRPORTS.items():
        if code == h:
            continue
        km = haversine(h, code)
        if km < 220:
            continue
        # Prefer same-continent-ish short/medium haul for ticketable connections.
        score = km
        if ap.country == AIRPORTS[h].country:
            score *= 0.55
        if code in HUBS:
            score *= 1.15  # C-as-hub is empirically less productive
        if ap.hub_carriers:
            score *= 0.9
        out.append((score, code))
    out.sort()
    return [c for _, c in out]


def hidden_city_candidates(origin: str, dest: str, limit: int = 16) -> list[str]:
    """
    Rank C destinations for A→B→C shopping.

    Priority: documented inversions, then same-country spokes from B,
    then remaining network. C == A and C in dest metro are excluded.
    """
    a, b = origin.upper(), dest.upper()
    origin_ap, dest_ap = airport(a), airport(b)
    if not dest_ap:
        return []

    seen: set[str] = {a, b}
    if origin_ap:
        seen.update(c for c, ap in AIRPORTS.items() if ap.metro == origin_ap.metro)
    seen.update(c for c, ap in AIRPORTS.items() if ap.metro == dest_ap.metro)

    ranked: list[str] = []

    def push(codes: list[str] | tuple[str, ...]) -> None:
        for c in codes:
            if c in AIRPORTS and c not in seen:
                seen.add(c)
                ranked.append(c)

    push(SEEDED.get((a, b), ()))
    # Reverse documented pairs sometimes still yield C options from B.
    for (oa, ob), cs in SEEDED.items():
        if ob == b:
            push(cs)
            push((oa,))

    same_country = [
        c
        for c in spokes_from(b)
        if AIRPORTS[c].country == dest_ap.country
    ]
    push(same_country)
    push(spokes_from(b))
    return ranked[:limit]
