use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Value};
use sqlx::types::Json;
use sqlx::{FromRow, PgPool, Row};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use crate::budget::provider_score;
use crate::engines::candidates::{score_candidate, RouteStat};
use crate::engines::expiry::{offer_unexpired, unexpired};
use crate::engines::hidden::{hidden_city_savings, meaningful_saving, HIDDEN_CITY_WARNINGS};
use crate::engines::index::observation_meta;
use crate::engines::learn::{merge_savings, summarize_savings, LearnBatch};
use crate::engines::risk::assess;
use crate::fx::{offer_in_usd, offers_in_usd, to_usd};
use crate::metros::{catalog_code_for_member, normalize_place_id, METROS};
use crate::models::{
    Airport, BookerLink, Country, HiddenCityMatch, HiddenDeal, Navaid, Offer, Region, Runway,
};
use crate::providers::bookers::booker_links;
use crate::providers::sandbox::is_live_fare;

static CONTINENTS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9]+").expect("token regex"));

const TYPE_W: &[(&str, i32)] = &[
    ("large_airport", 45),
    ("medium_airport", 18),
    ("small_airport", 0),
    ("seaplane_base", -10),
    ("heliport", -35),
    ("balloonport", -40),
    ("closed", -80),
];

fn type_weight(t: &str) -> i32 {
    TYPE_W
        .iter()
        .find(|(k, _)| *k == t)
        .map(|(_, v)| *v)
        .unwrap_or(-15)
}

fn iata3(s: &str) -> String {
    s.to_uppercase().chars().take(3).collect()
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn json_strings(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
        _ => vec![],
    }
}

fn json_f64s(v: &Value) -> Vec<f64> {
    match v {
        Value::Array(a) => a.iter().filter_map(|x| x.as_f64()).collect(),
        _ => vec![],
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct AirportRow {
    pub iata: String,
    pub icao: Option<String>,
    pub ident: Option<String>,
    pub name: String,
    pub municipality: String,
    pub iso_country: String,
    pub iso_region: String,
    pub continent: String,
    pub country_name: String,
    pub region_name: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: Option<i32>,
    #[sqlx(rename = "type")]
    pub type_: String,
    pub scheduled_service: bool,
    pub wikipedia: Option<String>,
    pub home_link: Option<String>,
    pub gps_code: Option<String>,
    pub local_code: Option<String>,
    pub keywords: Option<String>,
    pub source: String,
    pub ingested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
struct ContinentRow {
    code: String,
    name: String,
    source: String,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
struct CountryRow {
    iso2: String,
    iso3: Option<String>,
    iso_numeric: Option<String>,
    name: String,
    continent: String,
    capital: String,
    currency_code: String,
    currency_name: String,
    tld: String,
    phone: String,
    languages: String,
    population: Option<i32>,
    area_km2: Option<f64>,
    geoname_id: Option<i32>,
    wikipedia: Option<String>,
    sources: String,
    ingested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
struct RegionRow {
    code: String,
    local_code: String,
    name: String,
    iso_country: String,
    continent: String,
    wikipedia: Option<String>,
    source: String,
}

#[derive(Debug, Clone, FromRow)]
struct RunwayDb {
    id: i32,
    length_ft: Option<i32>,
    width_ft: Option<i32>,
    surface: String,
    lighted: bool,
    closed: bool,
    le_ident: String,
    he_ident: String,
}

#[derive(Debug, Clone, FromRow)]
struct NavaidDb {
    ident: String,
    name: String,
    #[sqlx(rename = "type")]
    type_: String,
    frequency_khz: Option<i32>,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct HiddenDealRow {
    pub id: i32,
    pub origin: String,
    pub destination: String,
    pub hidden_city: String,
    pub origin_city: String,
    pub dest_city: String,
    pub hidden_city_name: String,
    pub date: String,
    pub honest_price: f64,
    pub through_price: f64,
    pub currency: String,
    pub saving: f64,
    pub saving_pct: f64,
    pub first_flight: String,
    pub source: String,
    pub bookers: Json<Value>,
    pub local_payload: Json<Value>,
    pub through_payload: Json<Value>,
    pub retrieved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
struct OfferObsRow {
    fingerprint: String,
    payload: Json<Value>,
}

#[derive(Debug, Clone, FromRow)]
struct RouteStatRow {
    origin: String,
    intended: String,
    ticketed: String,
    observations: i32,
    successful_connections: i32,
    cheaper_than_direct_count: i32,
    success_rate: f64,
    average_saving: f64,
    median_saving: f64,
    maximum_saving: f64,
    average_saving_percent: f64,
    savings: Json<Value>,
    currency: String,
    best_through_price: Option<f64>,
    score: f64,
    last_success_at: Option<DateTime<Utc>>,
    last_cheaper_at: Option<DateTime<Utc>>,
    last_checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
struct RouteEdgeRow {
    origin: String,
    dest: String,
    carrier: String,
    flight_number: String,
    observation_count: i32,
    days_seen: i32,
    travel_dates: Json<Value>,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
}

pub async fn continent_names(pool: &PgPool) -> anyhow::Result<HashMap<String, String>> {
    {
        let guard = crate::mutex_lock(&CONTINENTS);
        if let Some(map) = guard.as_ref() {
            return Ok(map.clone());
        }
    }
    let rows: Vec<ContinentRow> = sqlx::query_as("SELECT code, name, source FROM continents")
        .fetch_all(pool)
        .await?;
    let map: HashMap<String, String> = rows.into_iter().map(|r| (r.code, r.name)).collect();
    *crate::mutex_lock(&CONTINENTS) = Some(map.clone());
    Ok(map)
}

pub fn to_airport(
    row: &AirportRow,
    currency: &str,
    continents: &HashMap<String, String>,
) -> Airport {
    Airport {
        iata: row.iata.clone(),
        place_id: row.iata.clone(),
        icao: row.icao.clone(),
        ident: row.ident.clone(),
        name: row.name.clone(),
        city: city_label(&row.municipality, &row.name),
        country: row.iso_country.clone(),
        country_name: if row.country_name.is_empty() {
            row.iso_country.clone()
        } else {
            row.country_name.clone()
        },
        region_code: row.iso_region.clone(),
        region_name: row.region_name.clone(),
        continent: row.continent.clone(),
        continent_name: continents
            .get(&row.continent)
            .cloned()
            .unwrap_or_else(|| row.continent.clone()),
        currency_code: currency.to_string(),
        lat: row.lat,
        lon: row.lon,
        metro: if row.municipality.is_empty() {
            row.iata.clone()
        } else {
            row.municipality.clone()
        },
        type_: row.type_.clone(),
        scheduled_service: row.scheduled_service,
        elevation_ft: row.elevation_ft,
        wikipedia: row.wikipedia.clone(),
        source: "ourairports".into(),
        hub_carriers: vec![],
        members: vec![],
        runways: vec![],
        navaids: vec![],
    }
}

pub async fn airports_by_iata(
    pool: &PgPool,
    codes: &[String],
) -> anyhow::Result<HashMap<String, Airport>> {
    let clean: Vec<String> = codes
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| iata3(c))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if clean.is_empty() {
        return Ok(HashMap::new());
    }
    let names = continent_names(pool).await?;
    let rows: Vec<AirportRow> =
        sqlx::query_as("SELECT * FROM airports WHERE iata = ANY($1::text[])")
            .bind(&clean)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .iter()
        .map(|r| (r.iata.clone(), to_airport(r, "", &names)))
        .collect())
}

async fn fetch_airport(pool: &PgPool, iata: &str) -> anyhow::Result<Option<AirportRow>> {
    Ok(sqlx::query_as("SELECT * FROM airports WHERE iata = $1")
        .bind(iata.to_uppercase())
        .fetch_optional(pool)
        .await?)
}

async fn fetch_country(pool: &PgPool, iso2: &str) -> anyhow::Result<Option<CountryRow>> {
    Ok(sqlx::query_as("SELECT * FROM countries WHERE iso2 = $1")
        .bind(iso2.to_uppercase())
        .fetch_optional(pool)
        .await?)
}

pub async fn get_airport(
    pool: &PgPool,
    iata: &str,
    detail: bool,
) -> anyhow::Result<Option<Airport>> {
    let Some(row) = fetch_airport(pool, iata).await? else {
        return Ok(None);
    };
    let mut currency = String::new();
    if !row.iso_country.is_empty() {
        if let Some(country) = fetch_country(pool, &row.iso_country).await? {
            currency = country.currency_code;
        }
    }
    let names = continent_names(pool).await?;
    let mut ap = to_airport(&row, &currency, &names);
    if detail {
        ap.runways = list_runways(pool, &row.iata).await?;
        ap.navaids = list_navaids(pool, row.ident.as_deref()).await?;
    }
    Ok(Some(ap))
}

pub async fn get_place(pool: &PgPool, code: &str, detail: bool) -> anyhow::Result<Option<Airport>> {
    let pid = normalize_place_id(code);
    if let Some(rest) = pid.strip_prefix("CITY-") {
        return get_metro(pool, rest).await;
    }
    if let Some(metro) = get_metro(pool, &pid).await? {
        if metro.place_id == pid {
            return Ok(Some(metro));
        }
    }
    get_airport(pool, &pid, detail).await
}

pub async fn get_metro(pool: &PgPool, code: &str) -> anyhow::Result<Option<Airport>> {
    let code = code.to_uppercase();
    let names = continent_names(pool).await?;
    if let Some((city, iso2, members, _aliases)) = METROS.get(code.as_str()) {
        let rows = existing_airports(pool, members).await?;
        if rows.len() < 2 {
            return Ok(None);
        }
        let country = fetch_country(pool, iso2).await?;
        return Ok(Some(metro_place(
            &code,
            city,
            iso2,
            &rows,
            &names,
            country.as_ref(),
            false,
        )));
    }
    let Some(row) = fetch_airport(pool, &code).await? else {
        return Ok(None);
    };
    if !row.scheduled_service {
        return Ok(None);
    }
    let cluster = city_cluster(pool, &row).await?;
    if cluster.len() < 2 {
        return Ok(None);
    }
    if let Some(catalog) = catalog_code_for_member(&row.iata) {
        return Box::pin(get_metro(pool, catalog)).await;
    }
    let country = fetch_country(pool, &cluster[0].iso_country).await?;
    Ok(Some(metro_place(
        &cluster[0].iata,
        &city_label(&cluster[0].municipality, &cluster[0].name),
        &cluster[0].iso_country,
        &cluster,
        &names,
        country.as_ref(),
        true,
    )))
}

async fn existing_airports(pool: &PgPool, codes: &[&str]) -> anyhow::Result<Vec<AirportRow>> {
    let want: Vec<String> = codes.iter().map(|c| c.to_uppercase()).collect();
    if want.is_empty() {
        return Ok(vec![]);
    }
    let rows: Vec<AirportRow> = sqlx::query_as(
        "SELECT * FROM airports WHERE iata = ANY($1::text[]) AND scheduled_service = TRUE",
    )
    .bind(&want)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn city_cluster(pool: &PgPool, seed: &AirportRow) -> anyhow::Result<Vec<AirportRow>> {
    let city = fold(&city_label(&seed.municipality, &seed.name));
    let near = nearby_airports(pool, &seed.iata, 80.0).await?;
    let mut rows = vec![seed.clone()];
    let mut have = HashSet::from([seed.iata.clone()]);
    for ap in near {
        if have.contains(&ap.iata) {
            continue;
        }
        if !matches!(ap.type_.as_str(), "large_airport" | "medium_airport") || !ap.scheduled_service
        {
            continue;
        }
        let Some(raw) = fetch_airport(pool, &ap.iata).await? else {
            continue;
        };
        let same_city = fold(&city_label(&raw.municipality, &raw.name)) == city
            && raw.iso_country == seed.iso_country;
        let catalog = catalog_code_for_member(&seed.iata);
        let in_same_metro = catalog.is_some_and(|c| catalog_code_for_member(&raw.iata) == Some(c));
        if same_city || in_same_metro {
            have.insert(raw.iata.clone());
            rows.push(raw);
        }
    }
    rows.sort_by_key(|r| {
        (
            if r.type_ == "large_airport" { 0 } else { 1 },
            r.iata.clone(),
        )
    });
    Ok(rows)
}

fn metro_place(
    code: &str,
    city: &str,
    iso2: &str,
    rows: &[AirportRow],
    continents: &HashMap<String, String>,
    session_country: Option<&CountryRow>,
    force_city_prefix: bool,
) -> Airport {
    let members: Vec<String> = rows.iter().map(|r| r.iata.clone()).collect();
    let airport_collision = rows.iter().any(|r| r.iata == code);
    let place_id = if airport_collision || force_city_prefix {
        format!("CITY-{code}")
    } else {
        code.to_string()
    };
    let lat = rows.iter().map(|r| r.lat).sum::<f64>() / rows.len() as f64;
    let lon = rows.iter().map(|r| r.lon).sum::<f64>() / rows.len() as f64;
    let country_name = session_country
        .map(|c| c.name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| iso2.to_string());
    Airport {
        iata: code.to_string(),
        place_id,
        icao: None,
        ident: None,
        name: "All airports".into(),
        city: city.to_string(),
        country: iso2.to_string(),
        country_name,
        region_code: String::new(),
        region_name: String::new(),
        continent: rows[0].continent.clone(),
        continent_name: continents
            .get(&rows[0].continent)
            .cloned()
            .unwrap_or_else(|| rows[0].continent.clone()),
        currency_code: session_country
            .map(|c| c.currency_code.clone())
            .unwrap_or_default(),
        lat,
        lon,
        metro: city.to_string(),
        type_: "city".into(),
        scheduled_service: true,
        elevation_ft: None,
        wikipedia: None,
        source: "metro".into(),
        hub_carriers: vec![],
        members,
        runways: vec![],
        navaids: vec![],
    }
}

fn metro_matches(needle: &str, code: &str, city: &str, aliases: &[&str]) -> bool {
    let q = fold(needle);
    if q.is_empty() {
        return false;
    }
    if q == fold(code) || q == fold(city) {
        return true;
    }
    if q.len() >= 2 && (has_phrase(city, needle) || fold(city).starts_with(&q)) {
        return true;
    }
    if aliases.iter().any(|a| q == fold(a)) {
        return true;
    }
    q.len() >= 2
        && aliases
            .iter()
            .any(|a| fold(a).starts_with(&q) || has_phrase(a, needle))
}

pub fn fold(s: &str) -> String {
    s.nfkd()
        .filter(|ch| !is_combining_mark(*ch))
        .collect::<String>()
        .to_lowercase()
        .trim()
        .to_string()
}

pub fn city_label(municipality: &str, name: &str) -> String {
    let mut m = municipality.trim().to_string();
    if let Some((head, _)) = m.split_once('(') {
        m = head.trim().to_string();
    }
    if let Some((head, _)) = m.split_once(',') {
        if head.len() >= 3 {
            m = head.trim().to_string();
        }
    }
    if m.is_empty() {
        name.to_string()
    } else {
        m
    }
}

pub fn tokens(s: &str) -> Vec<String> {
    TOKEN_RE
        .find_iter(&fold(s))
        .map(|m| m.as_str().to_string())
        .collect()
}

pub fn has_phrase(hay: &str, needle: &str) -> bool {
    let n = fold(needle);
    let h = fold(hay);
    if n.is_empty() || h.is_empty() {
        return false;
    }
    if h == n
        || h.starts_with(&(n.clone() + " "))
        || h.starts_with(&(n.clone() + "("))
        || h.starts_with(&(n.clone() + ","))
    {
        return true;
    }
    let re = Regex::new(&format!(r"(^|[^a-z0-9]){}([^a-z0-9]|$)", regex::escape(&n))).ok();
    re.map(|r| r.is_match(&h)).unwrap_or(false)
}

pub fn prefix_token(hay: &str, needle: &str) -> bool {
    let n = fold(needle);
    if n.len() < 4 {
        return false;
    }
    tokens(hay).iter().any(|t| {
        t.starts_with(&n) && {
            let d = t.len() as i32 - n.len() as i32;
            (1..=2).contains(&d)
        }
    })
}

pub fn keyword_parts(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return vec![];
    };
    raw.replace(';', ",")
        .split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

pub fn score_airport(row: &AirportRow, needle: &str) -> i32 {
    let q = fold(needle);
    if q.is_empty() {
        return 0;
    }
    let iata = row.iata.to_lowercase();
    let icao = row.icao.clone().unwrap_or_default().to_lowercase();
    let city = row.municipality.clone();
    let name = row.name.clone();
    let city_l = city_label(&city, &name);
    let kws = keyword_parts(row.keywords.as_deref());
    let mut score = 0;

    if iata == q || icao == q {
        score += 1000;
    } else if iata.starts_with(&q) && q.len() >= 3 {
        score += 720;
    } else if iata.starts_with(&q) && q.len() == 2 {
        // "pa" is also Paris; two-letter IATA prefixes must not bury city names.
        score += 160;
    } else if icao.starts_with(&q) && q.len() >= 4 {
        score += 700;
    }

    if !matches!(
        row.type_.as_str(),
        "large_airport" | "medium_airport" | "small_airport"
    ) && iata != q
        && icao != q
    {
        return 0;
    }

    let mut text_hit = false;
    let kw_folded: Vec<String> = kws.iter().map(|k| fold(k)).collect();
    if kw_folded.iter().any(|k| k == &q) {
        score += 460;
        text_hit = true;
    } else if kws
        .iter()
        .any(|k| has_phrase(k, &q) && tokens(k).len() == 1)
    {
        score += 240;
        text_hit = true;
    } else if kws.iter().any(|k| has_phrase(k, &q)) {
        score += 40;
        text_hit = true;
    }

    if fold(&city_l) == q {
        score += 420;
        text_hit = true;
    } else if fold(&city_l).starts_with(&q) && q.len() >= 2 {
        score += 300;
        text_hit = true;
    } else if has_phrase(&city_l, &q) || fold(&city_l).starts_with(&(q.clone() + " ")) {
        score += 300;
        text_hit = true;
    } else if has_phrase(&city, &q) {
        score += 80;
        text_hit = true;
    } else if prefix_token(&city_l, &q) {
        score += 60;
        text_hit = true;
    }

    if has_phrase(&name, &q) {
        score += 160;
        text_hit = true;
    } else if fold(&name).starts_with(&(q.clone() + " "))
        || fold(&name).starts_with(&(q.clone() + "-"))
    {
        score += 100;
        text_hit = true;
    }

    if q.len() >= 4 && has_phrase(&row.country_name, &q) {
        score += 90;
        text_hit = true;
    }
    if q.len() == 2 && q == row.iso_country.to_lowercase() {
        score += 100;
        text_hit = true;
    }
    if q.len() >= 5 && has_phrase(&row.region_name, &q) {
        score += 55;
        text_hit = true;
    }

    if iata == q || icao == q || (iata.starts_with(&q) && q.len() >= 2) {
        text_hit = true;
    }

    if !text_hit {
        return 0;
    }

    if row.scheduled_service {
        score += 120;
        if row.type_ == "large_airport" && fold(&city_l) == q {
            score += 90;
        }
    } else {
        score -= 90;
    }
    score += type_weight(&row.type_);
    score
}

async fn search_candidates(pool: &PgPool, needle: &str) -> anyhow::Result<Vec<AirportRow>> {
    let like = format!("%{needle}%");
    let code = needle.to_uppercase();
    let short = needle.len() <= 3 && needle.chars().all(|c| c.is_ascii_alphabetic());
    let code_like = if short {
        format!("{code}%")
    } else {
        code.clone()
    };
    let unaccent_sql = r#"
        SELECT iata FROM airports
        WHERE iata ILIKE $2
           OR icao ILIKE $2
           OR unaccent(municipality) ILIKE '%' || unaccent($1) || '%'
           OR unaccent(name) ILIKE '%' || unaccent($1) || '%'
           OR unaccent(coalesce(keywords, '')) ILIKE '%' || unaccent($1) || '%'
           OR ($3 AND unaccent(country_name) ILIKE '%' || unaccent($1) || '%')
           OR ($4 AND unaccent(region_name) ILIKE '%' || unaccent($1) || '%'
               AND unaccent(region_name) NOT ILIKE '%parish%')
           OR ($5 AND similarity(unaccent(municipality), unaccent($1)) > 0.4)
           OR ($5 AND similarity(unaccent(name), unaccent($1)) > 0.45)
        LIMIT 160
    "#;
    match sqlx::query_scalar::<_, String>(unaccent_sql)
        .bind(needle)
        .bind(&code_like)
        .bind(needle.len() >= 4)
        .bind(needle.len() >= 5)
        .bind(needle.len() >= 5)
        .fetch_all(pool)
        .await
    {
        Ok(codes) if !codes.is_empty() => {
            let rows: Vec<AirportRow> =
                sqlx::query_as("SELECT * FROM airports WHERE iata = ANY($1::text[])")
                    .bind(&codes)
                    .fetch_all(pool)
                    .await?;
            if !rows.is_empty() {
                return Ok(rows);
            }
        }
        _ => {}
    }
    let icao_like = if short {
        format!("{code}%")
    } else {
        like.clone()
    };
    let rows = sqlx::query_as(
        r#"
        SELECT * FROM airports
        WHERE iata ILIKE $1
           OR icao ILIKE $2
           OR municipality ILIKE $3
           OR name ILIKE $3
           OR keywords ILIKE $3
           OR ($4 AND country_name ILIKE $3)
           OR ($5 AND region_name ILIKE $3)
           OR ($6 AND iso_country = $7)
        LIMIT 160
        "#,
    )
    .bind(&code_like)
    .bind(&icao_like)
    .bind(&like)
    .bind(needle.len() >= 4)
    .bind(needle.len() >= 5)
    .bind(needle.len() == 2)
    .bind(&code)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn search_airports(pool: &PgPool, q: &str, limit: i64) -> anyhow::Result<Vec<Airport>> {
    let needle = q.trim().to_string();
    let names = continent_names(pool).await?;
    if needle.is_empty() {
        let rows: Vec<AirportRow> = sqlx::query_as(
            "SELECT * FROM airports WHERE scheduled_service = TRUE AND \"type\" = 'large_airport' \
             ORDER BY municipality LIMIT 20",
        )
        .fetch_all(pool)
        .await?;
        return Ok(rows.iter().map(|r| to_airport(r, "", &names)).collect());
    }

    let country_iso = matching_country_iso(pool, &needle).await?;
    let mut rows = search_candidates(pool, &needle).await?;
    if let Some(iso) = &country_iso {
        let extras: Vec<AirportRow> = sqlx::query_as(
            "SELECT * FROM airports WHERE iso_country = $1 AND scheduled_service = TRUE \
             AND \"type\" IN ('large_airport', 'medium_airport')",
        )
        .bind(iso)
        .fetch_all(pool)
        .await?;
        let have: HashSet<String> = rows.iter().map(|r| r.iata.clone()).collect();
        for r in extras {
            if !have.contains(&r.iata) {
                rows.push(r);
            }
        }
    }
    let folded = fold(&needle);
    if rows.is_empty()
        && country_iso.is_none()
        && (folded != needle.to_lowercase() || folded.contains(' '))
    {
        let token = folded.split_whitespace().next().unwrap_or(&folded);
        let like = format!("%{token}%");
        let extra: Vec<AirportRow> = sqlx::query_as(
            "SELECT * FROM airports WHERE scheduled_service = TRUE \
             AND \"type\" IN ('large_airport', 'medium_airport') \
             AND (municipality ILIKE $1 OR name ILIKE $1 OR keywords ILIKE $1) \
             LIMIT 400",
        )
        .bind(&like)
        .fetch_all(pool)
        .await?;
        let mut have: HashSet<String> = rows.iter().map(|r| r.iata.clone()).collect();
        for r in extra {
            if have.contains(&r.iata) {
                continue;
            }
            let blob = [
                fold(&r.municipality),
                fold(&r.name),
                fold(r.keywords.as_deref().unwrap_or("")),
                fold(&r.country_name),
                fold(&r.region_name),
            ]
            .join(" ");
            if blob.contains(&folded)
                || has_phrase(&r.municipality, &folded)
                || has_phrase(&r.name, &folded)
            {
                have.insert(r.iata.clone());
                rows.push(r);
            }
        }
    }

    let mut ranked: Vec<(i32, AirportRow)> = rows
        .into_iter()
        .map(|r| (score_airport(&r, &needle), r))
        .collect();
    if let Some(iso) = &country_iso {
        ranked = ranked
            .into_iter()
            .map(|(s, r)| {
                let boost = if needle.chars().count() >= 3 { 400 } else { 40 };
                (s + if r.iso_country == *iso { boost } else { 0 }, r)
            })
            .collect();
        // ISO2 is also a city prefix (PA→Paris, FR→Frankfurt). Keep a small
        // country boost, but only exclusive-filter on a name or ISO3.
        if needle.chars().count() >= 3 {
            let in_country: Vec<_> = ranked
                .iter()
                .filter(|(_, r)| r.iso_country == *iso)
                .cloned()
                .collect();
            if !in_country.is_empty() {
                ranked = in_country;
            }
        }
    }
    ranked.retain(|(s, _)| *s >= 50);

    let qf = fold(&needle);
    let city_hit = ranked
        .iter()
        .find(|(_, r)| fold(&city_label(&r.municipality, &r.name)) == qf && r.scheduled_service)
        .map(|(_, r)| r.clone());
    let mut siblings: HashSet<String> = HashSet::new();
    if let Some(hit) = city_hit {
        let near = nearby_airports(pool, &hit.iata, 90.0).await?;
        let mut have: HashSet<String> = ranked.iter().map(|(_, r)| r.iata.clone()).collect();
        for ap in near {
            if have.contains(&ap.iata) {
                continue;
            }
            if !ap.scheduled_service
                || !matches!(ap.type_.as_str(), "large_airport" | "medium_airport")
            {
                continue;
            }
            if let Some(raw) = fetch_airport(pool, &ap.iata).await? {
                ranked.push((score_airport(&raw, &needle) + 160, raw.clone()));
                have.insert(raw.iata.clone());
                siblings.insert(raw.iata);
            }
        }
    }

    if ranked
        .iter()
        .any(|(_, r)| fold(&city_label(&r.municipality, &r.name)) == qf)
    {
        let kept: Vec<_> = ranked
            .iter()
            .filter(|(_, r)| {
                if siblings.contains(&r.iata) {
                    return true;
                }
                if fold(&city_label(&r.municipality, &r.name)) == qf
                    || has_phrase(&r.municipality, &needle)
                    || has_phrase(&r.name, &needle)
                {
                    return true;
                }
                if keyword_parts(r.keywords.as_deref())
                    .iter()
                    .any(|k| fold(k) == qf || has_phrase(k, &needle))
                {
                    return true;
                }
                qf == r.iata.to_lowercase()
                    || qf == r.icao.clone().unwrap_or_default().to_lowercase()
            })
            .cloned()
            .collect();
        if !kept.is_empty() {
            ranked = kept;
        }
    }

    let exact_code = fold(&needle).len() == 3
        && ranked
            .iter()
            .any(|(_, r)| fold(&needle) == r.iata.to_lowercase());
    ranked.sort_by(|a, b| {
        let sa = if a.1.scheduled_service || exact_code {
            0
        } else {
            1
        };
        let sb = if b.1.scheduled_service || exact_code {
            0
        } else {
            1
        };
        sa.cmp(&sb)
            .then_with(|| b.0.cmp(&a.0))
            .then_with(|| a.1.iata.cmp(&b.1.iata))
    });
    if !exact_code {
        let scheduled: Vec<_> = ranked
            .iter()
            .filter(|(_, r)| r.scheduled_service)
            .cloned()
            .collect();
        let other: Vec<_> = ranked
            .iter()
            .filter(|(_, r)| !r.scheduled_service)
            .take(2)
            .cloned()
            .collect();
        ranked = scheduled;
        ranked.extend(other);
    }
    let airports: Vec<Airport> = ranked
        .iter()
        .take(limit as usize)
        .map(|(_, r)| to_airport(r, "", &names))
        .collect();
    let metros = metros_for_query(pool, &needle, &airports).await?;
    if metros.is_empty() {
        return Ok(airports);
    }
    if exact_code {
        let mut out = Vec::new();
        if let Some(first) = airports.first() {
            out.push(first.clone());
        }
        out.extend(metros);
        out.extend(airports.into_iter().skip(1));
        out.truncate(limit as usize);
        return Ok(out);
    }
    let metro_ids: HashSet<String> = metros.iter().map(|m| m.place_id.clone()).collect();
    let mut out = metros;
    out.extend(
        airports
            .into_iter()
            .filter(|a| !metro_ids.contains(&a.iata)),
    );
    out.truncate(limit as usize);
    Ok(out)
}

async fn metros_for_query(
    pool: &PgPool,
    needle: &str,
    hits: &[Airport],
) -> anyhow::Result<Vec<Airport>> {
    let qf = fold(needle);
    let exact = qf.len() == 3 && hits.iter().any(|h| fold(&h.iata) == qf);
    let mut out: Vec<Airport> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let add = |out: &mut Vec<Airport>, seen: &mut HashSet<String>, metro: Option<Airport>| {
        if let Some(m) = metro {
            if !seen.contains(&m.place_id) && m.members.len() >= 2 {
                seen.insert(m.place_id.clone());
                out.push(m);
            }
        }
    };

    for (code, (city, _iso2, members, aliases)) in METROS.iter() {
        let hit_member = hits.iter().any(|h| members.iter().any(|m| *m == h.iata));
        if exact && hit_member {
            add(&mut out, &mut seen, get_metro(pool, code).await?);
            continue;
        }
        if metro_matches(needle, code, city, aliases) {
            add(&mut out, &mut seen, get_metro(pool, code).await?);
        }
    }

    if out.is_empty() {
        let mut groups: HashMap<(String, String), Vec<&Airport>> = HashMap::new();
        for h in hits {
            if h.type_ == "city" || !h.scheduled_service {
                continue;
            }
            groups
                .entry((h.country.clone(), fold(&h.city)))
                .or_default()
                .push(h);
        }
        for ((_cc, cityf), group) in groups {
            if group.len() < 2 {
                continue;
            }
            if cityf != qf && !(qf.len() >= 3 && cityf.starts_with(&qf)) {
                continue;
            }
            add(&mut out, &mut seen, get_metro(pool, &group[0].iata).await?);
        }
    }
    Ok(out)
}

pub async fn nearby_airports(
    pool: &PgPool,
    iata: &str,
    max_km: f64,
) -> anyhow::Result<Vec<Airport>> {
    let Some(origin) = fetch_airport(pool, iata).await? else {
        return Ok(vec![]);
    };
    let dlat = max_km / 111.0;
    let dlon = max_km / (111.0 * origin.lat.to_radians().abs()).max(20.0);
    let rows: Vec<AirportRow> = sqlx::query_as(
        "SELECT * FROM airports WHERE iata <> $1 AND scheduled_service = TRUE \
         AND lat BETWEEN $2 AND $3 AND lon BETWEEN $4 AND $5",
    )
    .bind(&origin.iata)
    .bind(origin.lat - dlat)
    .bind(origin.lat + dlat)
    .bind(origin.lon - dlon)
    .bind(origin.lon + dlon)
    .fetch_all(pool)
    .await?;
    let names = continent_names(pool).await?;
    let mut out: Vec<(f64, Airport)> = rows
        .iter()
        .filter_map(|r| {
            let d = haversine(origin.lat, origin.lon, r.lat, r.lon);
            if d <= max_km {
                Some((d, to_airport(r, "", &names)))
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out.into_iter().take(6).map(|(_, a)| a).collect())
}

pub async fn destinations_from(
    pool: &PgPool,
    origin: &str,
    limit: i64,
) -> anyhow::Result<Vec<(String, String, i64)>> {
    destinations_from_many(pool, &[origin.to_string()], limit).await
}

pub async fn destinations_from_many(
    pool: &PgPool,
    origins: &[String],
    per_origin: i64,
) -> anyhow::Result<Vec<(String, String, i64)>> {
    let codes: Vec<String> = origins
        .iter()
        .filter(|o| !o.is_empty())
        .map(|o| iata3(o))
        .collect();
    if codes.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query(
        "SELECT origin_iata, dest_iata, airline_iata, COUNT(*)::bigint AS n \
         FROM routes WHERE origin_iata = ANY($1::text[]) AND stops = 0 \
         GROUP BY origin_iata, dest_iata, airline_iata \
         ORDER BY COUNT(*) DESC",
    )
    .bind(&codes)
    .fetch_all(pool)
    .await?;
    let mut seen: HashMap<String, HashSet<String>> =
        codes.iter().cloned().map(|c| (c, HashSet::new())).collect();
    let mut counts: HashMap<String, i64> = codes.iter().cloned().map(|c| (c, 0)).collect();
    let mut out = Vec::new();
    for row in rows {
        let origin: String = row.try_get("origin_iata")?;
        let dest: String = row.try_get("dest_iata")?;
        let airline: String = row.try_get("airline_iata")?;
        let n: i64 = row.try_get("n")?;
        let Some(used) = seen.get_mut(&origin) else {
            continue;
        };
        if used.contains(&dest) || counts.get(&origin).copied().unwrap_or(0) >= per_origin {
            continue;
        }
        used.insert(dest.clone());
        *counts.entry(origin).or_default() += 1;
        out.push((dest, airline, n));
    }
    Ok(out)
}

pub async fn origins_into(pool: &PgPool, dest: &str, limit: i64) -> anyhow::Result<Vec<String>> {
    let code = iata3(dest);
    if code.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query(
        "SELECT origin_iata FROM routes WHERE dest_iata = $1 AND stops = 0 \
         GROUP BY origin_iata ORDER BY COUNT(*) DESC LIMIT $2",
    )
    .bind(&code)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.try_get::<String, _>(0).ok())
        .filter(|s| !s.is_empty())
        .collect())
}

pub async fn busiest_airports(
    pool: &PgPool,
    limit: i64,
    exclude: &HashSet<String>,
) -> anyhow::Result<Vec<String>> {
    let skip: HashSet<String> = exclude
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| iata3(c))
        .collect();
    let fetch_n = (limit + skip.len() as i64 + 8).max(limit);
    let rows = sqlx::query(
        "SELECT origin_iata FROM routes WHERE stops = 0 AND origin_iata <> '' \
         GROUP BY origin_iata ORDER BY COUNT(*) DESC LIMIT $1",
    )
    .bind(fetch_n)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for row in rows {
        let code = iata3(&row.try_get::<String, _>(0).unwrap_or_default());
        if code.len() != 3 || skip.contains(&code) {
            continue;
        }
        out.push(code);
        if out.len() as i64 >= limit {
            break;
        }
    }
    Ok(out)
}

pub async fn continents_for(
    pool: &PgPool,
    codes: &[String],
) -> anyhow::Result<HashMap<String, String>> {
    let want: Vec<String> = codes
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| iata3(c))
        .collect();
    if want.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query("SELECT iata, continent FROM airports WHERE iata = ANY($1::text[])")
        .bind(&want)
        .fetch_all(pool)
        .await?;
    let mut out = HashMap::new();
    for row in rows {
        let iata: String = row.try_get("iata")?;
        let continent: String = row.try_get("continent").unwrap_or_default();
        if !iata.is_empty() {
            out.insert(iata, continent);
        }
    }
    Ok(out)
}

pub async fn list_runways(pool: &PgPool, iata: &str) -> anyhow::Result<Vec<Runway>> {
    let rows: Vec<RunwayDb> = sqlx::query_as(
        "SELECT id, length_ft, width_ft, surface, lighted, closed, le_ident, he_ident \
         FROM runways WHERE airport_iata = $1 ORDER BY length_ft DESC NULLS LAST",
    )
    .bind(iata.to_uppercase())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Runway {
            id: r.id,
            length_ft: r.length_ft,
            width_ft: r.width_ft,
            surface: r.surface,
            lighted: r.lighted,
            closed: r.closed,
            le_ident: r.le_ident,
            he_ident: r.he_ident,
        })
        .collect())
}

pub async fn list_navaids(pool: &PgPool, ident: Option<&str>) -> anyhow::Result<Vec<Navaid>> {
    let Some(ident) = ident else {
        return Ok(vec![]);
    };
    let rows: Vec<NavaidDb> = sqlx::query_as(
        "SELECT ident, name, \"type\", frequency_khz FROM navaids WHERE associated_airport = $1 LIMIT 20",
    )
    .bind(ident)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Navaid {
            ident: r.ident,
            name: r.name,
            type_: r.type_,
            frequency_khz: r.frequency_khz,
        })
        .collect())
}

fn to_country(
    row: &CountryRow,
    airport_count: i32,
    continents: &HashMap<String, String>,
) -> Country {
    Country {
        iso2: row.iso2.clone(),
        iso3: row.iso3.clone(),
        name: row.name.clone(),
        continent: row.continent.clone(),
        continent_name: continents
            .get(&row.continent)
            .cloned()
            .unwrap_or_else(|| row.continent.clone()),
        capital: row.capital.clone(),
        currency_code: row.currency_code.clone(),
        currency_name: row.currency_name.clone(),
        tld: row.tld.clone(),
        phone: row.phone.clone(),
        languages: row.languages.clone(),
        population: row.population,
        area_km2: row.area_km2,
        wikipedia: row.wikipedia.clone(),
        airport_count,
        sources: row.sources.clone(),
    }
}

fn country_alias(q: &str) -> Option<&'static str> {
    match q {
        "deutschland" => Some("DE"),
        "ivory" | "ivory coast" => Some("CI"),
        "holland" => Some("NL"),
        "uk" | "england" | "britain" | "great britain" => Some("GB"),
        "uae" => Some("AE"),
        "korea" | "south korea" => Some("KR"),
        "north korea" => Some("KP"),
        "czech" | "czechia" | "czech republic" => Some("CZ"),
        "russia" => Some("RU"),
        "burma" | "myanmar" => Some("MM"),
        "cape verde" => Some("CV"),
        "east timor" => Some("TL"),
        "swaziland" => Some("SZ"),
        "macedonia" => Some("MK"),
        "bolivia" => Some("BO"),
        "tanzania" => Some("TZ"),
        "iran" => Some("IR"),
        "laos" => Some("LA"),
        "syria" => Some("SY"),
        "vietnam" => Some("VN"),
        "venezuela" => Some("VE"),
        _ => None,
    }
}

fn score_country(row: &CountryRow, needle: &str) -> i32 {
    let q = fold(needle);
    if q.is_empty() {
        return 1;
    }
    let mut score = 0;
    let iso2 = fold(&row.iso2);
    let iso3 = fold(row.iso3.as_deref().unwrap_or(""));
    let name = row.name.clone();
    let capital = row.capital.clone();
    if iso2 == q {
        score += 1000;
    }
    if iso3 == q {
        score += 900;
    }
    if fold(&name) == q {
        score += 800;
    } else if fold(&name).starts_with(&q) {
        score += 520;
    } else if has_phrase(&name, &q) {
        score += 320;
    }
    if fold(&capital) == q {
        score += 450;
    } else if has_phrase(&capital, &q) {
        score += 180;
    }
    if q.len() == 3 && fold(&row.currency_code) == q {
        score += 60;
    }
    if country_alias(&q) == Some(row.iso2.as_str()) {
        score += 1000;
    }
    score
}

pub async fn search_countries(pool: &PgPool, q: &str, limit: i64) -> anyhow::Result<Vec<Country>> {
    let count_rows = sqlx::query(
        "SELECT iso_country, COUNT(*)::bigint FROM airports WHERE scheduled_service = TRUE GROUP BY iso_country",
    )
    .fetch_all(pool)
    .await?;
    let mut count_map: HashMap<String, i32> = HashMap::new();
    for row in count_rows {
        let iso: String = row.try_get(0)?;
        let n: i64 = row.try_get(1)?;
        count_map.insert(iso, n as i32);
    }
    let rows: Vec<CountryRow> = sqlx::query_as("SELECT * FROM countries")
        .fetch_all(pool)
        .await?;
    let names = continent_names(pool).await?;
    let needle = q.trim();
    if needle.is_empty() {
        let mut rows = rows;
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        return Ok(rows
            .iter()
            .take(limit as usize)
            .map(|r| to_country(r, count_map.get(&r.iso2).copied().unwrap_or(0), &names))
            .collect());
    }
    let qf = fold(needle);
    let mut ranked: Vec<(i32, CountryRow)> = rows
        .into_iter()
        .map(|r| (score_country(&r, needle), r))
        .filter(|(s, _)| *s > 0)
        .collect();
    if let Some(alias) = country_alias(&qf) {
        let exact: Vec<_> = ranked
            .iter()
            .filter(|(_, r)| r.iso2 == alias)
            .cloned()
            .collect();
        if !exact.is_empty() {
            ranked = exact;
        }
    } else {
        let exact_name: Vec<_> = ranked
            .iter()
            .filter(|(_, r)| fold(&r.name) == qf)
            .cloned()
            .collect();
        if !exact_name.is_empty() {
            ranked = exact_name;
        } else if qf.len() == 2 {
            let exact: Vec<_> = ranked
                .iter()
                .filter(|(_, r)| fold(&r.iso2) == qf)
                .cloned()
                .collect();
            if !exact.is_empty() {
                ranked = exact;
            }
        } else if qf.len() == 3 {
            let exact: Vec<_> = ranked
                .iter()
                .filter(|(_, r)| fold(r.iso3.as_deref().unwrap_or("")) == qf)
                .cloned()
                .collect();
            if !exact.is_empty() {
                ranked = exact;
            }
        }
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    Ok(ranked
        .into_iter()
        .take(limit as usize)
        .map(|(_, r)| to_country(&r, count_map.get(&r.iso2).copied().unwrap_or(0), &names))
        .collect())
}

async fn matching_country_iso(pool: &PgPool, needle: &str) -> anyhow::Result<Option<String>> {
    let qf = fold(needle);
    if let Some(alias) = country_alias(&qf) {
        return Ok(Some(alias.to_string()));
    }
    let rows: Vec<CountryRow> = sqlx::query_as("SELECT * FROM countries")
        .fetch_all(pool)
        .await?;
    let mut ranked: Vec<(i32, CountryRow)> = rows
        .into_iter()
        .map(|r| (score_country(&r, needle), r))
        .filter(|(s, _)| *s > 0)
        .collect();
    if ranked.is_empty() {
        return Ok(None);
    }
    if qf.len() == 2 {
        if let Some(r) = ranked.iter().find(|(_, r)| fold(&r.iso2) == qf) {
            return Ok(Some(r.1.iso2.clone()));
        }
    } else if qf.len() == 3 {
        if let Some(r) = ranked
            .iter()
            .find(|(_, r)| fold(r.iso3.as_deref().unwrap_or("")) == qf)
        {
            return Ok(Some(r.1.iso2.clone()));
        }
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    let top = &ranked[0].1;
    if qf == fold(&top.iso2)
        || qf == fold(top.iso3.as_deref().unwrap_or(""))
        || qf == fold(&top.name)
    {
        return Ok(Some(top.iso2.clone()));
    }
    Ok(None)
}

pub async fn get_country(pool: &PgPool, iso2: &str) -> anyhow::Result<Option<Country>> {
    let Some(row) = fetch_country(pool, iso2).await? else {
        return Ok(None);
    };
    let n: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM airports WHERE iso_country = $1 AND scheduled_service = TRUE",
    )
    .bind(&row.iso2)
    .fetch_one(pool)
    .await?;
    let names = continent_names(pool).await?;
    Ok(Some(to_country(&row, n.0 as i32, &names)))
}

pub async fn regions_for(pool: &PgPool, iso2: &str) -> anyhow::Result<Vec<Region>> {
    let country = fetch_country(pool, iso2).await?;
    let rows: Vec<RegionRow> =
        sqlx::query_as("SELECT * FROM regions WHERE iso_country = $1 ORDER BY name")
            .bind(iso2.to_uppercase())
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|r| Region {
            code: r.code,
            local_code: r.local_code,
            name: r.name,
            iso_country: r.iso_country.clone(),
            country_name: country
                .as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| r.iso_country),
            continent: r.continent,
        })
        .collect())
}

pub async fn list_continents(pool: &PgPool) -> anyhow::Result<Vec<Value>> {
    let rows: Vec<ContinentRow> =
        sqlx::query_as("SELECT code, name, source FROM continents ORDER BY name")
            .fetch_all(pool)
            .await?;
    let mut out = Vec::new();
    for r in rows {
        let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM countries WHERE continent = $1")
            .bind(&r.code)
            .fetch_one(pool)
            .await?;
        out.push(json!({
            "code": r.code,
            "name": r.name,
            "country_count": n.0,
            "source": r.source,
        }));
    }
    Ok(out)
}

pub async fn counts(pool: &PgPool) -> anyhow::Result<HashMap<String, i64>> {
    async fn c(pool: &PgPool, table: &str) -> anyhow::Result<i64> {
        let n: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await?;
        Ok(n.0)
    }
    Ok(HashMap::from([
        ("continents".into(), c(pool, "continents").await?),
        ("countries".into(), c(pool, "countries").await?),
        ("regions".into(), c(pool, "regions").await?),
        ("airports".into(), c(pool, "airports").await?),
        ("runways".into(), c(pool, "runways").await?),
        ("navaids".into(), c(pool, "navaids").await?),
        ("airlines".into(), c(pool, "airlines").await?),
        ("routes".into(), c(pool, "routes").await?),
        ("hidden_deals".into(), c(pool, "hidden_deals").await?),
        (
            "fare_observations".into(),
            c(pool, "fare_observations").await?,
        ),
        ("route_edges".into(), c(pool, "route_edges").await?),
        (
            "hidden_city_route_stats".into(),
            c(pool, "hidden_city_route_stats").await?,
        ),
        ("provider_calls".into(), c(pool, "provider_calls").await?),
    ]))
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (rlat1, rlon1, rlat2, rlon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let dlon = rlon2 - rlon1;
    let dlat = rlat2 - rlat1;
    let h = (dlat / 2.0).sin().powi(2) + rlat1.cos() * rlat2.cos() * (dlon / 2.0).sin().powi(2);
    6371.0 * 2.0 * h.sqrt().asin()
}

pub async fn persist_search(
    pool: &PgPool,
    origin: &str,
    destination: &str,
    date: &str,
    adults: i32,
    cabin: &str,
    currency: &str,
    elapsed_ms: f64,
    sources: &[String],
    offers: &[Value],
) -> anyhow::Result<i32> {
    let mut tx = pool.begin().await?;
    let row: (i32,) = sqlx::query_as(
        "INSERT INTO searches (origin, destination, date, adults, cabin, currency, elapsed_ms, sources) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
    )
    .bind(origin)
    .bind(destination)
    .bind(date)
    .bind(adults)
    .bind(cabin)
    .bind(currency)
    .bind(elapsed_ms)
    .bind(Json(sources))
    .fetch_one(&mut *tx)
    .await?;
    let search_id = row.0;
    for o in offers {
        sqlx::query(
            "INSERT INTO offers (search_id, offer_uid, source, layer, kind, price, currency, payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(search_id)
        .bind(o.get("id").and_then(|v| v.as_str()).unwrap_or(""))
        .bind(o.get("source").and_then(|v| v.as_str()).unwrap_or(""))
        .bind(o.get("layer").and_then(|v| v.as_str()).unwrap_or("priced-offer"))
        .bind(o.get("kind").and_then(|v| v.as_str()).unwrap_or(""))
        .bind(o.get("price").and_then(|v| v.as_f64()))
        .bind(o.get("currency").and_then(|v| v.as_str()))
        .bind(Json(o))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(search_id)
}

pub async fn remember_offers(
    pool: &PgPool,
    offers: &[Offer],
    date: &str,
    adults: i32,
    cabin: &str,
) -> anyhow::Result<i32> {
    let mut stored = 0i32;
    let now = Utc::now();
    for offer in offers {
        let Some(converted) = offer_in_usd(offer) else {
            continue;
        };
        let Some(meta) = observation_meta(&converted) else {
            continue;
        };
        let payload = serde_json::to_value(&converted).unwrap_or(json!({}));
        sqlx::query(
            "INSERT INTO offer_observations \
             (origin, ticketed, connections, date, adults, cabin, currency, source, fingerprint, price, payload, observed_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
             ON CONFLICT ON CONSTRAINT uq_offer_obs DO UPDATE SET \
               price = EXCLUDED.price, payload = EXCLUDED.payload, connections = EXCLUDED.connections, \
               observed_at = EXCLUDED.observed_at, currency = EXCLUDED.currency",
        )
        .bind(&meta.origin)
        .bind(&meta.ticketed)
        .bind(Json(&meta.connections))
        .bind(date)
        .bind(adults)
        .bind(cabin)
        .bind(&meta.currency)
        .bind(&meta.source)
        .bind(&meta.fingerprint)
        .bind(meta.price)
        .bind(Json(payload))
        .bind(now)
        .execute(pool)
        .await?;
        stored += 1;
    }
    Ok(stored)
}

pub async fn recall_offers(
    pool: &PgPool,
    origins: &HashSet<String>,
    date: &str,
    adults: i32,
    cabin: &str,
    max_age_seconds: i64,
) -> anyhow::Result<Vec<Offer>> {
    if origins.is_empty() {
        return Ok(vec![]);
    }
    let cutoff = Utc::now() - chrono::Duration::seconds(max_age_seconds);
    let codes: Vec<String> = origins.iter().map(|o| iata3(o)).collect();
    let rows: Vec<OfferObsRow> = sqlx::query_as(
        "SELECT fingerprint, payload FROM offer_observations \
         WHERE origin = ANY($1::text[]) AND date = $2 AND adults = $3 AND cabin = $4 AND observed_at >= $5 \
         ORDER BY observed_at DESC LIMIT 80",
    )
    .bind(&codes)
    .bind(date)
    .bind(adults)
    .bind(cabin)
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        if !seen.insert(row.fingerprint.clone()) {
            continue;
        }
        if let Ok(offer) = serde_json::from_value::<Offer>(row.payload.0) {
            out.push(offer);
        }
    }
    Ok(unexpired(&offers_in_usd(&out)))
}

pub async fn recall_candidate_dests(
    pool: &PgPool,
    origins: &HashSet<String>,
    intended: &HashSet<String>,
    date: &str,
    adults: i32,
    cabin: &str,
    max_age_seconds: i64,
    limit: i64,
    any_date: bool,
) -> anyhow::Result<Vec<String>> {
    if origins.is_empty() {
        return Ok(vec![]);
    }
    let cutoff = Utc::now() - chrono::Duration::seconds(max_age_seconds);
    let codes: Vec<String> = origins.iter().map(|o| iata3(o)).collect();
    let want: HashSet<String> = intended.iter().map(|c| c.to_uppercase()).collect();
    let rows = if any_date {
        sqlx::query(
            "SELECT ticketed, connections FROM offer_observations \
             WHERE origin = ANY($1::text[]) AND observed_at >= $2 \
             ORDER BY observed_at DESC LIMIT 2000",
        )
        .bind(&codes)
        .bind(cutoff)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT ticketed, connections FROM offer_observations \
             WHERE origin = ANY($1::text[]) AND observed_at >= $2 AND date = $3 AND adults = $4 AND cabin = $5 \
             ORDER BY observed_at DESC LIMIT 2000",
        )
        .bind(&codes)
        .bind(cutoff)
        .bind(date)
        .bind(adults)
        .bind(cabin)
        .fetch_all(pool)
        .await?
    };
    let mut ranked = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let ticketed: String = row.try_get("ticketed")?;
        let connections: Json<Value> = row.try_get("connections")?;
        let hops: HashSet<String> = json_strings(&connections.0)
            .into_iter()
            .map(|c| c.to_uppercase())
            .collect();
        let t = ticketed.to_uppercase();
        if !hops.is_disjoint(&want) && !want.contains(&t) && seen.insert(t.clone()) {
            ranked.push(t);
        }
        if ranked.len() as i64 >= limit {
            break;
        }
    }
    Ok(ranked)
}

pub async fn persist_tracks(
    pool: &PgPool,
    airport_iata: &str,
    states: &[Value],
    api_time: Option<i32>,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    for s in states {
        sqlx::query(
            "INSERT INTO track_snapshots \
             (airport_iata, icao24, callsign, origin_country, lat, lon, baro_altitude_m, on_ground, \
              velocity_ms, true_track, position_source, api_time) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(airport_iata)
        .bind(s.get("icao24").and_then(|v| v.as_str()).unwrap_or(""))
        .bind(s.get("callsign").and_then(|v| v.as_str()))
        .bind(s.get("origin_country").and_then(|v| v.as_str()))
        .bind(s.get("lat").and_then(|v| v.as_f64()))
        .bind(s.get("lon").and_then(|v| v.as_f64()))
        .bind(s.get("baro_altitude_m").and_then(|v| v.as_f64()))
        .bind(s.get("on_ground").and_then(|v| v.as_bool()).unwrap_or(false))
        .bind(s.get("velocity_ms").and_then(|v| v.as_f64()))
        .bind(s.get("true_track").and_then(|v| v.as_f64()))
        .bind(s.get("position_source").and_then(|v| v.as_i64()).map(|n| n as i32))
        .bind(api_time)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn airlines_by_iata(
    pool: &PgPool,
    codes: &[String],
) -> anyhow::Result<Vec<(String, String)>> {
    let clean: Vec<String> = codes
        .iter()
        .filter(|c| !c.is_empty() && c.len() >= 2)
        .map(|c| c.to_uppercase())
        .collect();
    if clean.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query(
        "SELECT iata, name FROM airlines WHERE iata = ANY($1::text[]) AND active = TRUE",
    )
    .bind(&clean)
    .fetch_all(pool)
    .await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let iata: Option<String> = row.try_get("iata")?;
        let name: String = row.try_get("name")?;
        if let Some(iata) = iata {
            if seen.insert(iata.clone()) {
                out.push((iata, name));
            }
        }
    }
    Ok(out)
}

pub async fn default_pair(pool: &PgPool) -> anyhow::Result<(Option<Airport>, Option<Airport>)> {
    let rows = sqlx::query(
        "SELECT origin_iata FROM routes GROUP BY origin_iata ORDER BY COUNT(*) DESC LIMIT 40",
    )
    .fetch_all(pool)
    .await?;
    let mut origin = None;
    for row in rows {
        let iata: String = row.try_get(0)?;
        if let Some(ap) = get_airport(pool, &iata, false).await? {
            if ap.scheduled_service {
                origin = Some(ap);
                break;
            }
        }
    }
    let Some(origin) = origin else {
        return Ok((None, None));
    };
    let mut dest = None;
    for (code, _, _) in destinations_from(pool, &origin.iata, 20).await? {
        if let Some(d) = get_airport(pool, &code, false).await? {
            if d.iata != origin.iata && (d.city != origin.city || d.country != origin.country) {
                dest = Some(d);
                break;
            }
        }
    }
    Ok((Some(origin), dest))
}

fn call_bookers(
    origin: &str,
    dest: &str,
    date: &str,
    adults: i32,
    airlines: &[(String, String)],
    currency: &str,
    cabin: &str,
    return_date: Option<&str>,
) -> Vec<BookerLink> {
    booker_links(
        origin,
        dest,
        date,
        adults,
        airlines,
        currency,
        cabin,
        return_date,
    )
}

pub async fn persist_hidden_deals(
    pool: &PgPool,
    matches: &[HiddenCityMatch],
    origin: &str,
    origin_city: &str,
    dest: &str,
    dest_city: &str,
    date: &str,
) -> anyhow::Result<i32> {
    let mut saved = 0i32;
    for m in matches {
        if !is_live_fare(&m.local_offer) || !is_live_fare(&m.through_offer) {
            continue;
        }
        let Some(local) = offer_in_usd(&m.local_offer) else {
            continue;
        };
        let Some(through) = offer_in_usd(&m.through_offer) else {
            continue;
        };
        let (Some(lp), Some(tp)) = (local.price, through.price) else {
            continue;
        };
        if tp >= lp || !offer_unexpired(&through) {
            continue;
        }
        let Some(saving) = hidden_city_savings(Some(lp), Some(tp)) else {
            continue;
        };
        if !meaningful_saving(Some(saving), None) {
            continue;
        }
        let pct = if lp != 0.0 {
            (100.0 * saving / lp * 100.0).round() / 100.0
        } else {
            0.0
        };
        let first = through.first_flight.clone();
        let bookers = if m.bookers.is_empty() {
            call_bookers(origin, &m.hidden_city, date, 1, &[], "USD", "ECONOMY", None)
        } else {
            m.bookers.clone()
        };
        let bookers_v = serde_json::to_value(&bookers).unwrap_or(json!([]));
        let local_v = serde_json::to_value(&local).unwrap_or(json!({}));
        let through_v = serde_json::to_value(&through).unwrap_or(json!({}));
        let existing: Option<(i32,)> = sqlx::query_as(
            "SELECT id FROM hidden_deals WHERE origin = $1 AND destination = $2 AND hidden_city = $3 \
             AND date = $4 AND first_flight = $5",
        )
        .bind(origin)
        .bind(dest)
        .bind(&m.hidden_city)
        .bind(date)
        .bind(&first)
        .fetch_optional(pool)
        .await?;
        if existing.is_some() {
            sqlx::query(
                "UPDATE hidden_deals SET origin_city=$1, dest_city=$2, hidden_city_name=$3, \
                 honest_price=$4, through_price=$5, currency='USD', saving=$6, saving_pct=$7, \
                 source=$8, bookers=$9, local_payload=$10, through_payload=$11 \
                 WHERE origin=$12 AND destination=$13 AND hidden_city=$14 AND date=$15 AND first_flight=$16",
            )
            .bind(origin_city)
            .bind(dest_city)
            .bind(&m.hidden_city_name)
            .bind(lp)
            .bind(tp)
            .bind(saving)
            .bind(pct)
            .bind(&through.source)
            .bind(bookers_v.clone())
            .bind(local_v.clone())
            .bind(through_v.clone())
            .bind(origin)
            .bind(dest)
            .bind(&m.hidden_city)
            .bind(date)
            .bind(&first)
            .execute(pool)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO hidden_deals \
                 (origin, destination, hidden_city, origin_city, dest_city, hidden_city_name, date, \
                  honest_price, through_price, currency, saving, saving_pct, first_flight, source, \
                  bookers, local_payload, through_payload) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'USD',$10,$11,$12,$13,$14,$15,$16)",
            )
            .bind(origin)
            .bind(dest)
            .bind(&m.hidden_city)
            .bind(origin_city)
            .bind(dest_city)
            .bind(&m.hidden_city_name)
            .bind(date)
            .bind(lp)
            .bind(tp)
            .bind(saving)
            .bind(pct)
            .bind(&first)
            .bind(&through.source)
            .bind(bookers_v)
            .bind(local_v)
            .bind(through_v)
            .execute(pool)
            .await?;
        }
        saved += 1;
    }
    Ok(saved)
}

fn payload_offer(payload: &Value) -> Option<Offer> {
    if payload.is_null() || payload.as_object().is_some_and(|o| o.is_empty()) {
        return None;
    }
    serde_json::from_value(payload.clone()).ok()
}

pub fn deal_from_row(row: &HiddenDealRow) -> Option<HiddenDeal> {
    let src = row.source.to_lowercase();
    if src != "" && src != "duffel" {
        return None;
    }
    let raw_through = payload_offer(&row.through_payload.0);
    let raw_local = payload_offer(&row.local_payload.0);
    if raw_through.as_ref().is_some_and(|o| !is_live_fare(o)) {
        return None;
    }
    if raw_local.as_ref().is_some_and(|o| !is_live_fare(o)) {
        return None;
    }
    if raw_through.is_none() && matches!(src.as_str(), "mock" | "duffel") {
        return None;
    }
    let ccy = if row.currency.is_empty() {
        "USD"
    } else {
        row.currency.as_str()
    };
    let honest = to_usd(Some(row.honest_price), Some(ccy))?;
    let through = to_usd(Some(row.through_price), Some(ccy))?;
    let saving = ((honest - through) * 100.0).round() / 100.0;
    if !meaningful_saving(Some(saving), None) {
        return None;
    }
    let pct = if honest != 0.0 {
        (100.0 * saving / honest * 100.0).round() / 100.0
    } else {
        row.saving_pct
    };
    let bookers = call_bookers(
        &row.origin,
        &row.hidden_city,
        &row.date,
        1,
        &[],
        "USD",
        "ECONOMY",
        None,
    );
    let local = raw_local.as_ref().and_then(offer_in_usd);
    let through_offer = raw_through.as_ref().and_then(offer_in_usd);
    if through_offer.as_ref().is_some_and(|o| !offer_unexpired(o)) {
        return None;
    }
    let mut risk = None;
    let mut warnings: Vec<String> = vec![];
    if let (Some(local_o), Some(through_o)) = (&local, &through_offer) {
        if local_o.price.is_some() && through_o.price.is_some() {
            risk = Some(assess(
                local_o,
                through_o,
                &row.hidden_city,
                &row.destination,
                "",
                "",
            ));
            warnings = HIDDEN_CITY_WARNINGS.iter().map(|s| s.to_string()).collect();
        }
    }
    Some(HiddenDeal {
        id: row.id,
        origin: row.origin.clone(),
        origin_city: row.origin_city.clone(),
        dest: row.destination.clone(),
        dest_city: row.dest_city.clone(),
        hidden_city: row.hidden_city.clone(),
        hidden_city_name: row.hidden_city_name.clone(),
        date: row.date.clone(),
        honest_price: honest,
        through_price: through,
        currency: "USD".into(),
        saving,
        saving_pct: pct,
        first_flight: row.first_flight.clone(),
        source: row.source.clone(),
        bookers,
        local_offer: local,
        through_offer,
        risk,
        warnings,
    })
}

pub async fn list_hidden_deals(
    pool: &PgPool,
    limit: i64,
    origin: &str,
    dest: &str,
) -> anyhow::Result<Vec<HiddenDeal>> {
    let origin = origin.to_uppercase();
    let dest = dest.to_uppercase();
    let fetch_n = (limit.max(1) * 4).clamp(48, 480);
    let rows: Vec<HiddenDealRow> = sqlx::query_as(
        "SELECT * FROM hidden_deals \
         WHERE ($1 = '' OR origin = $1) AND ($2 = '' OR destination = $2) \
         ORDER BY saving DESC \
         LIMIT $3",
    )
    .bind(&origin)
    .bind(&dest)
    .bind(fetch_n)
    .fetch_all(pool)
    .await?;
    let mut deals: Vec<HiddenDeal> = rows.iter().filter_map(deal_from_row).collect();
    deals.sort_by(|a, b| {
        b.saving
            .partial_cmp(&a.saving)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.saving_pct
                    .partial_cmp(&a.saving_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                a.through_price
                    .partial_cmp(&b.through_price)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let cap = limit.min(120) as usize;
    deals.truncate(cap);
    Ok(deals)
}

pub async fn clear_hidden_deals(pool: &PgPool) -> anyhow::Result<i64> {
    let res = sqlx::query("DELETE FROM hidden_deals")
        .execute(pool)
        .await?;
    Ok(res.rows_affected() as i64)
}

pub async fn route_map_from(
    pool: &PgPool,
    codes: &[String],
    per_origin: i64,
) -> anyhow::Result<HashMap<String, HashSet<String>>> {
    let want: Vec<String> = codes
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| iata3(c))
        .collect();
    if want.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        "SELECT origin_iata, dest_iata, COUNT(*)::bigint AS n FROM routes \
         WHERE origin_iata = ANY($1::text[]) AND stops = 0 \
         GROUP BY origin_iata, dest_iata ORDER BY COUNT(*) DESC",
    )
    .bind(&want)
    .fetch_all(pool)
    .await?;
    let mut out: HashMap<String, HashSet<String>> =
        want.iter().cloned().map(|c| (c, HashSet::new())).collect();
    for row in rows {
        let origin: String = row.try_get("origin_iata")?;
        let dest: String = row.try_get("dest_iata")?;
        let bucket = out.entry(origin).or_default();
        if bucket.len() < per_origin as usize {
            bucket.insert(dest);
        }
    }
    Ok(out)
}

pub async fn load_route_stats(
    pool: &PgPool,
    origins: &HashSet<String>,
    intended: &HashSet<String>,
) -> anyhow::Result<HashMap<String, RouteStat>> {
    let a_codes: Vec<String> = origins
        .iter()
        .filter(|o| !o.is_empty())
        .map(|o| iata3(o))
        .collect();
    let b_codes: Vec<String> = intended
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| iata3(b))
        .collect();
    if a_codes.is_empty() || b_codes.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<RouteStatRow> = sqlx::query_as(
        "SELECT * FROM hidden_city_route_stats WHERE origin = ANY($1::text[]) AND intended = ANY($2::text[])",
    )
    .bind(&a_codes)
    .bind(&b_codes)
    .fetch_all(pool)
    .await?;
    let mut merged: HashMap<String, RouteStat> = HashMap::new();
    let mut weights: HashMap<String, i32> = HashMap::new();
    for row in rows {
        let c = row.ticketed.to_uppercase();
        if let Some(cur) = merged.get_mut(&c) {
            cur.observations += row.observations;
            cur.successful_connections += row.successful_connections;
            let w_old = weights.get(&c).copied().unwrap_or(0);
            let w_new = row.cheaper_than_direct_count.max(0);
            if w_old + w_new > 0 {
                cur.average_saving_percent = (cur.average_saving_percent * w_old as f64
                    + row.average_saving_percent * w_new as f64)
                    / (w_old + w_new) as f64;
            }
            weights.insert(c.clone(), w_old + w_new);
            cur.cheaper_than_direct_count += row.cheaper_than_direct_count;
            cur.median_saving = cur.median_saving.max(row.median_saving);
            if let Some(b) = row.last_success_at {
                if cur.last_success_at.map(|a| b > a).unwrap_or(true) {
                    cur.last_success_at = Some(b);
                }
            }
            if let Some(b) = row.last_cheaper_at {
                if cur.last_cheaper_at.map(|a| b > a).unwrap_or(true) {
                    cur.last_cheaper_at = Some(b);
                }
            }
        } else {
            weights.insert(c.clone(), row.cheaper_than_direct_count.max(0));
            merged.insert(
                c.clone(),
                RouteStat {
                    origin: row.origin,
                    intended: row.intended,
                    ticketed: c,
                    observations: row.observations,
                    successful_connections: row.successful_connections,
                    cheaper_than_direct_count: row.cheaper_than_direct_count,
                    median_saving: row.median_saving,
                    average_saving_percent: row.average_saving_percent,
                    last_success_at: row.last_success_at,
                    last_cheaper_at: row.last_cheaper_at,
                },
            );
        }
    }
    Ok(merged)
}

/// Destinations we have actually seen as B→C inside priced tickets.
/// Stronger than OpenFlights for “who still flies beyond the intended city.”
pub async fn learned_beyond(
    pool: &PgPool,
    from: &HashSet<String>,
    limit: i64,
) -> anyhow::Result<HashMap<String, i32>> {
    let codes: Vec<String> = from
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| iata3(c))
        .collect();
    if codes.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        "SELECT dest, SUM(observation_count)::int AS n \
         FROM route_edges WHERE origin = ANY($1::text[]) \
         GROUP BY dest ORDER BY SUM(observation_count) DESC LIMIT $2",
    )
    .bind(&codes)
    .bind(limit.max(1).min(80))
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::new();
    for row in rows {
        let dest: String = row.try_get("dest")?;
        let n: i32 = row.try_get("n")?;
        let dest = dest.to_uppercase();
        if dest.len() == 3 && !codes.iter().any(|c| c == &dest) {
            out.insert(dest, n);
        }
    }
    Ok(out)
}

pub async fn provider_rates_for(
    pool: &PgPool,
    origins: &HashSet<String>,
    dests: &[String],
) -> anyhow::Result<HashMap<String, f64>> {
    let a_codes: Vec<String> = origins
        .iter()
        .filter(|o| !o.is_empty())
        .map(|o| iata3(o))
        .collect();
    let d_codes: Vec<String> = dests
        .iter()
        .filter(|d| !d.is_empty())
        .map(|d| iata3(d))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if a_codes.is_empty() || d_codes.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        "SELECT dest, COUNT(*)::bigint AS total, \
           SUM(CASE WHEN ok = TRUE AND offers > 0 THEN 1 ELSE 0 END)::bigint AS ok, \
           AVG(latency_ms) AS avg \
         FROM provider_calls \
         WHERE origin = ANY($1::text[]) AND dest = ANY($2::text[]) AND cost_usd > 0 \
         GROUP BY dest",
    )
    .bind(&a_codes)
    .bind(&d_codes)
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::new();
    for row in rows {
        let dest: String = row.try_get("dest")?;
        let total: i64 = row.try_get("total")?;
        let ok: i64 = row.try_get("ok")?;
        let avg: Option<f64> = row.try_get("avg")?;
        out.insert(
            dest,
            provider_score(ok as i32, total as i32, avg.unwrap_or(0.0)),
        );
    }
    Ok(out)
}

pub async fn provider_route_scores(
    pool: &PgPool,
    origin: &str,
    dest: &str,
) -> anyhow::Result<Vec<Value>> {
    let rows = sqlx::query(
        "SELECT provider, COUNT(*)::bigint AS total, \
           SUM(CASE WHEN ok = TRUE AND offers > 0 THEN 1 ELSE 0 END)::bigint AS ok, \
           AVG(latency_ms) AS lat, AVG(offers) AS avg_offers \
         FROM provider_calls WHERE origin = $1 AND dest = $2 GROUP BY provider",
    )
    .bind(iata3(origin))
    .bind(iata3(dest))
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for row in rows {
        let provider: String = row.try_get("provider")?;
        let total: i64 = row.try_get("total")?;
        let ok: i64 = row.try_get("ok")?;
        let lat: Option<f64> = row.try_get("lat")?;
        let avg_offers: Option<f64> = row.try_get("avg_offers")?;
        let lat = lat.unwrap_or(0.0);
        out.push(json!({
            "provider": provider,
            "calls": total,
            "with_offers": ok,
            "avg_latency_ms": (lat * 10.0).round() / 10.0,
            "avg_offers": (avg_offers.unwrap_or(0.0) * 10.0).round() / 10.0,
            "score": provider_score(ok as i32, total as i32, lat),
        }));
    }
    out.sort_by(|a, b| {
        let sa = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let sb = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

pub async fn persist_provider_calls(
    pool: &PgPool,
    calls: &[crate::budget::ProviderCall],
) -> anyhow::Result<i32> {
    if calls.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    for c in calls {
        sqlx::query(
            "INSERT INTO provider_calls (provider, origin, dest, date, purpose, ok, offers, latency_ms, cost_usd) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(&c.provider)
        .bind(clip(&c.origin, 3))
        .bind(clip(&c.dest, 3))
        .bind(&c.date)
        .bind(&c.purpose)
        .bind(c.ok)
        .bind(c.offers)
        .bind(c.latency_ms)
        .bind(c.cost_usd)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(calls.len() as i32)
}

pub async fn budget_summary(pool: &PgPool, hours: i64) -> anyhow::Result<Value> {
    let since = Utc::now() - chrono::Duration::hours(hours);
    let rows = sqlx::query(
        "SELECT provider, purpose, COUNT(*)::bigint AS n, SUM(cost_usd) AS cost, \
           SUM(CASE WHEN ok = TRUE AND offers > 0 THEN 1 ELSE 0 END)::bigint AS ok, \
           AVG(latency_ms) AS lat \
         FROM provider_calls WHERE called_at >= $1 GROUP BY provider, purpose",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    let mut by_row = Vec::new();
    for row in rows {
        let p: String = row.try_get("provider")?;
        let purpose: String = row.try_get("purpose")?;
        let n: i64 = row.try_get("n")?;
        let cost: Option<f64> = row.try_get("cost")?;
        let ok: i64 = row.try_get("ok")?;
        let lat: Option<f64> = row.try_get("lat")?;
        by_row.push(json!({
            "provider": p,
            "purpose": purpose,
            "calls": n,
            "cost_usd": (cost.unwrap_or(0.0) * 10000.0).round() / 10000.0,
            "with_offers": ok,
            "avg_latency_ms": (lat.unwrap_or(0.0) * 10.0).round() / 10.0,
        }));
    }
    let searches: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM searches WHERE created_at >= $1")
        .bind(since)
        .fetch_one(pool)
        .await?;
    let searches = searches.0;
    let total_calls: i64 = by_row
        .iter()
        .map(|r| r.get("calls").and_then(|v| v.as_i64()).unwrap_or(0))
        .sum();
    let paid_calls: i64 = by_row
        .iter()
        .filter(|r| r.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0) > 0.0)
        .map(|r| r.get("calls").and_then(|v| v.as_i64()).unwrap_or(0))
        .sum();
    let cost_usd: f64 = by_row
        .iter()
        .map(|r| r.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0))
        .sum();
    Ok(json!({
        "window_hours": hours,
        "searches": searches,
        "provider_calls": total_calls,
        "paid_calls": paid_calls,
        "cost_usd": (cost_usd * 10000.0).round() / 10000.0,
        "paid_calls_per_search": if searches > 0 {
            (paid_calls as f64 / searches as f64 * 100.0).round() / 100.0
        } else {
            0.0
        },
        "bookings": 0,
        "note": "This app never books; bookings stay 0. Booker clicks are outbound links.",
        "rows": by_row,
    }))
}

pub async fn apply_learning(
    pool: &PgPool,
    batch: &LearnBatch,
) -> anyhow::Result<HashMap<String, i32>> {
    let now = batch.observed_at;
    let mut written = HashMap::from([
        ("fares".into(), 0i32),
        ("edges".into(), 0i32),
        ("stats".into(), 0i32),
    ]);
    let mut tx = pool.begin().await?;
    for f in &batch.fares {
        sqlx::query(
            "INSERT INTO fare_observations \
             (offer_uid, fingerprint, origin, ticketed, connections, stops, carrier, provider, \
              price, currency, date, adults, cabin, fare_brand, expires_at, observed_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(clip(&f.offer_uid, 80))
        .bind(clip(&f.fingerprint, 240))
        .bind(clip(&f.origin, 3))
        .bind(clip(&f.ticketed, 3))
        .bind(Json(&f.connections))
        .bind(f.stops)
        .bind(clip(&f.carrier, 8))
        .bind(clip(&f.provider, 32))
        .bind(f.price)
        .bind(clip(&f.currency, 8))
        .bind(&f.date)
        .bind(f.adults)
        .bind(&f.cabin)
        .bind(f.fare_brand.as_ref().map(|s| clip(s, 64)))
        .bind(f.expires_at.as_ref().map(|s| clip(s, 40)))
        .bind(now)
        .execute(&mut *tx)
        .await?;
        *written.get_mut("fares").unwrap() += 1;
    }

    if !batch.edges.is_empty() {
        let origins: Vec<String> = batch
            .edges
            .keys()
            .map(|k| k.0.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let dests: Vec<String> = batch
            .edges
            .keys()
            .map(|k| k.1.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let existing: Vec<RouteEdgeRow> = sqlx::query_as(
            "SELECT * FROM route_edges WHERE origin = ANY($1::text[]) AND dest = ANY($2::text[])",
        )
        .bind(&origins)
        .bind(&dests)
        .fetch_all(&mut *tx)
        .await?;
        let mut by_key: HashMap<(String, String, String, String), RouteEdgeRow> = existing
            .into_iter()
            .map(|r| {
                (
                    (
                        r.origin.clone(),
                        r.dest.clone(),
                        r.carrier.clone(),
                        r.flight_number.clone(),
                    ),
                    r,
                )
            })
            .collect();
        for (key, edge) in &batch.edges {
            if let Some(row) = by_key.get_mut(key) {
                let mut dates: HashSet<String> =
                    json_strings(&row.travel_dates.0).into_iter().collect();
                dates.extend(edge.travel_dates.iter().cloned());
                let mut dates: Vec<String> = dates.into_iter().collect();
                dates.sort();
                if dates.len() > 60 {
                    dates = dates[dates.len() - 60..].to_vec();
                }
                let n = dates.len() as i32;
                sqlx::query(
                    "UPDATE route_edges SET observation_count = observation_count + $1, travel_dates = $2, \
                     days_seen = $3, last_seen = $4 WHERE origin=$5 AND dest=$6 AND carrier=$7 AND flight_number=$8",
                )
                .bind(edge.count)
                .bind(Json(&dates))
                .bind(n)
                .bind(now)
                .bind(&row.origin)
                .bind(&row.dest)
                .bind(&row.carrier)
                .bind(&row.flight_number)
                .execute(&mut *tx)
                .await?;
            } else {
                let mut dates: Vec<String> = edge.travel_dates.iter().cloned().collect();
                dates.sort();
                sqlx::query(
                    "INSERT INTO route_edges (origin, dest, carrier, flight_number, observation_count, days_seen, travel_dates, first_seen, last_seen) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$8)",
                )
                .bind(clip(&edge.origin, 3))
                .bind(clip(&edge.dest, 3))
                .bind(clip(&edge.carrier, 8))
                .bind(clip(&edge.flight_number, 16))
                .bind(edge.count)
                .bind(dates.len() as i32)
                .bind(Json(&dates))
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
            *written.get_mut("edges").unwrap() += 1;
        }
    }

    if !batch.stats.is_empty() {
        let a_codes: Vec<String> = batch
            .stats
            .keys()
            .map(|k| k.0.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let b_codes: Vec<String> = batch
            .stats
            .keys()
            .map(|k| k.1.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let c_codes: Vec<String> = batch
            .stats
            .keys()
            .map(|k| k.2.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let existing: Vec<RouteStatRow> = sqlx::query_as(
            "SELECT * FROM hidden_city_route_stats WHERE origin = ANY($1::text[]) AND intended = ANY($2::text[]) AND ticketed = ANY($3::text[])",
        )
        .bind(&a_codes)
        .bind(&b_codes)
        .bind(&c_codes)
        .fetch_all(&mut *tx)
        .await?;
        let mut by_key: HashMap<(String, String, String), RouteStatRow> = existing
            .into_iter()
            .map(|r| {
                (
                    (r.origin.clone(), r.intended.clone(), r.ticketed.clone()),
                    r,
                )
            })
            .collect();
        for (key, d) in &batch.stats {
            let mut row = by_key.remove(key).unwrap_or(RouteStatRow {
                origin: clip(&d.origin, 3),
                intended: clip(&d.intended, 3),
                ticketed: clip(&d.ticketed, 3),
                observations: 0,
                successful_connections: 0,
                cheaper_than_direct_count: 0,
                success_rate: 0.0,
                average_saving: 0.0,
                median_saving: 0.0,
                maximum_saving: 0.0,
                average_saving_percent: 0.0,
                savings: Json(json!([])),
                currency: String::new(),
                best_through_price: None,
                score: 0.0,
                last_success_at: None,
                last_cheaper_at: None,
                last_checked_at: now,
            });
            let is_new = !sqlx::query_scalar::<_, i32>(
                "SELECT id FROM hidden_city_route_stats WHERE origin=$1 AND intended=$2 AND ticketed=$3",
            )
            .bind(&row.origin)
            .bind(&row.intended)
            .bind(&row.ticketed)
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
            if is_new {
                sqlx::query(
                    "INSERT INTO hidden_city_route_stats (origin, intended, ticketed) VALUES ($1,$2,$3)",
                )
                .bind(&row.origin)
                .bind(&row.intended)
                .bind(&row.ticketed)
                .execute(&mut *tx)
                .await?;
            }
            let prev_cheaper = row.cheaper_than_direct_count;
            row.observations += d.observations;
            row.successful_connections += d.successful_connections;
            row.cheaper_than_direct_count = prev_cheaper + d.cheaper_than_direct_count;
            row.observations = row.observations.max(row.successful_connections);
            row.successful_connections = row
                .successful_connections
                .max(row.cheaper_than_direct_count);
            row.success_rate = if row.observations > 0 {
                (row.successful_connections as f64 / row.observations as f64 * 10000.0).round()
                    / 10000.0
            } else {
                0.0
            };
            if !d.savings.is_empty() {
                let merged = merge_savings(&json_f64s(&row.savings.0), &d.savings);
                let (avg, med, mx) = summarize_savings(&merged);
                row.savings = Json(json!(merged));
                row.average_saving = avg;
                row.median_saving = med;
                row.maximum_saving = mx;
                let n_new = d.saving_pcts.len() as i32;
                let total = prev_cheaper + n_new;
                row.average_saving_percent = if total > 0 {
                    ((row.average_saving_percent * prev_cheaper as f64
                        + d.saving_pcts.iter().sum::<f64>())
                        / total as f64
                        * 100.0)
                        .round()
                        / 100.0
                } else {
                    0.0
                };
                if !d.currency.is_empty() {
                    row.currency = d.currency.clone();
                }
                row.last_cheaper_at = Some(now);
            }
            if let Some(best) = d.best_through_price {
                if row.best_through_price.map(|p| best < p).unwrap_or(true) {
                    row.best_through_price = Some(best);
                }
            }
            if d.success {
                row.last_success_at = Some(now);
            }
            row.last_checked_at = now;
            let stat = RouteStat {
                origin: row.origin.clone(),
                intended: row.intended.clone(),
                ticketed: row.ticketed.clone(),
                observations: row.observations,
                successful_connections: row.successful_connections,
                cheaper_than_direct_count: row.cheaper_than_direct_count,
                median_saving: row.median_saving,
                average_saving_percent: row.average_saving_percent,
                last_success_at: row.last_success_at,
                last_cheaper_at: row.last_cheaper_at,
            };
            let (score, _) = score_candidate(Some(&stat), 0.0, 0.5, now);
            row.score = score;
            sqlx::query(
                "UPDATE hidden_city_route_stats SET observations=$1, successful_connections=$2, \
                 cheaper_than_direct_count=$3, success_rate=$4, average_saving=$5, median_saving=$6, \
                 maximum_saving=$7, average_saving_percent=$8, savings=$9, currency=$10, \
                 best_through_price=$11, score=$12, last_success_at=$13, last_cheaper_at=$14, last_checked_at=$15 \
                 WHERE origin=$16 AND intended=$17 AND ticketed=$18",
            )
            .bind(row.observations)
            .bind(row.successful_connections)
            .bind(row.cheaper_than_direct_count)
            .bind(row.success_rate)
            .bind(row.average_saving)
            .bind(row.median_saving)
            .bind(row.maximum_saving)
            .bind(row.average_saving_percent)
            .bind(Json(&row.savings.0))
            .bind(&row.currency)
            .bind(row.best_through_price)
            .bind(row.score)
            .bind(row.last_success_at)
            .bind(row.last_cheaper_at)
            .bind(row.last_checked_at)
            .bind(&row.origin)
            .bind(&row.intended)
            .bind(&row.ticketed)
            .execute(&mut *tx)
            .await?;
            *written.get_mut("stats").unwrap() += 1;
        }
    }

    if written.values().any(|n| *n > 0) {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(written)
}

pub async fn route_stats_snapshot(
    pool: &PgPool,
    origin: &str,
    intended: &str,
    limit: i64,
) -> anyhow::Result<Vec<Value>> {
    let limit = limit.min(500);
    let rows: Vec<RouteStatRow> = if !origin.is_empty() && !intended.is_empty() {
        sqlx::query_as(
            "SELECT * FROM hidden_city_route_stats WHERE origin = $1 AND intended = $2 \
             ORDER BY score DESC, observations DESC LIMIT $3",
        )
        .bind(iata3(origin))
        .bind(iata3(intended))
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else if !origin.is_empty() {
        sqlx::query_as(
            "SELECT * FROM hidden_city_route_stats WHERE origin = $1 \
             ORDER BY score DESC, observations DESC LIMIT $2",
        )
        .bind(iata3(origin))
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else if !intended.is_empty() {
        sqlx::query_as(
            "SELECT * FROM hidden_city_route_stats WHERE intended = $1 \
             ORDER BY score DESC, observations DESC LIMIT $2",
        )
        .bind(iata3(intended))
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT * FROM hidden_city_route_stats ORDER BY score DESC, observations DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(|r| {
            json!({
                "origin": r.origin,
                "intended_destination": r.intended,
                "ticketed_destination": r.ticketed,
                "observations": r.observations,
                "successful_connections": r.successful_connections,
                "success_rate": r.success_rate,
                "cheaper_than_direct_count": r.cheaper_than_direct_count,
                "average_saving": r.average_saving,
                "median_saving": r.median_saving,
                "maximum_saving": r.maximum_saving,
                "average_saving_percent": r.average_saving_percent,
                "currency": r.currency,
                "best_through_price": r.best_through_price,
                "score": r.score,
                "last_success_at": r.last_success_at.map(|d| d.to_rfc3339()),
                "last_cheaper_at": r.last_cheaper_at.map(|d| d.to_rfc3339()),
                "last_checked_at": r.last_checked_at.to_rfc3339(),
            })
        })
        .collect())
}

pub async fn route_edges_snapshot(
    pool: &PgPool,
    origin: &str,
    limit: i64,
) -> anyhow::Result<Vec<Value>> {
    let limit = limit.min(500);
    let rows: Vec<RouteEdgeRow> = if !origin.is_empty() {
        sqlx::query_as(
            "SELECT * FROM route_edges WHERE origin = $1 ORDER BY observation_count DESC LIMIT $2",
        )
        .bind(iata3(origin))
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as("SELECT * FROM route_edges ORDER BY observation_count DESC LIMIT $1")
            .bind(limit)
            .fetch_all(pool)
            .await?
    };
    Ok(rows
        .into_iter()
        .map(|r| {
            json!({
                "origin": r.origin,
                "destination": r.dest,
                "carrier": r.carrier,
                "flight_number": r.flight_number,
                "observation_count": r.observation_count,
                "days_seen": r.days_seen,
                "first_seen": r.first_seen.to_rfc3339(),
                "last_seen": r.last_seen.to_rfc3339(),
            })
        })
        .collect())
}

pub async fn price_history(
    pool: &PgPool,
    fingerprint: &str,
    limit: i64,
) -> anyhow::Result<Vec<Value>> {
    let rows = sqlx::query(
        "SELECT price, currency, provider, observed_at, expires_at FROM fare_observations \
         WHERE fingerprint = $1 ORDER BY observed_at DESC LIMIT $2",
    )
    .bind(clip(fingerprint, 240))
    .bind(limit.min(1000))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let observed: DateTime<Utc> = r.get("observed_at");
            json!({
                "price": r.get::<f64, _>("price"),
                "currency": r.get::<String, _>("currency"),
                "provider": r.get::<String, _>("provider"),
                "observed_at": observed.to_rfc3339(),
                "expires_at": r.get::<Option<String>, _>("expires_at"),
            })
        })
        .collect())
}

pub async fn busy_city_pairs(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "SELECT origin_iata, dest_iata FROM routes WHERE stops = 0 \
         GROUP BY origin_iata, dest_iata ORDER BY COUNT(*) DESC LIMIT $1",
    )
    .bind(limit * 4)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let origin: String = row.try_get("origin_iata")?;
        let dest: String = row.try_get("dest_iata")?;
        if seen.contains(&(origin.clone(), dest.clone())) || origin == dest {
            continue;
        }
        let Some(o_ap) = get_airport(pool, &origin, false).await? else {
            continue;
        };
        let Some(d_ap) = get_airport(pool, &dest, false).await? else {
            continue;
        };
        if !o_ap.scheduled_service || !d_ap.scheduled_service {
            continue;
        }
        if !matches!(o_ap.type_.as_str(), "large_airport" | "medium_airport") {
            continue;
        }
        if !matches!(d_ap.type_.as_str(), "large_airport" | "medium_airport") {
            continue;
        }
        seen.insert((origin.clone(), dest.clone()));
        out.push((origin, dest));
        if out.len() as i64 >= limit {
            break;
        }
    }
    Ok(out)
}
