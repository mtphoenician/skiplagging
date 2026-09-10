use chrono::{DateTime, Utc};

use crate::engines::expiry::parse_offer_time;
use crate::engines::hidden::itinerary_fingerprint;
use crate::models::{FareWindow, HiddenCityMatch, HiddenDeal, Offer, PricePoint};

const MAX_POINTS: usize = 24;

#[derive(Debug, Clone)]
pub struct FarePoint {
    pub fingerprint: String,
    pub origin: String,
    pub ticketed: String,
    pub date: String,
    pub price: f64,
    pub observed_at: DateTime<Utc>,
}

pub fn urgency_rank(urgency: &str) -> u8 {
    match urgency {
        "now" => 3,
        "soon" => 2,
        "watch" => 1,
        _ => 0,
    }
}

pub fn sort_matches(matches: &mut [HiddenCityMatch]) {
    matches.sort_by(|a, b| {
        let ua = a.window.as_ref().map(|w| urgency_rank(&w.urgency)).unwrap_or(0);
        let ub = b.window.as_ref().map(|w| urgency_rank(&w.urgency)).unwrap_or(0);
        ub.cmp(&ua)
            .then_with(|| {
                b.gross_saving
                    .partial_cmp(&a.gross_saving)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                b.risk
                    .net_saving_estimate
                    .partial_cmp(&a.risk.net_saving_estimate)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
}

pub fn sort_deals(deals: &mut [HiddenDeal]) {
    deals.sort_by(|a, b| {
        let ua = a.window.as_ref().map(|w| urgency_rank(&w.urgency)).unwrap_or(0);
        let ub = b.window.as_ref().map(|w| urgency_rank(&w.urgency)).unwrap_or(0);
        ub.cmp(&ua)
            .then_with(|| {
                b.saving
                    .partial_cmp(&a.saving)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                a.through_price
                    .partial_cmp(&b.through_price)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
}

pub fn attach_windows(matches: &mut [HiddenCityMatch], series: &[FarePoint], now: DateTime<Utc>) {
    for m in matches.iter_mut() {
        m.window = Some(window_for(
            &m.through_offer,
            m.through_offer.price,
            series,
            now,
        ));
    }
}

pub fn attach_deal_windows(deals: &mut [HiddenDeal], series: &[FarePoint], now: DateTime<Utc>) {
    for d in deals.iter_mut() {
        let Some(through) = d.through_offer.as_ref() else {
            d.window = Some(window_for_price(d.through_price, series_for_deal(d, series), now));
            continue;
        };
        d.window = Some(window_for(through, Some(d.through_price), series, now));
    }
}

pub fn window_for(
    through: &Offer,
    current: Option<f64>,
    series: &[FarePoint],
    now: DateTime<Utc>,
) -> FareWindow {
    let fp = itinerary_fingerprint(through);
    let origin = through
        .segments
        .first()
        .map(|s| s.origin.to_uppercase())
        .unwrap_or_default();
    let ticketed = through
        .segments
        .last()
        .map(|s| s.dest.to_uppercase())
        .unwrap_or_default();
    let date = through
        .segments
        .first()
        .and_then(|s| s.dep.get(..10))
        .unwrap_or("")
        .to_string();
    let picked = pick_series(&fp, &origin, &ticketed, &date, series);
    finish_window(through, current, picked, now)
}

fn window_for_price(current: f64, picked: Vec<FarePoint>, now: DateTime<Utc>) -> FareWindow {
    let offer = Offer {
        price: Some(current),
        currency: "USD".into(),
        ..Offer::default()
    };
    finish_window(&offer, Some(current), picked, now)
}

fn series_for_deal(deal: &HiddenDeal, series: &[FarePoint]) -> Vec<FarePoint> {
    pick_series(
        "",
        &deal.origin.to_uppercase(),
        &deal.hidden_city.to_uppercase(),
        &deal.date,
        series,
    )
}

fn pick_series(
    fingerprint: &str,
    origin: &str,
    ticketed: &str,
    date: &str,
    series: &[FarePoint],
) -> Vec<FarePoint> {
    let exact: Vec<FarePoint> = series
        .iter()
        .filter(|p| !fingerprint.is_empty() && p.fingerprint == fingerprint)
        .cloned()
        .collect();
    if exact.len() >= 2 {
        return exact;
    }
    let same_day: Vec<FarePoint> = series
        .iter()
        .filter(|p| p.origin == origin && p.ticketed == ticketed && p.date == date)
        .cloned()
        .collect();
    if same_day.len() >= 2 {
        return same_day;
    }
    if !exact.is_empty() {
        return exact;
    }
    if !same_day.is_empty() {
        return same_day;
    }
    series
        .iter()
        .filter(|p| p.origin == origin && p.ticketed == ticketed)
        .cloned()
        .collect()
}

fn finish_window(
    through: &Offer,
    current: Option<f64>,
    mut picked: Vec<FarePoint>,
    now: DateTime<Utc>,
) -> FareWindow {
    if let Some(price) = current.filter(|p| *p > 0.0) {
        let stale = picked.last().map(|p| {
            (now - p.observed_at).num_minutes().abs() > 20 || (p.price - price).abs() > 0.5
        });
        if stale.unwrap_or(true) {
            picked.push(FarePoint {
                fingerprint: String::new(),
                origin: String::new(),
                ticketed: String::new(),
                date: String::new(),
                price,
                observed_at: now,
            });
        }
    }
    picked.sort_by_key(|p| p.observed_at);
    let points = downsample(&picked);
    let (low, high, low_at) = extrema(&points);
    let trend = trend_of(&points);
    let seats = through.seats.filter(|n| *n > 0);
    let depart = parse_offer_time(through.segments.first().map(|s| s.dep.as_str()));
    let expire = soonest_deadline(through);
    let minutes_to_depart = depart.map(|d| (d - now).num_minutes());
    let minutes_to_expire = expire.filter(|d| *d > now).map(|d| (d - now).num_minutes());
    let (urgency, label, headline) = decide(
        seats,
        minutes_to_depart,
        minutes_to_expire,
        trend,
        current,
        low,
    );
    FareWindow {
        urgency,
        label,
        headline,
        minutes_to_depart: minutes_to_depart.filter(|m| *m > 0),
        minutes_to_expire,
        seats,
        trend: trend.to_string(),
        current,
        low,
        high,
        low_at,
        points,
    }
}

fn soonest_deadline(offer: &Offer) -> Option<DateTime<Utc>> {
    let exp = parse_offer_time(offer.expires_at.as_deref());
    let ticket = parse_offer_time(offer.last_ticketing_date.as_deref());
    match (exp, ticket) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn downsample(rows: &[FarePoint]) -> Vec<PricePoint> {
    if rows.is_empty() {
        return vec![];
    }
    if rows.len() <= MAX_POINTS {
        return rows.iter().map(to_point).collect();
    }
    let mut keep = vec![0usize];
    if let Some((i, _)) = rows
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.price.partial_cmp(&b.1.price).unwrap_or(std::cmp::Ordering::Equal))
    {
        keep.push(i);
    }
    let step = (rows.len() - 1) as f64 / (MAX_POINTS - 1) as f64;
    for n in 1..MAX_POINTS {
        keep.push(((n as f64 * step).round() as usize).min(rows.len() - 1));
    }
    keep.push(rows.len() - 1);
    keep.sort_unstable();
    keep.dedup();
    keep.into_iter().map(|i| to_point(&rows[i])).collect()
}

fn to_point(row: &FarePoint) -> PricePoint {
    PricePoint {
        t: row.observed_at.to_rfc3339(),
        price: row.price,
    }
}

fn extrema(points: &[PricePoint]) -> (Option<f64>, Option<f64>, Option<String>) {
    let mut low = None;
    let mut high = None;
    let mut low_at = None;
    for p in points {
        if low.map(|n| p.price < n).unwrap_or(true) {
            low = Some(p.price);
            low_at = Some(p.t.clone());
        }
        if high.map(|n| p.price > n).unwrap_or(true) {
            high = Some(p.price);
        }
    }
    (low, high, low_at)
}

fn trend_of(points: &[PricePoint]) -> &'static str {
    if points.len() < 2 {
        return "new";
    }
    let first = points[0].price;
    let last = points[points.len() - 1].price;
    if last > first * 1.04 {
        "rising"
    } else if last < first * 0.96 {
        "falling"
    } else {
        "stable"
    }
}

fn decide(
    seats: Option<i32>,
    depart: Option<i64>,
    expire: Option<i64>,
    trend: &str,
    current: Option<f64>,
    low: Option<f64>,
) -> (String, String, String) {
    let mut urgency = "open";
    if let Some(d) = depart {
        if d <= 18 * 60 {
            urgency = "now";
        } else if d <= 72 * 60 {
            urgency = "soon";
        } else if d <= 10 * 24 * 60 {
            urgency = "watch";
        }
    }
    if let Some(s) = seats {
        if s <= 2 {
            urgency = max_u(urgency, "now");
        } else if s <= 5 {
            urgency = max_u(urgency, "soon");
        }
    }
    if let Some(e) = expire {
        if e <= 20 {
            urgency = max_u(urgency, "now");
        } else if e <= 90 {
            urgency = max_u(urgency, "soon");
        }
    }
    if trend == "rising" {
        if let (Some(c), Some(l)) = (current, low) {
            if l > 0.0 && c >= l * 1.08 {
                urgency = max_u(urgency, bump(urgency));
            }
        }
    }

    let label = if seats.map(|s| s <= 2).unwrap_or(false) {
        format!("{} seats left", seats.unwrap())
    } else if expire.map(|e| e <= 20).unwrap_or(false) {
        format!("Quote ends in {}", human_mins(expire.unwrap()))
    } else if depart.map(|d| d <= 18 * 60).unwrap_or(false) {
        format!("Departs in {}", human_mins(depart.unwrap()))
    } else if seats.map(|s| s <= 5).unwrap_or(false) {
        format!("{} seats left", seats.unwrap())
    } else if expire.map(|e| e <= 90).unwrap_or(false) {
        format!("Quote live {}", human_mins(expire.unwrap()))
    } else if depart.map(|d| d <= 72 * 60).unwrap_or(false) {
        format!("Departs in {}", human_mins(depart.unwrap()))
    } else if urgency == "watch" {
        "This week".into()
    } else if trend == "rising" {
        "Price rising".into()
    } else {
        "Window open".into()
    };

    let headline = headline(seats, depart, expire, trend, current, low);
    (urgency.into(), label, headline)
}

fn max_u<'a>(a: &'a str, b: &'a str) -> &'a str {
    if urgency_rank(b) > urgency_rank(a) {
        b
    } else {
        a
    }
}

fn bump(urgency: &str) -> &'static str {
    match urgency {
        "open" => "watch",
        "watch" => "soon",
        _ => "now",
    }
}

fn headline(
    seats: Option<i32>,
    depart: Option<i64>,
    expire: Option<i64>,
    trend: &str,
    current: Option<f64>,
    low: Option<f64>,
) -> String {
    if expire.map(|e| e <= 20).unwrap_or(false) {
        return format!(
            "This quote dies in {}. Recheck, then buy on a booker — we never hold the seat.",
            human_mins(expire.unwrap())
        );
    }
    if let Some(s) = seats.filter(|s| *s <= 5) {
        return format!(
            "{s} seat{} left on the through ticket. Hidden-city ends when that class sells — the unused leg fills first.",
            if s == 1 { "" } else { "s" }
        );
    }
    if let Some(d) = depart {
        if d <= 18 * 60 {
            return format!(
                "Departs in {}. Cheap connecting buckets are usually gone this close in. If it is still inverted, it will not last.",
                human_mins(d)
            );
        }
        if d <= 72 * 60 {
            return format!(
                "Departs in {}. This is when A→C inventory tightens and the skiplag window closes.",
                human_mins(d)
            );
        }
        if d <= 10 * 24 * 60 {
            return "Typical hidden-city window. Recheck the morning you buy — one fill on the unused leg and the fare is gone.".into();
        }
        if d > 21 * 24 * 60 {
            return format!(
                "More than {} out. The inversion can vanish as the plane fills. Do not wait until the week of.",
                human_mins(d)
            );
        }
    }
    if trend == "rising" {
        if let (Some(c), Some(l)) = (current, low) {
            if l > 0.0 && c > l {
                let pct = ((c / l - 1.0) * 100.0).round();
                return format!(
                    "Up {pct:.0}% from the recent low. The unused segment is filling."
                );
            }
        }
        return "The through fare is rising. Book while it is still cheaper than flying only to your stop.".into();
    }
    if trend == "new" {
        return "First time we have priced this ticket. Recheck before you buy — inventory moves by the hour.".into();
    }
    "Still cheaper than the honest A→B ticket. Recheck before you buy; we do not hold inventory.".into()
}

pub fn human_mins(minutes: i64) -> String {
    let m = minutes.max(0);
    if m < 90 {
        format!("{m} min")
    } else if m < 36 * 60 {
        let h = ((m as f64) / 60.0).round() as i64;
        format!("{h}h")
    } else if m < 14 * 24 * 60 {
        let d = ((m as f64) / 1440.0).round() as i64;
        format!("{d}d")
    } else if m < 60 * 24 * 60 {
        let w = ((m as f64) / 10_080.0).round() as i64;
        format!("{w}wk")
    } else {
        let mo = ((m as f64) / 43_800.0).round().max(1.0) as i64;
        format!("{mo}mo")
    }
}
