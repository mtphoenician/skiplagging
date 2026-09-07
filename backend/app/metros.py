"""IATA city / metro codes for multi-airport cities.

OurAirports has airports (LHR), not city codes (LON). Duffel and Google Flights
accept the city code as “all airports in this city.”
"""

from __future__ import annotations

# city_code → (city, ISO2, member airports largest-first, extra search aliases)
METROS: dict[str, tuple[str, str, tuple[str, ...], tuple[str, ...]]] = {
    "NYC": ("New York", "US", ("JFK", "LGA", "EWR"), ("nyc", "new york", "new york city")),
    "LON": ("London", "GB", ("LHR", "LGW", "STN", "LCY", "LTN", "SEN"), ("london", "lon")),
    "PAR": ("Paris", "FR", ("CDG", "ORY", "BVA"), ("paris", "par")),
    "CHI": ("Chicago", "US", ("ORD", "MDW"), ("chicago", "chi")),
    "WAS": ("Washington", "US", ("DCA", "IAD", "BWI"), ("washington", "washington dc", "d.c.")),
    "TYO": ("Tokyo", "JP", ("HND", "NRT"), ("tokyo", "tyo")),
    "OSA": ("Osaka", "JP", ("KIX", "ITM", "UKB"), ("osaka",)),
    "SEL": ("Seoul", "KR", ("ICN", "GMP"), ("seoul",)),
    "MIL": ("Milan", "IT", ("MXP", "LIN", "BGY"), ("milan", "milano")),
    "ROM": ("Rome", "IT", ("FCO", "CIA"), ("rome", "roma")),
    "YTO": ("Toronto", "CA", ("YYZ", "YTZ", "YHM"), ("toronto",)),
    "YMQ": ("Montreal", "CA", ("YUL", "YHU"), ("montreal", "montréal")),
    "BJS": ("Beijing", "CN", ("PEK", "PKX"), ("beijing", "peking")),
    "SHA": ("Shanghai", "CN", ("PVG", "SHA"), ("shanghai",)),
    "SAO": ("São Paulo", "BR", ("GRU", "CGH", "VCP"), ("sao paulo", "são paulo")),
    "RIO": ("Rio de Janeiro", "BR", ("GIG", "SDU"), ("rio", "rio de janeiro")),
    "BUE": ("Buenos Aires", "AR", ("EZE", "AEP"), ("buenos aires",)),
    "BHZ": ("Belo Horizonte", "BR", ("CNF", "PLU"), ("belo horizonte",)),
    "JKT": ("Jakarta", "ID", ("CGK", "HLP"), ("jakarta",)),
    "BKK": ("Bangkok", "TH", ("BKK", "DMK"), ("bangkok",)),
    "MOW": ("Moscow", "RU", ("SVO", "DME", "VKO"), ("moscow", "moskva")),
    "STO": ("Stockholm", "SE", ("ARN", "BMA", "NYO"), ("stockholm",)),
    "REK": ("Reykjavik", "IS", ("KEF", "RKV"), ("reykjavik", "reykjavík")),
    "BUH": ("Bucharest", "RO", ("OTP", "BBU"), ("bucharest",)),
    "TCI": ("Tenerife", "ES", ("TFS", "TFN"), ("tenerife",)),
    "HOU": ("Houston", "US", ("IAH", "HOU"), ("houston",)),
    "DFW": ("Dallas", "US", ("DFW", "DAL"), ("dallas", "fort worth", "dallas fort worth")),
    "LAX": ("Los Angeles", "US", ("LAX", "BUR", "LGB", "SNA", "ONT"), ("los angeles", "la")),
    "SFO": ("San Francisco", "US", ("SFO", "OAK", "SJC"), ("san francisco", "bay area")),
    "MIA": ("Miami", "US", ("MIA", "FLL", "PBI"), ("miami",)),
    "IST": ("Istanbul", "TR", ("IST", "SAW"), ("istanbul",)),
    "BRU": ("Brussels", "BE", ("BRU", "CRL"), ("brussels", "bruxelles")),
    "DUS": ("Düsseldorf", "DE", ("DUS", "NRN"), ("dusseldorf", "düsseldorf")),
    "FRA": ("Frankfurt", "DE", ("FRA", "HHN"), ("frankfurt",)),
    "OSL": ("Oslo", "NO", ("OSL", "TRF", "RYG"), ("oslo",)),
    "JNB": ("Johannesburg", "ZA", ("JNB", "HLA"), ("johannesburg", "joburg")),
    "DXB": ("Dubai", "AE", ("DXB", "DWC"), ("dubai",)),
    "THR": ("Tehran", "IR", ("IKA", "THR"), ("tehran", "teheran")),
    "SPK": ("Sapporo", "JP", ("CTS", "OKD"), ("sapporo",)),
    "MEL": ("Melbourne", "AU", ("MEL", "AVV"), ("melbourne",)),
    "VCE": ("Venice", "IT", ("VCE", "TSF"), ("venice", "venezia")),
}


def normalize_place_id(code: str) -> str:
    t = (code or "").strip().upper().replace("_", "-")
    if t.startswith("CITY-") and len(t) >= 8:
        return f"CITY-{t.split('-', 1)[1][:3]}"
    return t


def catalog_code_for_member(iata: str) -> str | None:
    iata = iata.upper()
    for code, (_city, _cc, members, _aliases) in METROS.items():
        if iata in members:
            return code
    return None
