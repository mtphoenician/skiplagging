from __future__ import annotations

from math import asin, cos, radians, sin, sqrt

from app.models import Airport

# Curated world graph: major O-D markets where hidden-city inversions
# concentrate (hub-and-spoke carriers, competitive beyond-hub cities).
_RAW: list[tuple] = [
    # iata, name, city, country, lat, lon, metro, hubs
    ("ATL", "Hartsfield-Jackson", "Atlanta", "US", 33.6407, -84.4277, "ATL", ["DL"]),
    ("DFW", "Dallas/Fort Worth", "Dallas", "US", 32.8998, -97.0403, "DFW", ["AA"]),
    ("ORD", "O'Hare", "Chicago", "US", 41.9742, -87.9073, "CHI", ["AA", "UA"]),
    ("MDW", "Midway", "Chicago", "US", 41.7868, -87.7522, "CHI", ["WN"]),
    ("DEN", "Denver", "Denver", "US", 39.8561, -104.6737, "DEN", ["UA", "F9"]),
    ("CLT", "Charlotte Douglas", "Charlotte", "US", 35.2140, -80.9431, "CLT", ["AA"]),
    ("IAH", "George Bush Intercontinental", "Houston", "US", 29.9902, -95.3368, "HOU", ["UA"]),
    ("HOU", "Hobby", "Houston", "US", 29.6454, -95.2789, "HOU", ["WN"]),
    ("LAX", "Los Angeles", "Los Angeles", "US", 33.9416, -118.4085, "LAX", ["AA", "DL", "UA"]),
    ("SFO", "San Francisco", "San Francisco", "US", 37.6213, -122.3790, "SFO", ["UA"]),
    ("OAK", "Oakland", "Oakland", "US", 37.7126, -122.2197, "SFO", ["WN"]),
    ("SJC", "Norman Y. Mineta", "San Jose", "US", 37.3639, -121.9289, "SFO", []),
    ("SEA", "Seattle-Tacoma", "Seattle", "US", 47.4502, -122.3088, "SEA", ["AS", "DL"]),
    ("PHX", "Sky Harbor", "Phoenix", "US", 33.4373, -112.0078, "PHX", ["AA"]),
    ("LAS", "Harry Reid", "Las Vegas", "US", 36.0840, -115.1537, "LAS", []),
    ("MSP", "Minneapolis-Saint Paul", "Minneapolis", "US", 44.8848, -93.2223, "MSP", ["DL"]),
    ("DTW", "Detroit Metro", "Detroit", "US", 42.2162, -83.3554, "DTW", ["DL"]),
    ("SLC", "Salt Lake City", "Salt Lake City", "US", 40.7899, -111.9791, "SLC", ["DL"]),
    ("EWR", "Newark Liberty", "Newark", "US", 40.6895, -74.1745, "NYC", ["UA"]),
    ("JFK", "John F. Kennedy", "New York", "US", 40.6413, -73.7781, "NYC", ["DL", "AA", "B6"]),
    ("LGA", "LaGuardia", "New York", "US", 40.7769, -73.8740, "NYC", ["DL", "AA"]),
    ("BOS", "Logan", "Boston", "US", 42.3656, -71.0096, "BOS", ["B6", "DL"]),
    ("PHL", "Philadelphia", "Philadelphia", "US", 39.8729, -75.2437, "PHL", ["AA"]),
    ("IAD", "Dulles", "Washington", "US", 38.9531, -77.4565, "WAS", ["UA"]),
    ("DCA", "Ronald Reagan National", "Washington", "US", 38.8512, -77.0402, "WAS", ["AA"]),
    ("BWI", "Baltimore/Washington", "Baltimore", "US", 39.1774, -76.6684, "WAS", ["WN"]),
    ("MIA", "Miami", "Miami", "US", 25.7959, -80.2870, "MIA", ["AA"]),
    ("FLL", "Fort Lauderdale", "Fort Lauderdale", "US", 26.0742, -80.1506, "MIA", ["B6", "NK"]),
    ("MCO", "Orlando", "Orlando", "US", 28.4312, -81.3081, "MCO", []),
    ("TPA", "Tampa", "Tampa", "US", 27.9755, -82.5332, "TPA", []),
    ("BNA", "Nashville", "Nashville", "US", 36.1263, -86.6774, "BNA", ["WN"]),
    ("AUS", "Austin-Bergstrom", "Austin", "US", 30.1945, -97.6699, "AUS", []),
    ("SAN", "San Diego", "San Diego", "US", 32.7338, -117.1933, "SAN", []),
    ("PDX", "Portland", "Portland", "US", 45.5898, -122.5951, "PDX", ["AS"]),
    ("RDU", "Raleigh-Durham", "Raleigh", "US", 35.8801, -78.7880, "RDU", []),
    ("PIT", "Pittsburgh", "Pittsburgh", "US", 40.4915, -80.2329, "PIT", []),
    ("CLE", "Cleveland Hopkins", "Cleveland", "US", 41.4117, -81.8498, "CLE", []),
    ("CVG", "Cincinnati/Northern Kentucky", "Cincinnati", "US", 39.0488, -84.6678, "CVG", ["DL"]),
    ("IND", "Indianapolis", "Indianapolis", "US", 39.7173, -86.2944, "IND", []),
    ("MCI", "Kansas City", "Kansas City", "US", 39.2976, -94.7139, "MCI", []),
    ("STL", "St. Louis Lambert", "St. Louis", "US", 38.7487, -90.3700, "STL", []),
    ("MKE", "Milwaukee", "Milwaukee", "US", 42.9472, -87.8966, "MKE", []),
    ("CMH", "John Glenn", "Columbus", "US", 39.9980, -82.8919, "CMH", []),
    ("BDL", "Bradley", "Hartford", "US", 41.9389, -72.6832, "BDL", []),
    ("RIC", "Richmond", "Richmond", "US", 37.5052, -77.3197, "RIC", []),
    ("JAX", "Jacksonville", "Jacksonville", "US", 30.4941, -81.6879, "JAX", []),
    ("CHS", "Charleston", "Charleston", "US", 32.8986, -80.0405, "CHS", []),
    ("SAV", "Savannah/Hilton Head", "Savannah", "US", 32.1276, -81.2021, "SAV", []),
    ("MSY", "Louis Armstrong", "New Orleans", "US", 29.9934, -90.2580, "MSY", []),
    ("SAT", "San Antonio", "San Antonio", "US", 29.5337, -98.4698, "SAT", []),
    ("OKC", "Will Rogers", "Oklahoma City", "US", 35.3931, -97.6007, "OKC", []),
    ("ABQ", "Albuquerque", "Albuquerque", "US", 35.0402, -106.6092, "ABQ", []),
    ("ELP", "El Paso", "El Paso", "US", 31.8072, -106.3778, "ELP", []),
    ("TUS", "Tucson", "Tucson", "US", 32.1161, -110.9410, "TUS", []),
    ("SMF", "Sacramento", "Sacramento", "US", 38.6954, -121.5908, "SMF", []),
    ("RNO", "Reno-Tahoe", "Reno", "US", 39.4993, -119.7681, "RNO", []),
    ("BOI", "Boise", "Boise", "US", 43.5644, -116.2228, "BOI", []),
    ("ANC", "Ted Stevens", "Anchorage", "US", 61.1743, -149.9963, "ANC", ["AS"]),
    ("HNL", "Daniel K. Inouye", "Honolulu", "US", 21.3187, -157.9225, "HNL", ["HA"]),
    ("BUF", "Buffalo Niagara", "Buffalo", "US", 42.9405, -78.7322, "BUF", []),
    ("ROC", "Greater Rochester", "Rochester", "US", 43.1189, -77.6724, "ROC", []),
    ("SYR", "Syracuse Hancock", "Syracuse", "US", 43.1112, -76.1063, "SYR", []),
    ("PWM", "Portland International Jetport", "Portland ME", "US", 43.6462, -70.3087, "PWM", []),
    ("BTV", "Burlington", "Burlington", "US", 44.4719, -73.1533, "BTV", []),
    ("GNV", "Gainesville", "Gainesville", "US", 29.6900, -82.2718, "GNV", []),
    ("PBI", "Palm Beach", "West Palm Beach", "US", 26.6832, -80.0956, "PBI", []),
    ("RSW", "Southwest Florida", "Fort Myers", "US", 26.5362, -81.7552, "RSW", []),
    ("YYZ", "Toronto Pearson", "Toronto", "CA", 43.6777, -79.6248, "YTO", ["AC"]),
    ("YUL", "Montréal-Trudeau", "Montreal", "CA", 45.4706, -73.7408, "YMQ", ["AC"]),
    ("YVR", "Vancouver", "Vancouver", "CA", 49.1947, -123.1792, "YVR", ["AC"]),
    ("YYC", "Calgary", "Calgary", "CA", 51.1215, -114.0076, "YYC", ["WS"]),
    ("MEX", "Mexico City", "Mexico City", "MX", 19.4363, -99.0721, "MEX", ["AM"]),
    ("CUN", "Cancún", "Cancun", "MX", 21.0365, -86.8771, "CUN", []),
    ("LHR", "Heathrow", "London", "GB", 51.4700, -0.4543, "LON", ["BA"]),
    ("LGW", "Gatwick", "London", "GB", 51.1537, -0.1821, "LON", ["U2", "BA"]),
    ("STN", "Stansted", "London", "GB", 51.8860, 0.2389, "LON", ["FR"]),
    ("LCY", "London City", "London", "GB", 51.5053, 0.0553, "LON", []),
    ("CDG", "Charles de Gaulle", "Paris", "FR", 49.0097, 2.5479, "PAR", ["AF"]),
    ("ORY", "Orly", "Paris", "FR", 48.7233, 2.3794, "PAR", ["AF", "TO"]),
    ("AMS", "Schiphol", "Amsterdam", "NL", 52.3105, 4.7683, "AMS", ["KL"]),
    ("FRA", "Frankfurt", "Frankfurt", "DE", 50.0379, 8.5622, "FRA", ["LH"]),
    ("MUC", "Munich", "Munich", "DE", 48.3537, 11.7750, "MUC", ["LH"]),
    ("MAD", "Adolfo Suárez Madrid-Barajas", "Madrid", "ES", 40.4983, -3.5676, "MAD", ["IB"]),
    ("BCN", "Barcelona-El Prat", "Barcelona", "ES", 41.2974, 2.0833, "BCN", ["VY"]),
    ("FCO", "Fiumicino", "Rome", "IT", 41.8003, 12.2389, "ROM", ["AZ"]),
    ("MXP", "Malpensa", "Milan", "IT", 45.6306, 8.7281, "MIL", []),
    ("ZRH", "Zurich", "Zurich", "CH", 47.4647, 8.5492, "ZRH", ["LX"]),
    ("VIE", "Vienna", "Vienna", "AT", 48.1103, 16.5697, "VIE", ["OS"]),
    ("CPH", "Copenhagen", "Copenhagen", "DK", 55.6180, 12.6508, "CPH", ["SK"]),
    ("ARN", "Stockholm Arlanda", "Stockholm", "SE", 59.6498, 17.9238, "STO", ["SK"]),
    ("OSL", "Oslo Gardermoen", "Oslo", "NO", 60.1976, 11.0004, "OSL", ["SK"]),
    ("HEL", "Helsinki", "Helsinki", "FI", 60.3172, 24.9633, "HEL", ["AY"]),
    ("DUB", "Dublin", "Dublin", "IE", 53.4264, -6.2499, "DUB", ["EI"]),
    ("LIS", "Humberto Delgado", "Lisbon", "PT", 38.7742, -9.1342, "LIS", ["TP"]),
    ("BRU", "Brussels", "Brussels", "BE", 50.9010, 4.4856, "BRU", ["SN"]),
    ("WAW", "Warsaw Chopin", "Warsaw", "PL", 52.1657, 20.9671, "WAW", ["LO"]),
    ("PRG", "Václav Havel", "Prague", "CZ", 50.1008, 14.2600, "PRG", []),
    ("BUD", "Budapest Ferenc Liszt", "Budapest", "HU", 47.4298, 19.2611, "BUD", []),
    ("ATH", "Eleftherios Venizelos", "Athens", "GR", 37.9364, 23.9445, "ATH", ["A3"]),
    ("IST", "Istanbul", "Istanbul", "TR", 41.2753, 28.7519, "IST", ["TK"]),
    ("DXB", "Dubai", "Dubai", "AE", 25.2532, 55.3657, "DXB", ["EK"]),
    ("DOH", "Hamad", "Doha", "QA", 25.2731, 51.6081, "DOH", ["QR"]),
    ("AUH", "Zayed", "Abu Dhabi", "AE", 24.4330, 54.6511, "AUH", ["EY"]),
    ("SIN", "Changi", "Singapore", "SG", 1.3644, 103.9915, "SIN", ["SQ"]),
    ("HKG", "Hong Kong", "Hong Kong", "HK", 22.3080, 113.9185, "HKG", ["CX"]),
    ("NRT", "Narita", "Tokyo", "JP", 35.7720, 140.3929, "TYO", ["NH", "JL"]),
    ("HND", "Haneda", "Tokyo", "JP", 35.5494, 139.7798, "TYO", ["NH", "JL"]),
    ("ICN", "Incheon", "Seoul", "KR", 37.4602, 126.4407, "SEL", ["KE", "OZ"]),
    ("PEK", "Beijing Capital", "Beijing", "CN", 40.0799, 116.6031, "BJS", ["CA"]),
    ("PVG", "Pudong", "Shanghai", "CN", 31.1443, 121.8083, "SHA", ["MU"]),
    ("BKK", "Suvarnabhumi", "Bangkok", "TH", 13.6900, 100.7501, "BKK", ["TG"]),
    ("KUL", "Kuala Lumpur", "Kuala Lumpur", "MY", 2.7456, 101.7099, "KUL", ["MH"]),
    ("SYD", "Kingsford Smith", "Sydney", "AU", -33.9399, 151.1753, "SYD", ["QF"]),
    ("MEL", "Melbourne", "Melbourne", "AU", -37.6733, 144.8433, "MEL", ["QF"]),
    ("GRU", "Guarulhos", "São Paulo", "BR", -23.4356, -46.4731, "SAO", ["LA", "G3"]),
    ("EZE", "Ministro Pistarini", "Buenos Aires", "AR", -34.8222, -58.5358, "BUE", ["AR"]),
    ("BOG", "El Dorado", "Bogotá", "CO", 4.7016, -74.1469, "BOG", ["AV"]),
    ("LIM", "Jorge Chávez", "Lima", "PE", -12.0219, -77.1143, "LIM", ["LA"]),
    ("SCL", "Arturo Merino Benítez", "Santiago", "CL", -33.3930, -70.7858, "SCL", ["LA"]),
    ("JNB", "O. R. Tambo", "Johannesburg", "ZA", -26.1392, 28.2460, "JNB", ["SA"]),
    ("CAI", "Cairo", "Cairo", "EG", 30.1219, 31.4056, "CAI", ["MS"]),
    ("NBO", "Jomo Kenyatta", "Nairobi", "KE", -1.3192, 36.9278, "NBO", ["KQ"]),
    ("ADD", "Bole", "Addis Ababa", "ET", 8.9779, 38.7993, "ADD", ["ET"]),
]

AIRPORTS: dict[str, Airport] = {
    row[0]: Airport(
        iata=row[0],
        name=row[1],
        city=row[2],
        country=row[3],
        lat=row[4],
        lon=row[5],
        metro=row[6],
        hub_carriers=list(row[7]),
    )
    for row in _RAW
}

METRO: dict[str, list[str]] = {}
for _a in AIRPORTS.values():
    METRO.setdefault(_a.metro, []).append(_a.iata)


def airport(code: str) -> Airport | None:
    return AIRPORTS.get(code.upper())


def nearby(code: str, include_self: bool = False) -> list[str]:
    a = airport(code)
    if not a:
        return [code.upper()] if include_self else []
    codes = METRO.get(a.metro, [a.iata])
    if include_self:
        return codes
    return [c for c in codes if c != a.iata]


def haversine(a: str, b: str) -> float:
    """Great-circle distance in kilometres."""
    x, y = airport(a), airport(b)
    if not x or not y:
        return 0.0
    lon1, lat1, lon2, lat2 = map(radians, (x.lon, x.lat, y.lon, y.lat))
    dlon, dlat = lon2 - lon1, lat2 - lat1
    h = sin(dlat / 2) ** 2 + cos(lat1) * cos(lat2) * sin(dlon / 2) ** 2
    return 6371.0 * 2 * asin(sqrt(h))
