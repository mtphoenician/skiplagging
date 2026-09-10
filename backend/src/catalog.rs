use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::models::SourceDef;

fn src(
    id: &str,
    name: &str,
    layer: &str,
    role: &str,
    is_not: &str,
    freshness: &str,
    url: &str,
    can_price: bool,
    can_book: bool,
    can_track: bool,
) -> SourceDef {
    SourceDef {
        id: id.to_string(),
        name: name.to_string(),
        layer: layer.to_string(),
        role: role.to_string(),
        is_not: is_not.to_string(),
        freshness: freshness.to_string(),
        url: url.to_string(),
        can_price,
        can_book,
        can_track,
    }
}

pub static SOURCES: LazyLock<Vec<SourceDef>> = LazyLock::new(|| {
    vec![
        src(
            "ourairports",
            "OurAirports",
            "reference",
            "Nightly public-domain dumps: countries.csv, regions.csv, airports.csv, runways.csv, navaids.csv. This is the aviation gazetteer, not a shop.",
            "Not a timetable, fare, booking, or live track. Country codes here are ISO 3166-1 alpha-2 as used on airport rows.",
            "Regenerated nightly; public domain.",
            "https://ourairports.com/data/",
            false,
            false,
            false,
        ),
        src(
            "geonames-countries",
            "GeoNames countryInfo",
            "reference",
            "ISO2/ISO3, capital, currency, population, languages, phone, TLD. Joined to OurAirports countries on ISO 3166-1 alpha-2.",
            "Not an airport database. CC BY attribution to GeoNames.",
            "Downloadable dump from download.geonames.org.",
            "https://download.geonames.org/export/dump/",
            false,
            false,
            false,
        ),
        src(
            "openflights-routes",
            "OpenFlights routes",
            "historical-route-map",
            "Which airlines historically published a nonstop between two IATA codes.",
            "Not current schedules, not inventory, not a price. Last material dump ~2014–2017.",
            "Frozen / stale. Use only as a structural prior for C-candidate generation.",
            "https://openflights.org/data.php",
            false,
            false,
            false,
        ),
        src(
            "opensky",
            "OpenSky Network",
            "live-track",
            "ADS-B / MLAT / ASTERIX / FLARM state vectors: who is in the air right now near an airport.",
            "Not a ticket, not a fare, not a future schedule. Coverage has gaps (oceans, sparse receivers). Anonymous access is current states only, ~10s resolution. OAuth (client id + secret) is a higher rate limit, not a fare shop.",
            "Live (seconds). Anonymous unless OPENSKY_CLIENT_ID and OPENSKY_CLIENT_SECRET are both set.",
            "https://opensky-network.org",
            false,
            false,
            true,
        ),
        src(
            "flightradar24",
            "Flightradar24",
            "live-track-link",
            "Commercial tracker (ADS-B + MLAT + radar + FAA). We deep-link; we do not scrape or resell their feed.",
            "Not a booker. A position on FR24 is not a reservation.",
            "Live on their site.",
            "https://www.flightradar24.com",
            false,
            false,
            true,
        ),
        src(
            "flightaware",
            "FlightAware",
            "live-track-link",
            "Commercial tracker / status (FAA SWIM + ADS-B). Deep-link only.",
            "Not a booker. AeroAPI (paid) would be a status API; this app does not call it unless you add a key later.",
            "Live on their site.",
            "https://www.flightaware.com",
            false,
            false,
            true,
        ),
        src(
            "aerodatabox",
            "AeroDataBox FIDS",
            "schedule-status",
            "Airport flight-information board: scheduled / estimated / actual departures and arrivals.",
            "Not a fare shop and not ADS-B tracking (unless withLocation). Requires a RapidAPI key.",
            "Operational board, typically hours around now — not a 330-day timetable.",
            "https://aerodatabox.com",
            false,
            false,
            false,
        ),
        src(
            "duffel",
            "Duffel",
            "priced-offer",
            "Airline-retailing aggregator (NDC + other). Offer Request = shop. Order = book. This app shops only; it never creates an order or takes payment.",
            "Test tokens return Duffel Airways sandbox offers, which are not real tickets. This app drops every sandbox offer (_drop_sandbox).",
            "Live shop only with a duffel_live_… token. A duffel_test_… token is sandbox — offers are fetched then deleted.",
            "https://duffel.com/docs",
            true,
            false,
            false,
        ),
        src(
            "google-flights",
            "Google Flights",
            "meta-search",
            "Metasearch. Compares airline/OTA prices and hands you off. Deep-link only.",
            "Does not issue the e-ticket. Not a GDS you query from this API.",
            "Live on Google when you click through.",
            "https://www.google.com/travel/flights",
            false,
            false,
            false,
        ),
        src(
            "kayak",
            "Kayak",
            "meta-search",
            "Metasearch / comparison. Deep-link only.",
            "Usually not the validating carrier. The booking happens on an airline or OTA.",
            "Live on Kayak when you click through.",
            "https://www.kayak.com",
            false,
            false,
            false,
        ),
        src(
            "skyscanner",
            "Skyscanner",
            "meta-search",
            "Metasearch. Deep-link only.",
            "Not the ticket issuer.",
            "Live on Skyscanner when you click through.",
            "https://www.skyscanner.com",
            false,
            false,
            false,
        ),
        src(
            "momondo",
            "Momondo",
            "meta-search",
            "Kayak-family metasearch. Deep-link only.",
            "Not the ticket issuer.",
            "Live on Momondo when you click through.",
            "https://www.momondo.com",
            false,
            false,
            false,
        ),
        src(
            "cheapflights",
            "Cheapflights",
            "meta-search",
            "Kayak-family metasearch. Deep-link only.",
            "Not the ticket issuer.",
            "Live on Cheapflights when you click through.",
            "https://www.cheapflights.com",
            false,
            false,
            false,
        ),
        src(
            "wego",
            "Wego",
            "meta-search",
            "Metasearch for Middle East and Asia. Deep-link only.",
            "Not the ticket issuer.",
            "Live on Wego when you click through.",
            "https://www.wego.com",
            false,
            false,
            false,
        ),
        src(
            "expedia",
            "Expedia",
            "ota",
            "Online travel agency. Typically issues via GDS as merchant of record.",
            "Not airline-direct NDC (except where Expedia has NDC contracts). Deep-link only.",
            "Live on Expedia when you click through.",
            "https://www.expedia.com",
            false,
            true,
            false,
        ),
        src(
            "booking-com",
            "Booking.com",
            "ota",
            "OTA flight shop. Deep-link only — we open their one-way search, we do not book.",
            "Not a fare we price in this API. Checkout happens on Booking.com.",
            "Live on Booking.com when you click through.",
            "https://flights.booking.com",
            false,
            true,
            false,
        ),
        src(
            "trip-com",
            "Trip.com",
            "ota",
            "OTA flight shop. Deep-link only — we open their one-way search, we do not book.",
            "Not a fare we price in this API. Checkout happens on Trip.com.",
            "Live on Trip.com when you click through.",
            "https://www.trip.com/flights",
            false,
            true,
            false,
        ),
        src(
            "priceline",
            "Priceline",
            "ota",
            "OTA. Deep-link only.",
            "Not a fare we price in this API.",
            "Live on Priceline when you click through.",
            "https://www.priceline.com",
            false,
            true,
            false,
        ),
        src(
            "kiwi",
            "Kiwi.com",
            "ota",
            "OTA and virtual-interline shop. Deep-link only.",
            "Self-transfer is not a hidden-city inversion. We do not book.",
            "Live on Kiwi when you click through.",
            "https://www.kiwi.com",
            false,
            true,
            false,
        ),
        src(
            "cheapoair",
            "CheapOair",
            "ota",
            "US OTA. Deep-link only.",
            "Not a fare we price in this API.",
            "Live on CheapOair when you click through.",
            "https://www.cheapoair.com",
            false,
            true,
            false,
        ),
        src(
            "edreams",
            "eDreams",
            "ota",
            "European OTA. Deep-link only.",
            "Not a fare we price in this API.",
            "Live on eDreams when you click through.",
            "https://www.edreams.com",
            false,
            true,
            false,
        ),
        src(
            "traveloka",
            "Traveloka",
            "ota",
            "Southeast Asia OTA. Deep-link only.",
            "Not a fare we price in this API.",
            "Live on Traveloka when you click through.",
            "https://www.traveloka.com",
            false,
            true,
            false,
        ),
        src(
            "makemytrip",
            "MakeMyTrip",
            "ota",
            "India OTA. Deep-link only.",
            "Not a fare we price in this API.",
            "Live on MakeMyTrip when you click through.",
            "https://www.makemytrip.com",
            false,
            true,
            false,
        ),
        src(
            "despegar",
            "Despegar",
            "ota",
            "Latin America OTA. Deep-link only.",
            "Not a fare we price in this API.",
            "Live on Despegar when you click through.",
            "https://www.despegar.com",
            false,
            true,
            false,
        ),
        src(
            "airline-direct",
            "Airline.com",
            "airline-direct",
            "Carrier website / NDC. The airline is usually merchant of record.",
            "A deep link is a search, not a confirmed PNR.",
            "Live on the carrier site.",
            "https://www.iata.org/en/programs/airline-distribution/ndc/",
            false,
            true,
            false,
        ),
        src(
            "skiplagged-com",
            "Skiplagged.com",
            "specialist-meta",
            "Consumer hidden-city search product. Deep-link for comparison.",
            "This repository is not Skiplagged. Linking is not an endorsement or a booking.",
            "Live on their site.",
            "https://skiplagged.com",
            false,
            false,
            false,
        ),
    ]
});

/// Chip + configured flag for `/sources`. Duffel never uses `"on"`: a test token
/// is sandbox (offers dropped), not a live shop. OpenSky is anonymous or OAuth.
fn source_runtime(id: &str, enabled: &HashMap<String, bool>, default_on: bool) -> SourceRuntime {
    match id {
        "duffel" => {
            if *enabled.get("duffel_live").unwrap_or(&false) {
                SourceRuntime {
                    configured: true,
                    status: "live",
                    status_label: "Live",
                }
            } else if *enabled.get("duffel_sandbox").unwrap_or(&false) {
                SourceRuntime {
                    configured: false,
                    status: "sandbox",
                    status_label: "Sandbox — offers dropped",
                }
            } else {
                SourceRuntime {
                    configured: false,
                    status: "off",
                    status_label: "Needs a key",
                }
            }
        }
        "opensky" => {
            if *enabled.get("opensky_oauth").unwrap_or(&false) {
                SourceRuntime {
                    configured: true,
                    status: "oauth",
                    status_label: "OAuth",
                }
            } else {
                SourceRuntime {
                    configured: true,
                    status: "anonymous",
                    status_label: "Anonymous",
                }
            }
        }
        "aerodatabox" => {
            if *enabled.get("aerodatabox").unwrap_or(&false) {
                SourceRuntime {
                    configured: true,
                    status: "on",
                    status_label: "On",
                }
            } else {
                SourceRuntime {
                    configured: false,
                    status: "off",
                    status_label: "Needs a key",
                }
            }
        }
        _ => {
            let on = *enabled.get(id).unwrap_or(&default_on);
            if on {
                SourceRuntime {
                    configured: true,
                    status: "on",
                    status_label: "On",
                }
            } else {
                SourceRuntime {
                    configured: false,
                    status: "off",
                    status_label: "Needs a key",
                }
            }
        }
    }
}

struct SourceRuntime {
    configured: bool,
    status: &'static str,
    status_label: &'static str,
}

pub fn sources_payload(enabled: &HashMap<String, bool>) -> Vec<Value> {
    let mut out = Vec::new();
    for s in SOURCES.iter() {
        let mut item = serde_json::to_value(s).unwrap_or_else(|_| serde_json::json!({}));
        let default_on = s.id == "ourairports"
            || s.id == "geonames-countries"
            || s.id == "openflights-routes"
            || s.layer.ends_with("link")
            || matches!(
                s.layer.as_str(),
                "meta-search" | "ota" | "airline-direct" | "specialist-meta" | "live-track-link"
            );
        let runtime = source_runtime(&s.id, enabled, default_on);
        if let Some(obj) = item.as_object_mut() {
            obj.insert("configured".into(), Value::Bool(runtime.configured));
            obj.insert("status".into(), Value::String(runtime.status.into()));
            obj.insert(
                "status_label".into(),
                Value::String(runtime.status_label.into()),
            );
        }
        out.push(item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_source(payload: &[Value], id: &str) -> Value {
        payload
            .iter()
            .find(|s| s["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("missing source {id}"))
    }

    fn duffel_flags(live: bool, sandbox: bool) -> HashMap<String, bool> {
        HashMap::from([
            ("duffel".into(), live || sandbox),
            ("duffel_live".into(), live),
            ("duffel_sandbox".into(), sandbox),
            ("opensky_oauth".into(), false),
            ("aerodatabox".into(), false),
        ])
    }

    #[test]
    fn duffel_live_token_is_live_not_on() {
        let d = find_source(&sources_payload(&duffel_flags(true, false)), "duffel");
        assert_eq!(d["status"], "live");
        assert_eq!(d["status_label"], "Live");
        assert_eq!(d["configured"], true);
    }

    #[test]
    fn duffel_test_token_is_sandbox_not_on() {
        let d = find_source(&sources_payload(&duffel_flags(false, true)), "duffel");
        assert_eq!(d["status"], "sandbox");
        assert_eq!(d["status_label"], "Sandbox — offers dropped");
        assert_eq!(d["configured"], false);
        assert_ne!(d["status"], "on");
        assert_ne!(d["status_label"], "On");
    }

    #[test]
    fn duffel_missing_token_needs_a_key() {
        let d = find_source(&sources_payload(&duffel_flags(false, false)), "duffel");
        assert_eq!(d["status"], "off");
        assert_eq!(d["status_label"], "Needs a key");
        assert_eq!(d["configured"], false);
    }

    #[test]
    fn duffel_enabled_without_live_or_sandbox_flags_is_not_on() {
        let enabled = HashMap::from([("duffel".into(), true)]);
        let d = find_source(&sources_payload(&enabled), "duffel");
        assert_eq!(d["status"], "off");
        assert_eq!(d["status_label"], "Needs a key");
        assert_ne!(d["status"], "on");
    }

    #[test]
    fn opensky_anonymous_is_not_on() {
        let o = find_source(&sources_payload(&duffel_flags(false, false)), "opensky");
        assert_eq!(o["status"], "anonymous");
        assert_eq!(o["status_label"], "Anonymous");
        assert_eq!(o["configured"], true);
        assert_ne!(o["status"], "on");
    }

    #[test]
    fn opensky_oauth_is_not_on() {
        let mut flags = duffel_flags(false, false);
        flags.insert("opensky_oauth".into(), true);
        let o = find_source(&sources_payload(&flags), "opensky");
        assert_eq!(o["status"], "oauth");
        assert_eq!(o["status_label"], "OAuth");
        assert_eq!(o["configured"], true);
        assert_ne!(o["status"], "on");
    }
}
