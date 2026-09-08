from app.providers.bookers import booker_links, google_flights_url, google_tfs


def test_google_tfs_matches_known_one_way():
    # JFK → LHR on 2026-02-18, 1 adult, economy — captured Google Flights URL
    assert google_tfs("JFK", "LHR", "2026-02-18") == (
        "CBwQAhoeEgoyMDI2LTAyLTE4agcIARIDSkZLcgcIARIDTEhSQAFIAXABggELCP___________wGYAQI"
    )


def test_google_url_prefills_search_not_hash():
    url = google_flights_url("JFK", "ORD", "2026-10-10", "USD", 1, "ECONOMY")
    assert url.startswith("https://www.google.com/travel/flights/search?tfs=")
    assert "JFK" not in url.split("tfs=")[0]  # airports live inside tfs
    assert "curr=USD" in url
    assert "#flt=" not in url


def test_all_bookers_carry_route_and_date():
    links = booker_links("JFK", "ORD", "2026-10-10", 1, currency="USD", cabin="ECONOMY")
    by_id = {b.id: b.url for b in links}
    assert "tfs=" in by_id["google-flights"]
    assert "JFK-ORD/2026-10-10" in by_id["kayak"]
    assert "/jfk/ord/261010/" in by_id["skyscanner"]
    assert "adultsv2=1" in by_id["skyscanner"] and "rtn=0" in by_id["skyscanner"]
    assert "JFK.AIRPORT-ORD.AIRPORT" in by_id["booking-com"]
    assert "depart=2026-10-10" in by_id["booking-com"]
    assert "from:JFK,to:ORD,departure:10/10/2026" in by_id["expedia"]
    assert "trip=oneway" in by_id["expedia"]
    assert "/jfk/ord/2026-10-10/no-return" in by_id["kiwi"]
    assert "JFK/ORD/2026-10-10" in by_id["skiplagged-com"]
    assert "jfk-ord/2026-10-10/1-0-0" in by_id["edreams"]
    for sid, url in by_id.items():
        if sid == "google-flights" or sid.startswith("carrier-"):
            continue
        assert "JFK" in url.upper() and "ORD" in url.upper(), sid
        assert "2026" in url or "261010" in url or "10Oct26" in url, sid
