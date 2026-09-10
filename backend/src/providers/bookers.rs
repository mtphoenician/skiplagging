use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use urlencoding::encode;

use crate::metros::METROS;
use crate::models::BookerLink;
use crate::providers::sandbox::is_test_carrier;

/// Confirm-link count on results. Google + Kayak + two market sites + Skyscanner.
pub const FEATURED_BOOKERS: usize = 5;

fn varint(n: i64) -> Vec<u8> {
    let mut n = n as u64;
    let mut out = Vec::new();
    loop {
        let mut bit = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            bit |= 0x80;
            out.push(bit);
        } else {
            out.push(bit);
            break;
        }
    }
    out
}

fn key(field: i32, wire: i32) -> Vec<u8> {
    varint(((field as i64) << 3) | wire as i64)
}

fn ld(field: i32, payload: &[u8]) -> Vec<u8> {
    let mut out = key(field, 2);
    out.extend(varint(payload.len() as i64));
    out.extend_from_slice(payload);
    out
}

fn var(field: i32, n: i64) -> Vec<u8> {
    let mut out = key(field, 0);
    out.extend(varint(n));
    out
}

fn str_field(field: i32, value: &str) -> Vec<u8> {
    ld(field, value.as_bytes())
}

fn place(code: &str) -> Vec<u8> {
    let mut out = var(1, 1);
    out.extend(str_field(2, &code.to_uppercase()));
    out
}

pub fn google_tfs(
    origin: &str,
    dest: &str,
    date: &str,
    adults: i32,
    cabin: i32,
    return_date: Option<&str>,
) -> String {
    let outbound = {
        let mut b = str_field(2, date);
        b.extend(ld(13, &place(origin)));
        b.extend(ld(14, &place(dest)));
        b
    };
    let mut body = Vec::new();
    body.extend(var(1, 28));
    body.extend(var(2, 2));
    body.extend(ld(3, &outbound));
    if let Some(ret) = return_date {
        let mut inbound = str_field(2, ret);
        inbound.extend(ld(13, &place(dest)));
        inbound.extend(ld(14, &place(origin)));
        body.extend(ld(3, &inbound));
    }
    for _ in 0..adults.max(1).min(9) {
        body.extend(var(8, 1));
    }
    body.extend(var(9, cabin as i64));
    body.extend(var(14, 1));
    body.extend(ld(16, &var(1, -1)));
    body.extend(var(19, 2));
    URL_SAFE_NO_PAD.encode(body)
}

fn google_cabin(cabin: &str) -> i32 {
    match cabin.to_uppercase().as_str() {
        "PREMIUM_ECONOMY" => 2,
        "BUSINESS" => 3,
        "FIRST" => 4,
        _ => 1,
    }
}

pub fn google_flights_url(
    origin: &str,
    dest: &str,
    date: &str,
    currency: &str,
    adults: i32,
    cabin: &str,
    return_date: Option<&str>,
) -> String {
    let o = origin.to_uppercase();
    let d = dest.to_uppercase();
    let ccy = if currency.is_empty() { "USD" } else { currency }.to_uppercase();
    let tfs = google_tfs(&o, &d, date, adults, google_cabin(cabin), return_date);
    format!("https://www.google.com/travel/flights/search?tfs={tfs}&hl=en&curr={ccy}")
}

fn booking_point(code: &str) -> String {
    if METROS.contains_key(code) {
        format!("{code}.CITY")
    } else {
        format!("{code}.AIRPORT")
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn ymd(date: &str) -> Option<(&str, usize, i32)> {
    let mut parts = date.split('-');
    let y = parts.next()?;
    let m: usize = parts.next()?.parse().ok()?;
    let d: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) {
        return None;
    }
    Some((y, m, d))
}

fn us(date: &str) -> String {
    let Some((y, m, d)) = ymd(date) else {
        return date.to_string();
    };
    format!("{m:02}/{d:02}/{y}")
}

fn dmon(date: &str) -> String {
    let Some((y, m, d)) = ymd(date) else {
        return date.to_string();
    };
    format!("{d:02}{}{y}", MONTHS[m - 1])
}

fn mmt(date: &str) -> String {
    let Some((y, m, d)) = ymd(date) else {
        return date.to_string();
    };
    let yy = if y.len() >= 2 { &y[y.len() - 2..] } else { y };
    format!("{d:02}{}{yy}", MONTHS[m - 1])
}

fn tvldt(date: &str) -> String {
    let Some((y, m, d)) = ymd(date) else {
        return date.to_string();
    };
    format!("{d:02}-{m:02}-{y}")
}

struct Meta {
    id: &'static str,
    name: &'static str,
    layer: &'static str,
    role: &'static str,
    issues: bool,
    tmpl: &'static str,
    /// Always in the featured five (Google, Kayak, Skyscanner).
    core: bool,
    /// Used when the city pair has no regional shop.
    global_fill: bool,
    /// ISO2 from the airport/country row. Only for shops that do not cover a whole continent.
    countries: &'static [&'static str],
    /// OurAirports continent codes from the airport/country row.
    continents: &'static [&'static str],
}

const META: &[Meta] = &[
    Meta { id: "google-flights", name: "Google Flights", layer: "meta-search", role: "Metasearch: compares and hands off. Does not issue the e-ticket.", issues: false, tmpl: "", core: true, global_fill: false, countries: &[], continents: &[] },
    Meta { id: "kayak", name: "Kayak", layer: "meta-search", role: "Metasearch. Booking completes on an airline or OTA, not on this link alone.", issues: false, tmpl: "https://www.kayak.com/flights/{o}-{d}/{kayak_dates}{kayak_adults}?sort=bestflight_a", core: true, global_fill: false, countries: &[], continents: &[] },
    Meta { id: "skyscanner", name: "Skyscanner", layer: "meta-search", role: "Metasearch. Date path is YYMMDD. Not the validating carrier.", issues: false, tmpl: "https://www.skyscanner.com/transport/flights/{ol}/{dl}/{sky_path}?adultsv2={adults}&cabinclass={sky_cabin}&rtn={sky_rtn}&preferdirects=false", core: true, global_fill: false, countries: &[], continents: &[] },
    Meta { id: "momondo", name: "Momondo", layer: "meta-search", role: "Kayak-family metasearch. Often wider international OTA coverage.", issues: false, tmpl: "https://www.momondo.com/flight-search/{o}-{d}/{kayak_dates}{kayak_adults}?sort=bestflight_a", core: false, global_fill: false, countries: &[], continents: &[] },
    Meta { id: "cheapflights", name: "Cheapflights", layer: "meta-search", role: "Kayak-family metasearch. US-facing comparison.", issues: false, tmpl: "https://www.cheapflights.com/flight-search/{o}-{d}/{kayak_dates}{kayak_adults}?sort=bestflight_a", core: false, global_fill: false, countries: &[], continents: &["NA"] },
    Meta { id: "wego", name: "Wego", layer: "meta-search", role: "Metasearch strong in the Middle East and Asia.", issues: false, tmpl: "https://www.wego.com/flights/searches/{o}-{d}-{wego_dates}/economy/{adults}a", core: false, global_fill: false, countries: &[], continents: &["AS", "AF"] },
    Meta { id: "expedia", name: "Expedia", layer: "ota", role: "OTA. Typically issues via GDS as merchant of record if you finish checkout there.", issues: true, tmpl: "https://www.expedia.com/Flights-Search?flight-type=on&mode=search&trip=oneway&leg1=from:{o},to:{d},departure:{us}TANYT&passengers=adults:{adults},children:0,infantinlap:N", core: false, global_fill: false, countries: &[], continents: &["NA", "OC"] },
    Meta { id: "booking-com", name: "Booking.com", layer: "ota", role: "OTA. Flight checkout on Booking.com can issue the ticket if you finish there.", issues: true, tmpl: "https://flights.booking.com/flights/{ob}-{db}/?type={book_type}&adults={adults}&cabinClass={book_cabin}&depart={date}{book_return}&from={o}&to={d}&sort=BEST", core: false, global_fill: true, countries: &[], continents: &[] },
    Meta { id: "trip-com", name: "Trip.com", layer: "ota", role: "OTA. Trip.com is often merchant of record if you finish checkout there.", issues: true, tmpl: "https://www.trip.com/flights/{ol}-to-{dl}/?dcity={o}&acity={d}&ddate={date}{trip_adate}&flighttype={flighttype}&class=ys&quantity={adults}", core: false, global_fill: false, countries: &["CN", "HK", "TW", "MO", "JP", "KR"], continents: &[] },
    Meta { id: "priceline", name: "Priceline", layer: "ota", role: "OTA. Booking Holdings shop. Can issue if you finish checkout there.", issues: true, tmpl: "https://www.priceline.com/m/fly/search/{priceline_path}/?cabin-class=ECO&no-of-adults={adults}", core: false, global_fill: false, countries: &[], continents: &[] },
    Meta { id: "kiwi", name: "Kiwi.com", layer: "ota", role: "OTA. Virtual interlining / self-transfer specialist. Issues if you finish there.", issues: true, tmpl: "https://www.kiwi.com/en/search/results/{ol}/{dl}/{date}/{kiwi_ret}?adults={adults}", core: false, global_fill: false, countries: &[], continents: &[] },
    Meta { id: "cheapoair", name: "CheapOair", layer: "ota", role: "US OTA. Can issue if you finish checkout there.", issues: true, tmpl: "https://www.cheapoair.com/flights/results?from={o}&to={d}&fromDt={us}{cheapo_ret}&tripType={cheapo_trip}&adults={adults}", core: false, global_fill: false, countries: &[], continents: &[] },
    Meta { id: "edreams", name: "eDreams", layer: "ota", role: "European OTA. Can issue if you finish checkout there.", issues: true, tmpl: "https://www.edreams.com/flights/{edreams_path}", core: false, global_fill: false, countries: &["TR"], continents: &["EU"] },
    Meta { id: "traveloka", name: "Traveloka", layer: "ota", role: "Southeast Asia OTA. Can issue if you finish checkout there.", issues: true, tmpl: "https://www.traveloka.com/en-en/flight/fullsearch?ap={o}.{d}&dt={tvldt_pair}&ps={adults}.0.0", core: false, global_fill: false, countries: &["TH", "VN", "ID", "MY", "SG", "PH", "KH", "LA", "MM", "BN"], continents: &[] },
    Meta { id: "makemytrip", name: "MakeMyTrip", layer: "ota", role: "India OTA. Can issue if you finish checkout there.", issues: true, tmpl: "https://www.makemytrip.com/flight/search?itinerary={mmt_itin}&tripType={mmt_trip}&paxType=A-{adults}_C-0_I-0&cabinClass=E", core: false, global_fill: false, countries: &["IN"], continents: &[] },
    Meta { id: "despegar", name: "Despegar", layer: "ota", role: "Latin America OTA. Can issue if you finish checkout there.", issues: true, tmpl: "https://www.despegar.com/shop/flights/results/{despegar_kind}/{o}/{d}/{date}{despegar_ret}/{adults}/0/0", core: false, global_fill: false, countries: &["MX"], continents: &["SA"] },
    Meta { id: "skiplagged-com", name: "Skiplagged.com", layer: "specialist-meta", role: "Specialist hidden-city search. Independent of this repo.", issues: false, tmpl: "https://skiplagged.com/flights/{o}/{d}/{date}", core: false, global_fill: false, countries: &[], continents: &[] },
];

fn subst(tmpl: &str, ctx: &std::collections::HashMap<&str, String>) -> String {
    let mut out = tmpl.to_string();
    // longest keys first so {kayak_dates} beats {date}
    let mut keys: Vec<_> = ctx.keys().copied().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for k in keys {
        out = out.replace(&format!("{{{k}}}"), &ctx[k]);
    }
    out
}

pub fn booker_links(
    origin: &str,
    dest: &str,
    date: &str,
    adults: i32,
    airlines: &[(String, String)],
    currency: &str,
    cabin: &str,
    return_date: Option<&str>,
) -> Vec<BookerLink> {
    let o = origin.to_uppercase();
    let d = dest.to_uppercase();
    let ccy = if currency.is_empty() { "USD" } else { currency }.to_uppercase();
    let cabin_u = if cabin.is_empty() { "ECONOMY" } else { cabin }.to_uppercase();
    if date.len() < 10 {
        return vec![];
    }
    let ret = return_date.unwrap_or("");
    let yymmdd = date[2..].replace('-', "");
    let ret_yymmdd = if ret.len() >= 10 {
        ret[2..].replace('-', "")
    } else {
        String::new()
    };
    let kayak_adults = if adults <= 1 {
        String::new()
    } else {
        format!("/{adults}adults")
    };
    let kayak_dates = if !ret.is_empty() {
        format!("{date}/{ret}")
    } else {
        date.to_string()
    };
    let sky_path = if !ret.is_empty() {
        format!("{yymmdd}/{ret_yymmdd}/")
    } else {
        format!("{yymmdd}/")
    };
    let wego_dates = if !ret.is_empty() {
        format!("{}:{ret}", dmon(date))
    } else {
        format!("{}:ow", dmon(date))
    };
    let priceline_path = if !ret.is_empty() {
        format!(
            "{}-{}-{}/{}-{}-{}",
            o,
            d,
            date.replace('-', ""),
            d,
            o,
            ret.replace('-', "")
        )
    } else {
        format!("{}-{}-{}", o, d, date.replace('-', ""))
    };
    let edreams_path = if !ret.is_empty() {
        format!(
            "{}-{}/{}/{}/{adults}-0-0/",
            o.to_lowercase(),
            d.to_lowercase(),
            date,
            ret
        )
    } else {
        format!(
            "{}-{}/{}/{adults}-0-0/",
            o.to_lowercase(),
            d.to_lowercase(),
            date
        )
    };
    let tvldt_pair = if !ret.is_empty() {
        format!("{}.{}", tvldt(date), tvldt(ret))
    } else {
        format!("{}.NA", tvldt(date))
    };
    let mmt_itin = if !ret.is_empty() {
        format!("{}-{}-{}_{}-{}-{}", o, d, mmt(date), d, o, mmt(ret))
    } else {
        format!("{}-{}-{}", o, d, mmt(date))
    };
    let sky_cabin = match cabin_u.as_str() {
        "PREMIUM_ECONOMY" => "premiumeconomy",
        "BUSINESS" => "business",
        "FIRST" => "first",
        _ => "economy",
    };
    let q = encode(&format!(
        "{} flights from {o} to {d} on {date}{}",
        if !ret.is_empty() {
            "round trip"
        } else {
            "one way"
        },
        if !ret.is_empty() {
            format!(" returning {ret}")
        } else {
            String::new()
        }
    ))
    .into_owned();
    let ctx = std::collections::HashMap::from([
        ("o", o.clone()),
        ("d", d.clone()),
        ("ol", o.to_lowercase()),
        ("dl", d.to_lowercase()),
        ("ob", booking_point(&o)),
        ("db", booking_point(&d)),
        ("date", date.to_string()),
        ("ret", ret.to_string()),
        ("yymmdd", yymmdd),
        ("ret_yymmdd", ret_yymmdd),
        ("ymd", date.replace('-', "")),
        ("ret_ymd", ret.replace('-', "")),
        ("dmon", dmon(date)),
        ("mmt", mmt(date)),
        ("tvldt", tvldt(date)),
        ("adults", adults.to_string()),
        ("kayak_adults", kayak_adults),
        ("kayak_dates", kayak_dates),
        ("sky_rtn", if !ret.is_empty() { "1" } else { "0" }.into()),
        ("sky_path", sky_path),
        (
            "trip",
            if !ret.is_empty() {
                "roundtrip"
            } else {
                "oneway"
            }
            .into(),
        ),
        ("wego_dates", wego_dates),
        (
            "book_type",
            if !ret.is_empty() {
                "ROUNDTRIP"
            } else {
                "ONEWAY"
            }
            .into(),
        ),
        (
            "book_return",
            if !ret.is_empty() {
                format!("&return={ret}")
            } else {
                String::new()
            },
        ),
        (
            "trip_adate",
            if !ret.is_empty() {
                format!("&adate={ret}")
            } else {
                String::new()
            },
        ),
        ("priceline_path", priceline_path),
        (
            "cheapo_trip",
            if !ret.is_empty() {
                "ROUNDTRIP"
            } else {
                "ONEWAY"
            }
            .into(),
        ),
        (
            "cheapo_ret",
            if !ret.is_empty() {
                format!("&toDt={}", us(ret))
            } else {
                String::new()
            },
        ),
        ("edreams_path", edreams_path),
        ("tvldt_pair", tvldt_pair),
        ("mmt_itin", mmt_itin),
        ("mmt_trip", if !ret.is_empty() { "R" } else { "O" }.into()),
        (
            "despegar_kind",
            if !ret.is_empty() {
                "roundtrip"
            } else {
                "oneway"
            }
            .into(),
        ),
        (
            "despegar_ret",
            if !ret.is_empty() {
                format!("/{ret}")
            } else {
                String::new()
            },
        ),
        (
            "kiwi_ret",
            if !ret.is_empty() {
                ret.to_string()
            } else {
                "no-return".into()
            },
        ),
        (
            "flighttype",
            if !ret.is_empty() { "rt" } else { "ow" }.into(),
        ),
        ("sky_cabin", sky_cabin.into()),
        ("book_cabin", cabin_u.clone()),
        ("us", us(date)),
        (
            "ret_us",
            if !ret.is_empty() {
                us(ret)
            } else {
                String::new()
            },
        ),
        ("q", q),
    ]);
    let mut links = Vec::new();
    for m in META {
        let url = if m.id == "google-flights" {
            google_flights_url(&o, &d, date, &ccy, adults, &cabin_u, return_date)
        } else if m.id == "expedia" && !ret.is_empty() {
            format!(
                "https://www.expedia.com/Flights-Search?flight-type=on&mode=search&trip=roundtrip&leg1=from:{o},to:{d},departure:{}TANYT&leg2=from:{d},to:{o},departure:{}TANYT&passengers=adults:{adults},children:0,infantinlap:N",
                ctx["us"], ctx["ret_us"]
            )
        } else {
            subst(m.tmpl, &ctx)
        };
        links.push(BookerLink {
            id: m.id.into(),
            name: m.name.into(),
            layer: m.layer.into(),
            role: m.role.into(),
            url,
            issues_ticket: m.issues,
        });
    }
    let google = google_flights_url(&o, &d, date, &ccy, adults, &cabin_u, return_date);
    for (iata, name) in airlines {
        let iata = iata.to_uppercase();
        if iata.len() < 2 || is_test_carrier(&iata) {
            continue;
        }
        links.push(BookerLink {
            id: format!("carrier-{iata}"),
            name: format!("{name} ({iata})"),
            layer: "airline-direct".into(),
            role: "Carrier name comes from the airline table. Link is a metasearch prefilter, not a PNR on the airline host.".into(),
            url: google.clone(),
            issues_ticket: false,
        });
    }
    links
}

pub fn airport_geo(ap: &crate::models::Airport) -> (&str, &str) {
    (ap.continent.as_str(), ap.country.as_str())
}

/// Shops that fit this airport row: country first (slice of a continent), then continent, then global.
fn matching_ids(continent: &str, country: &str) -> Vec<&'static str> {
    let ctry = country.trim().to_ascii_uppercase();
    let cont = continent.trim().to_ascii_uppercase();
    let mut ids = Vec::new();
    let mut push = |id: &'static str| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    for m in META {
        if m.core {
            continue;
        }
        if !ctry.is_empty() && m.countries.iter().any(|c| c.eq_ignore_ascii_case(&ctry)) {
            push(m.id);
        }
    }
    for m in META {
        if m.core {
            continue;
        }
        if !cont.is_empty() && m.continents.iter().any(|c| c.eq_ignore_ascii_case(&cont)) {
            push(m.id);
        }
    }
    for m in META {
        if m.core || !m.global_fill {
            continue;
        }
        push(m.id);
    }
    ids
}

fn pick_market_ids(
    origin_continent: &str,
    origin_country: &str,
    dest_continent: &str,
    dest_country: &str,
) -> [&'static str; 2] {
    let o = matching_ids(origin_continent, origin_country);
    let d = matching_ids(dest_continent, dest_country);
    let o0 = o.first().copied().unwrap_or("booking-com");
    let d0 = d.first().copied().unwrap_or("booking-com");
    if o0 != d0 {
        return [o0, d0];
    }
    let second = o
        .get(1)
        .copied()
        .or_else(|| d.get(1).copied())
        .or_else(|| {
            META.iter()
                .find(|m| m.global_fill && m.id != o0)
                .map(|m| m.id)
        })
        .unwrap_or("expedia");
    [o0, second]
}

/// Five confirm links: Google, Kayak, origin-market, dest-market, Skyscanner.
/// Market slots are where people in that region actually shop — not a price rank.
pub fn featured_bookers(
    links: Vec<BookerLink>,
    origin_continent: &str,
    origin_country: &str,
    dest_continent: &str,
    dest_country: &str,
    n: usize,
) -> Vec<BookerLink> {
    let cap = n.max(1);
    let mut by_id: std::collections::HashMap<String, BookerLink> = std::collections::HashMap::new();
    let mut roster: Vec<String> = Vec::new();
    for b in links {
        if b.id.starts_with("carrier-") || b.id == "skiplagged-com" {
            continue;
        }
        if by_id.contains_key(&b.id) {
            continue;
        }
        roster.push(b.id.clone());
        by_id.insert(b.id.clone(), b);
    }

    let market = pick_market_ids(
        origin_continent,
        origin_country,
        dest_continent,
        dest_country,
    );
    let preferred = [
        "google-flights",
        "kayak",
        market[0],
        market[1],
        "skyscanner",
    ];
    let fill: Vec<&str> = matching_ids(origin_continent, origin_country)
        .into_iter()
        .chain(matching_ids(dest_continent, dest_country))
        .collect();

    let kayak_family = ["momondo", "cheapflights"];
    let mut used_family = false;
    let mut out = Vec::new();
    let mut take = |id: &str| {
        if out.len() >= cap || !by_id.contains_key(id) {
            return;
        }
        if out.iter().any(|b: &BookerLink| b.id == id) {
            return;
        }
        if kayak_family.contains(&id) {
            if used_family {
                return;
            }
            used_family = true;
        }
        if let Some(b) = by_id.remove(id) {
            out.push(b);
        }
    };
    for id in preferred {
        take(id);
    }
    for id in fill {
        take(id);
    }
    for id in roster {
        take(&id);
    }
    out
}

/// Full roster, then the five links that belong on this city pair.
pub fn featured_booker_links(
    origin: &str,
    dest: &str,
    date: &str,
    adults: i32,
    currency: &str,
    cabin: &str,
    return_date: Option<&str>,
    origin_geo: Option<(&str, &str)>,
    dest_geo: Option<(&str, &str)>,
) -> Vec<BookerLink> {
    let links = booker_links(
        origin,
        dest,
        date,
        adults,
        &[],
        currency,
        cabin,
        return_date,
    );
    let (oc, oiso) = origin_geo.unwrap_or(("", ""));
    let (dc, diso) = dest_geo.unwrap_or(("", ""));
    featured_bookers(links, oc, oiso, dc, diso, FEATURED_BOOKERS)
}
