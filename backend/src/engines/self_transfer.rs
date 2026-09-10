use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use std::collections::{HashMap, HashSet};

use crate::engines::hidden::{detect_hidden_city, is_standard_to, ticketed_destination, Intended};
use crate::models::{Offer, SeparateTicket};

pub const MIN_TRANSFER_MIN: i32 = 150;
pub const MAX_TRANSFER_MIN: i32 = 22 * 60;
pub const MAX_SELF_FLIGHTS: usize = 5;
pub const MAX_LEG_FLIGHTS: usize = 3;

pub fn next_day(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| {
            (d + chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| date.to_string())
}

fn parse_dt(raw: &str) -> Option<DateTime<Utc>> {
    if raw.is_empty() {
        return None;
    }
    let text = raw.replace('Z', "+00:00");
    DateTime::parse_from_rfc3339(&text)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M")
                .ok()
                .map(|n| n.and_utc())
        })
}

pub fn connection_minutes(arr: &str, dep: &str) -> Option<i32> {
    let a = parse_dt(arr)?;
    let b = parse_dt(dep)?;
    Some(((b - a).num_seconds() / 60) as i32)
}

pub fn bridge_pairs(
    hubs: &[String],
    from_origin: &HashSet<String>,
    into_dest: &HashSet<String>,
    limit: usize,
) -> Vec<(String, String)> {
    if limit == 0 {
        return vec![];
    }
    let chosen: Vec<String> = hubs
        .iter()
        .map(|h| h.to_uppercase())
        .filter(|h| h.len() == 3)
        .collect();
    let from_u: HashSet<String> = from_origin.iter().map(|c| c.to_uppercase()).collect();
    let into_u: HashSet<String> = into_dest.iter().map(|c| c.to_uppercase()).collect();
    if from_u.is_empty() || into_u.is_empty() {
        return vec![];
    }
    if chosen
        .iter()
        .any(|h| from_u.contains(h) && into_u.contains(h))
    {
        return vec![];
    }
    let lefts: Vec<String> = chosen
        .iter()
        .filter(|h| from_u.contains(*h))
        .cloned()
        .collect();
    let rights: Vec<String> = chosen
        .iter()
        .filter(|h| into_u.contains(*h))
        .cloned()
        .collect();
    let mut out = Vec::new();
    for h1 in &lefts {
        for h2 in &rights {
            if h1 != h2 {
                out.push((h1.clone(), h2.clone()));
            }
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

pub fn pick_transfer_hubs(
    origin: &str,
    dest: &str,
    from_origin: &HashSet<String>,
    into_dest: &HashSet<String>,
    limit: usize,
    extra_hubs: &[String],
    continents: &HashMap<String, String>,
) -> Vec<String> {
    if limit == 0 {
        return vec![];
    }
    let origin_u = origin.to_uppercase();
    let dest_u = dest.to_uppercase();
    let exclude: HashSet<String> = HashSet::from([origin_u.clone(), dest_u.clone()]);
    let from_u: HashSet<String> = from_origin
        .iter()
        .map(|c| c.to_uppercase())
        .filter(|c| !exclude.contains(c))
        .collect();
    let into_u: HashSet<String> = into_dest
        .iter()
        .map(|c| c.to_uppercase())
        .filter(|c| !exclude.contains(c))
        .collect();
    let extra: Vec<String> = extra_hubs
        .iter()
        .map(|h| h.to_uppercase())
        .filter(|h| h.len() == 3 && !exclude.contains(h))
        .collect();
    let extra_rank: HashMap<String, usize> = extra
        .iter()
        .enumerate()
        .map(|(i, h)| (h.clone(), i))
        .collect();
    let mut seen = HashSet::new();
    let mut ranked: Vec<(i32, usize, String)> = Vec::new();
    for hub in from_u.iter().chain(into_u.iter()).chain(extra.iter()) {
        if !seen.insert(hub.clone()) || hub.len() != 3 {
            continue;
        }
        let overlap = i32::from(from_u.contains(hub)) + i32::from(into_u.contains(hub));
        ranked.push((
            -overlap,
            extra_rank.get(hub).copied().unwrap_or(80),
            hub.clone(),
        ));
    }
    ranked.sort();
    let origin_c = continents.get(&origin_u).cloned().unwrap_or_default();
    let dest_c = continents.get(&dest_u).cloned().unwrap_or_default();
    let region_cap = if !origin_c.is_empty() && !dest_c.is_empty() && origin_c != dest_c {
        3
    } else {
        2
    };
    let mut out = Vec::new();
    let mut region_n: HashMap<String, usize> = HashMap::new();
    for (_, _, h) in &ranked {
        let region = continents.get(h).cloned().unwrap_or_else(|| h.clone());
        if region_n.get(&region).copied().unwrap_or(0) >= region_cap {
            continue;
        }
        *region_n.entry(region).or_insert(0) += 1;
        out.push(h.clone());
        if out.len() >= limit {
            return out;
        }
    }
    for (_, _, h) in &ranked {
        if !out.contains(h) {
            out.push(h.clone());
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn ticket_date(offer: &Offer) -> String {
    let dep = offer.segments.first().map(|s| s.dep.as_str()).unwrap_or("");
    if dep.len() >= 10 {
        dep[..10].to_string()
    } else {
        String::new()
    }
}

pub fn stitch_chain(parts: &[Offer], intended: impl Intended) -> Option<Offer> {
    if parts.len() < 2 {
        return None;
    }
    let dests = intended.codes();
    if parts
        .iter()
        .any(|p| p.price.map(|x| x <= 0.0).unwrap_or(true))
    {
        return None;
    }
    if parts.iter().any(|p| p.segments.is_empty()) {
        return None;
    }
    let ccy = parts[0].currency.to_uppercase();
    if ccy.is_empty() || parts.iter().any(|p| p.currency.to_uppercase() != ccy) {
        return None;
    }
    let cabin = &parts[0].cabin;
    if parts.iter().any(|p| &p.cabin != cabin) {
        return None;
    }
    if dests.contains(&parts[0].segments[0].origin.to_uppercase()) {
        return None;
    }
    let mut hubs = Vec::new();
    for pair in parts.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        let hub = ticketed_destination(left);
        if hub.is_empty() || dests.contains(&hub) {
            return None;
        }
        if right.segments[0].origin.to_uppercase() != hub {
            return None;
        }
        if !is_standard_to(left, hub.as_str()) {
            return None;
        }
        if detect_hidden_city(left, &dests).is_some() {
            return None;
        }
        let gap = connection_minutes(&left.segments.last()?.arr, &right.segments[0].dep)?;
        if gap < MIN_TRANSFER_MIN || gap > MAX_TRANSFER_MIN {
            return None;
        }
        hubs.push(hub);
    }
    let unique: HashSet<String> = hubs.iter().cloned().collect();
    if hubs.len() != unique.len() {
        return None;
    }
    let last = parts.last()?;
    if !is_standard_to(last, &dests) {
        return None;
    }
    if detect_hidden_city(last, &dests).is_some() {
        return None;
    }
    let segments: Vec<_> = parts.iter().flat_map(|p| p.segments.clone()).collect();
    if segments.len() > MAX_SELF_FLIGHTS {
        return None;
    }
    let first = &segments[0];
    let last_seg = segments.last()?;
    let door = connection_minutes(&first.dep, &last_seg.arr);
    let mut carriers = Vec::new();
    for p in parts {
        if !p.carrier.is_empty() && !carriers.contains(&p.carrier) {
            carriers.push(p.carrier.clone());
        }
    }
    let tickets: Vec<SeparateTicket> = parts
        .iter()
        .map(|p| SeparateTicket {
            origin: p.segments[0].origin.to_uppercase(),
            dest: ticketed_destination(p),
            date: ticket_date(p),
            price: p.price.unwrap_or(0.0),
            currency: ccy.clone(),
            carrier: p.carrier.clone(),
        })
        .collect();
    let bags = parts.iter().map(|p| p.bags_included).min().unwrap_or(0);
    Some(Offer {
        id: format!("self-{}", parts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join("+")),
        kind: "self-transfer".into(),
        channel: "ota".into(),
        source: "self-transfer".into(),
        layer: "priced-offer".into(),
        segments: segments.clone(),
        price: Some(
            ((parts.iter().map(|p| p.price.unwrap_or(0.0)).sum::<f64>()) * 100.0).round() / 100.0,
        ),
        currency: ccy,
        cabin: cabin.clone(),
        fare_basis: String::new(),
        bags_included: bags,
        carrier: carriers.first().cloned().unwrap_or_else(|| last.carrier.clone()),
        duration_min: door
            .filter(|d| *d > 0)
            .unwrap_or_else(|| segments.iter().map(|s| s.duration_min).sum()),
        stops: (segments.len().saturating_sub(1)) as i32,
        first_flight: first.flight_number.clone(),
        live: Some(parts.iter().all(|p| p.live != Some(false))),
        adults: parts.first().map(|p| p.adults.clamp(1, 9)).unwrap_or(1),
        self_transfer_airports: hubs.clone(),
        separate_tickets: tickets,
        note: Some(format!(
            "Self-transfer: {} separate tickets. You check in again at {}. A missed connection is not protected. This is not hidden-city.",
            parts.len(),
            hubs.join(", ")
        )),
        ..Offer::default()
    })
}

pub fn combine_at_hubs(
    inbound: &HashMap<String, Vec<Offer>>,
    outbound: &HashMap<String, Vec<Offer>>,
    intended: &HashSet<String>,
    mids: Option<&HashMap<(String, String), Vec<Offer>>>,
    limit: usize,
) -> Vec<Offer> {
    let mut out = Vec::new();
    let cheapest = |offers: &[Offer], n: usize| -> Vec<Offer> {
        let mut v = offers.to_vec();
        v.sort_by(|a, b| {
            a.stops
                .cmp(&b.stops)
                .then_with(|| {
                    a.price
                        .unwrap_or(1e12)
                        .partial_cmp(&b.price.unwrap_or(1e12))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.duration_min.cmp(&b.duration_min))
        });
        v.truncate(n);
        v
    };
    for (hub, lefts) in inbound {
        let rights = outbound.get(hub).cloned().unwrap_or_default();
        for left in cheapest(lefts, 4) {
            for right in cheapest(&rights, 4) {
                if let Some(joined) = stitch_chain(&[left.clone(), right], intended) {
                    out.push(joined);
                }
            }
        }
    }
    if let Some(mids) = mids {
        for ((h1, h2), middles) in mids {
            for left in cheapest(inbound.get(h1).map(|v| v.as_slice()).unwrap_or(&[]), 4) {
                for mid in cheapest(middles, 3) {
                    for right in cheapest(outbound.get(h2).map(|v| v.as_slice()).unwrap_or(&[]), 4)
                    {
                        if let Some(joined) =
                            stitch_chain(&[left.clone(), mid.clone(), right], intended)
                        {
                            out.push(joined);
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| {
        a.price
            .unwrap_or(1e12)
            .partial_cmp(&b.price.unwrap_or(1e12))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.segments.len().cmp(&b.segments.len()))
            .then_with(|| a.separate_tickets.len().cmp(&b.separate_tickets.len()))
            .then_with(|| a.duration_min.cmp(&b.duration_min))
    });
    let mut seen = HashSet::new();
    let mut uniq = Vec::new();
    for offer in out {
        let key: Vec<_> = offer
            .segments
            .iter()
            .map(|s| {
                (
                    s.origin.clone(),
                    s.dest.clone(),
                    s.flight_number.clone(),
                    s.dep.chars().take(16).collect::<String>(),
                )
            })
            .collect();
        if seen.insert(key) {
            uniq.push(offer);
        }
    }
    let twos: Vec<_> = uniq
        .iter()
        .filter(|o| o.separate_tickets.len() <= 2)
        .cloned()
        .collect();
    let mut threes: Vec<_> = uniq
        .iter()
        .filter(|o| o.separate_tickets.len() >= 3)
        .cloned()
        .collect();
    let best_two = twos
        .iter()
        .filter_map(|o| o.price)
        .fold(None, |acc, p| Some(acc.map(|a: f64| a.min(p)).unwrap_or(p)));
    if let Some(best_two) = best_two {
        threes.retain(|o| o.price.unwrap_or(1e12) < best_two);
    }
    let mut ranked = twos;
    ranked.extend(threes);
    ranked.sort_by(|a, b| {
        a.price
            .unwrap_or(1e12)
            .partial_cmp(&b.price.unwrap_or(1e12))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.segments.len().cmp(&b.segments.len()))
            .then_with(|| a.separate_tickets.len().cmp(&b.separate_tickets.len()))
            .then_with(|| a.duration_min.cmp(&b.duration_min))
    });
    ranked.truncate(limit);
    ranked
}
