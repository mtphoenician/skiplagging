use reqwest::header::HeaderValue;
use serde_json::{json, Value};
use std::time::Duration;

use crate::config::Settings;
use crate::engines::hidden::{itinerary_fingerprint, ticketed_destination};
use crate::models::{Offer, Segment, ShopRequest};

pub const DUFFEL_MAX_CONNECTIONS: i32 = 2;

pub fn offer_request_body(req: &ShopRequest) -> Value {
    let connections = if req.nonstop {
        0
    } else {
        DUFFEL_MAX_CONNECTIONS
    };
    let mut slices = vec![json!({
        "origin": req.origin,
        "destination": req.dest,
        "departure_date": req.date,
    })];
    if let Some(ret) = &req.return_date {
        slices.push(json!({
            "origin": req.dest,
            "destination": req.origin,
            "departure_date": ret,
        }));
    }
    let cabin = req.cabin.to_lowercase();
    json!({
        "data": {
            "slices": slices,
            "passengers": (0..req.adults.clamp(1, 9)).map(|_| json!({"type": "adult"})).collect::<Vec<_>>(),
            "cabin_class": cabin,
            "max_connections": connections,
        }
    })
}

pub struct DuffelProvider {
    auth: HeaderValue,
    client: reqwest::Client,
}

impl DuffelProvider {
    pub fn new(settings: &Settings, client: reqwest::Client) -> Self {
        let auth = HeaderValue::from_str(&format!("Bearer {}", settings.duffel_token))
            .unwrap_or_else(|_| HeaderValue::from_static("Bearer"));
        Self { auth, client }
    }

    pub async fn shop(&self, req: &ShopRequest) -> anyhow::Result<Vec<Offer>> {
        let payload = offer_request_body(req);
        let mut last: Option<reqwest::Response> = None;
        for _ in 0..3 {
            let r = self
                .client
                .post("https://api.duffel.com/air/offer_requests")
                .query(&[("return_offers", "true")])
                .header("Authorization", self.auth.clone())
                .header("Duffel-Version", "v2")
                .header("Accept", "application/json")
                .json(&payload)
                .timeout(Duration::from_secs(22))
                .send()
                .await?;
            if r.status().as_u16() != 429 {
                last = Some(r);
                break;
            }
            let wait = r
                .headers()
                .get("retry-after")
                .or_else(|| r.headers().get("ratelimit-reset"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(3.0)
                .clamp(1.5, 8.0);
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
        }
        let Some(r) = last else {
            anyhow::bail!("Duffel rate-limited offer_requests after retries");
        };
        let status = r.status().as_u16();
        if status >= 400 {
            let body = r.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(240).collect();
            anyhow::bail!("Duffel HTTP {status}: {snippet}");
        }
        let mut payload: Value = r
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Duffel JSON: {e}"))?;
        let raw_offers = match payload
            .get_mut("data")
            .and_then(|d| d.get_mut("offers"))
            .map(std::mem::take)
        {
            Some(Value::Array(v)) => v,
            _ => Vec::new(),
        };
        drop(payload);
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut offers = Vec::new();
        for raw in raw_offers {
            if let Some(parsed) = parse_one(&raw, req, &now) {
                offers.push(parsed);
            }
            if offers.len() >= req.max_offers as usize {
                break;
            }
        }
        Ok(offers)
    }

    pub async fn get_offer(
        &self,
        offer_id: &str,
        cabin: Option<&str>,
        currency: Option<&str>,
        adults: Option<i32>,
    ) -> anyhow::Result<Option<Offer>> {
        let oid = offer_id.strip_prefix("duffel-").unwrap_or(offer_id);
        if oid.is_empty() {
            return Ok(None);
        }
        let r = self
            .client
            .get(format!("https://api.duffel.com/air/offers/{oid}"))
            .header("Authorization", self.auth.clone())
            .header("Duffel-Version", "v2")
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        if r.status().as_u16() >= 400 {
            return Ok(None);
        }
        let mut body: Value = r.json().await.unwrap_or(json!({}));
        let raw = body
            .get_mut("data")
            .map(std::mem::take)
            .unwrap_or(json!({}));
        if raw.is_null() || raw.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            return Ok(None);
        }
        let dummy = ShopRequest {
            origin: "AAA".into(),
            dest: "BBB".into(),
            date: "2099-01-01".into(),
            adults: adults.unwrap_or(1).clamp(1, 9),
            cabin: cabin.unwrap_or("ECONOMY").into(),
            currency: currency.unwrap_or("USD").into(),
            ..ShopRequest::default()
        };
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        Ok(parse_one(&raw, &dummy, &now))
    }

    pub async fn refresh_offer(&self, offer: &Offer) -> anyhow::Result<Option<Offer>> {
        let fresh = self
            .get_offer(
                &offer.id,
                Some(&offer.cabin),
                Some("USD"),
                Some(offer.adults),
            )
            .await?;
        if let Some(fresh) = fresh {
            if fresh.live != Some(false) {
                return Ok(Some(fresh));
            }
        }
        if offer.segments.is_empty() {
            return Ok(None);
        }
        let first = &offer.segments[0];
        let date = if first.dep.len() >= 10 {
            first.dep[..10].to_string()
        } else {
            return Ok(None);
        };
        let outbound_n = offer
            .outbound_end
            .map(|n| n + 1)
            .unwrap_or(offer.segments.len() as i32);
        let req = ShopRequest {
            origin: first.origin.clone(),
            dest: {
                let t = ticketed_destination(offer);
                if t.is_empty() {
                    offer
                        .segments
                        .last()
                        .map(|s| s.dest.clone())
                        .unwrap_or_default()
                } else {
                    t
                }
            },
            date,
            adults: offer.adults.clamp(1, 9),
            cabin: offer.cabin.clone(),
            currency: "USD".into(),
            nonstop: outbound_n == 1,
            max_offers: 20,
            return_date: offer.return_date.clone(),
        };
        let want = itinerary_fingerprint(offer);
        for candidate in self.shop(&req).await? {
            if itinerary_fingerprint(&candidate) == want {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }
}

pub fn parse_one(raw: &Value, req: &ShopRequest, now: &str) -> Option<Offer> {
    _one(raw, req, now)
}

pub fn _one(raw: &Value, req: &ShopRequest, now: &str) -> Option<Offer> {
    let slices = raw.get("slices")?.as_array()?;
    if slices.is_empty() {
        return None;
    }
    let mut segments = Vec::new();
    let mut outbound_end = -1i32;
    let mut total_mins = 0i32;
    for (slice_i, sl) in slices.iter().enumerate() {
        let segs_raw = sl.get("segments")?.as_array()?;
        if segs_raw.is_empty() {
            return None;
        }
        for s in segs_raw {
            let origin = s
                .get("origin")
                .and_then(|o| o.get("iata_code"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let dest = s
                .get("destination")
                .and_then(|o| o.get("iata_code"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mkt = s.get("marketing_carrier");
            let op = s.get("operating_carrier");
            let mkt_code = mkt
                .and_then(|o| o.get("iata_code"))
                .and_then(|v| v.as_str())
                .unwrap_or("XX");
            let flight_n = s
                .get("marketing_carrier_flight_number")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            segments.push(Segment {
                origin,
                dest,
                carrier: mkt_code.to_string(),
                operating_carrier: op
                    .and_then(|o| o.get("iata_code"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                flight_number: format!("{mkt_code}{flight_n}"),
                dep: s
                    .get("departing_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                arr: s
                    .get("arriving_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                duration_min: dur(s.get("duration").and_then(|v| v.as_str())),
                rbd: String::new(),
                aircraft: s
                    .get("aircraft")
                    .and_then(|a| a.get("iata_code"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                fare_basis: None,
            });
        }
        if slice_i == 0 {
            outbound_end = (segments.len() - 1) as i32;
        }
        total_mins += dur(sl.get("duration").and_then(|v| v.as_str()));
    }
    if segments.is_empty() || outbound_end < 0 {
        return None;
    }
    let first_carrier = segments[0].carrier.clone();
    let first_flight = segments[0].flight_number.clone();
    let duration_min = if total_mins > 0 {
        total_mins
    } else {
        segments.iter().map(|s| s.duration_min).sum()
    };
    let owner = raw
        .get("owner")
        .and_then(|o| o.get("iata_code"))
        .and_then(|v| v.as_str())
        .unwrap_or(&first_carrier)
        .to_string();
    let live = raw.get("live_mode").and_then(|v| v.as_bool());
    let live_label = match live {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    };
    let adults = passenger_count(raw, req);
    let outbound_n = outbound_end + 1;
    let mut ret = None;
    if slices.len() > 1 {
        let inbound = &slices[1];
        ret = inbound
            .get("departure_date")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(10).collect::<String>())
            .filter(|s| !s.is_empty());
        if ret.is_none() {
            ret = inbound
                .get("segments")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|s| s.get("departing_at"))
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(10).collect::<String>())
                .filter(|s| !s.is_empty());
        }
        if ret.is_none() {
            ret = req.return_date.clone();
        }
    }
    Some(Offer {
        id: format!("duffel-{}", raw.get("id").and_then(|v| v.as_str()).unwrap_or("")),
        kind: if outbound_n == 1 {
            "nonstop".into()
        } else {
            "connecting".into()
        },
        channel: "ndc".into(),
        source: "duffel".into(),
        layer: "priced-offer".into(),
        segments,
        price: as_f(raw.get("total_amount")),
        base_price: as_f(raw.get("base_amount")),
        taxes: as_f(raw.get("tax_amount")),
        currency: raw
            .get("total_currency")
            .and_then(|v| v.as_str())
            .unwrap_or(&req.currency)
            .to_string(),
        quoted_usd: usd_quote(raw),
        cabin: req.cabin.clone(),
        adults,
        fare_basis: String::new(),
        validating_airline: Some(owner),
        carrier: first_carrier,
        duration_min,
        stops: (outbound_n - 1).max(0),
        first_flight,
        seats: offer_seats(raw),
        last_ticketing_date: payment_deadline(raw),
        retrieved_at: Some(now.to_string()),
        expires_at: raw.get("expires_at").and_then(|v| v.as_str()).map(|s| s.to_string()),
        live,
        return_date: ret,
        outbound_end: Some(outbound_end),
        note: Some(format!(
            "Duffel offer. live_mode={live_label}. Not an order. Creating an order would require passenger + payment and is disabled here.{}",
            if live == Some(false) {
                " Test tokens include Duffel Airways sandbox inventory."
            } else {
                ""
            }
        )),
        ..Offer::default()
    })
}

fn passenger_count(raw: &Value, req: &ShopRequest) -> i32 {
    let from_raw = raw
        .get("passengers")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as i32)
        .unwrap_or(0);
    if (1..=9).contains(&from_raw) {
        from_raw
    } else {
        req.adults.clamp(1, 9)
    }
}

fn offer_seats(raw: &Value) -> Option<i32> {
    for key in ["available_quantity", "number_of_bookable_seats"] {
        if let Some(n) = as_i(raw.get(key)) {
            if (1..10).contains(&n) {
                return Some(n);
            }
        }
    }
    None
}

fn payment_deadline(raw: &Value) -> Option<String> {
    let pr = raw.get("payment_requirements")?;
    pr.get("payment_required_by")
        .or_else(|| pr.get("price_guarantee_expires_at"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn as_i(v: Option<&Value>) -> Option<i32> {
    v.and_then(|v| match v {
        Value::Number(n) => n.as_i64().map(|n| n as i32).or_else(|| n.as_f64().map(|n| n as i32)),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

fn as_f(v: Option<&Value>) -> Option<f64> {
    v.and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

/// Prefer a USD total Duffel already put on the offer. `tax_amount` is not a fare.
pub fn _usd_quote(raw: &Value) -> Option<f64> {
    usd_quote(raw)
}

fn usd_quote(raw: &Value) -> Option<f64> {
    const PAIRS: &[(&str, &str)] = &[
        ("total_amount", "total_currency"),
        ("available_amount", "available_currency"),
        ("converted_amount", "converted_currency"),
    ];
    for (amt, ccy) in PAIRS {
        let c = raw.get(*ccy).and_then(|v| v.as_str()).unwrap_or("");
        if !c.eq_ignore_ascii_case("USD") {
            continue;
        }
        let v = as_f(raw.get(*amt))?;
        if v > 0.0 {
            return Some(v);
        }
    }
    None
}

pub fn dur(iso: Option<&str>) -> i32 {
    _dur(iso.unwrap_or(""))
}

pub fn _dur(iso: &str) -> i32 {
    parse_dur(iso).unwrap_or(0)
}

fn parse_int(s: &str) -> Option<i32> {
    if s.is_empty() {
        Some(0)
    } else {
        s.parse().ok()
    }
}

fn parse_dur(iso: &str) -> Option<i32> {
    if iso.is_empty() {
        return Some(0);
    }
    let raw = iso.trim().to_uppercase();
    if !raw.starts_with('P') {
        return Some(0);
    }
    let body = &raw[1..];
    let (date_part, time_part) = if let Some((d, t)) = body.split_once('T') {
        (d, t)
    } else {
        (body, "")
    };
    let days = if let Some((n, _)) = date_part.split_once('D') {
        parse_int(n)?
    } else {
        0
    };
    let mut rest = time_part.to_string();
    let mut hours = 0i32;
    let mut minutes = 0i32;
    let mut seconds = 0i32;
    if let Some((h, r)) = rest.split_once('H') {
        hours = parse_int(h)?;
        rest = r.to_string();
    }
    if let Some((m, r)) = rest.split_once('M') {
        minutes = parse_int(m)?;
        rest = r.to_string();
    }
    if let Some((s, _)) = rest.split_once('S') {
        seconds = parse_int(s)?;
    }
    Some(days * 1440 + hours * 60 + minutes + seconds / 60)
}
