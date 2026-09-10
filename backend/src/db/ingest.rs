use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::time::Duration;

use chrono::{DateTime, Utc};
use csv::ReaderBuilder;
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool, Postgres, QueryBuilder};

const OURAIRPORTS: &str = "https://davidmegginson.github.io/ourairports-data";
const GEONAMES_COUNTRIES: &str = "https://download.geonames.org/export/dump/countryInfo.txt";
const OPENFLIGHTS_AIRLINES: &str =
    "https://raw.githubusercontent.com/jpatokal/openflights/master/data/airlines.dat";
const OPENFLIGHTS_ROUTES: &str =
    "https://raw.githubusercontent.com/jpatokal/openflights/master/data/routes.dat";

const CONTINENT_NAMES: &[(&str, &str)] = &[
    ("AF", "Africa"),
    ("AN", "Antarctica"),
    ("AS", "Asia"),
    ("EU", "Europe"),
    ("NA", "North America"),
    ("OC", "Oceania"),
    ("SA", "South America"),
];

const ROUTE_NOTE: &str = "OpenFlights Airline Route Mapper extract. Dataset last published around 2014–2017; \
not a current schedule, not availability, not a fare.";
const AIRLINE_NOTE: &str = "OpenFlights airlines dump (last material update ~2017). Use IATA/ICAO as identifiers, \
not as proof the carrier still operates a given route.";

fn type_rank(typ: &str) -> i32 {
    match typ {
        "large_airport" => 0,
        "medium_airport" => 1,
        "small_airport" => 2,
        _ => 9,
    }
}

fn parse_int(v: Option<&str>) -> Option<i32> {
    let v = v?.trim();
    if v.is_empty() || v == "\\N" {
        return None;
    }
    v.parse::<f64>().ok().map(|n| n as i32)
}

fn parse_float(v: Option<&str>) -> Option<f64> {
    let v = v?.trim();
    if v.is_empty() || v == "\\N" {
        return None;
    }
    v.parse().ok()
}

fn continent_name(code: &str) -> String {
    CONTINENT_NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| (*n).to_string())
        .unwrap_or_else(|| code.to_string())
}

struct GeoCountry {
    iso3: Option<String>,
    iso_numeric: Option<String>,
    name: String,
    capital: String,
    area_km2: Option<f64>,
    population: Option<i32>,
    continent: String,
    tld: String,
    currency_code: String,
    currency_name: String,
    phone: String,
    languages: String,
    geoname_id: Option<i32>,
}

struct AirportCand {
    iata: String,
    icao: Option<String>,
    ident: Option<String>,
    name: String,
    municipality: String,
    iso_country: String,
    iso_region: String,
    continent: String,
    lat: f64,
    lon: f64,
    elevation_ft: Option<i32>,
    type_: String,
    scheduled_service: bool,
    wikipedia: Option<String>,
    home_link: Option<String>,
    gps_code: Option<String>,
    local_code: Option<String>,
    keywords: Option<String>,
    rank: (i32, i32),
}

struct CountryRec {
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
}

struct RegionRec {
    code: String,
    local_code: String,
    name: String,
    iso_country: String,
    continent: String,
    wikipedia: Option<String>,
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    let r = client.get(url).send().await?.error_for_status()?;
    Ok(r.text().await?)
}

fn geonames(text: &str) -> HashMap<String, GeoCountry> {
    let mut out = HashMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 15 {
            continue;
        }
        let iso2 = cols[0].trim().to_uppercase();
        if iso2.len() != 2 {
            continue;
        }
        out.insert(
            iso2,
            GeoCountry {
                iso3: {
                    let s = cols[1].trim().to_uppercase();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                },
                iso_numeric: {
                    let s = cols[2].trim();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                },
                name: cols[4].trim().to_string(),
                capital: cols[5].trim().to_string(),
                area_km2: parse_float(Some(cols[6])),
                population: parse_int(Some(cols[7])),
                continent: cols[8].trim().to_uppercase(),
                tld: cols[9].trim().to_string(),
                currency_code: cols[10].trim().to_uppercase(),
                currency_name: cols[11].trim().to_string(),
                phone: cols[12].trim().to_string(),
                languages: if cols.len() > 15 {
                    cols[15].trim().to_string()
                } else {
                    String::new()
                },
                geoname_id: if cols.len() > 16 {
                    parse_int(Some(cols[16]))
                } else {
                    None
                },
            },
        );
    }
    out
}

fn nonempty(s: Option<&str>) -> Option<String> {
    let t = s?.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn row_map(headers: &csv::StringRecord, rec: &csv::StringRecord) -> HashMap<String, String> {
    headers
        .iter()
        .zip(rec.iter())
        .map(|(h, v)| (h.to_string(), v.to_string()))
        .collect()
}

fn csv_rows(text: &str) -> anyhow::Result<Vec<HashMap<String, String>>> {
    let mut reader = ReaderBuilder::new()
        .flexible(true)
        .from_reader(Cursor::new(text));
    let headers = reader.headers()?.clone();
    let mut out = Vec::new();
    for rec in reader.records() {
        let rec = rec?;
        out.push(row_map(&headers, &rec));
    }
    Ok(out)
}

fn best_airports(text: &str) -> anyhow::Result<HashMap<String, AirportCand>> {
    let mut best: HashMap<String, AirportCand> = HashMap::new();
    for row in csv_rows(text)? {
        let iata = row
            .get("iata_code")
            .map(|s| s.trim().to_uppercase())
            .unwrap_or_default();
        if iata.len() != 3 || !iata.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let lat = parse_float(row.get("latitude_deg").map(|s| s.as_str()));
        let lon = parse_float(row.get("longitude_deg").map(|s| s.as_str()));
        let (Some(lat), Some(lon)) = (lat, lon) else {
            continue;
        };
        let typ = row.get("type").cloned().unwrap_or_default();
        let scheduled = row
            .get("scheduled_service")
            .map(|s| s.eq_ignore_ascii_case("yes"))
            .unwrap_or(false);
        let rank = (if scheduled { 0 } else { 1 }, type_rank(&typ));
        let icao = nonempty(row.get("icao_code").map(|s| s.as_str()))
            .or_else(|| nonempty(row.get("gps_code").map(|s| s.as_str())))
            .map(|s| s.to_uppercase());
        let cand = AirportCand {
            iata: iata.clone(),
            icao,
            ident: nonempty(row.get("ident").map(|s| s.as_str())),
            name: nonempty(row.get("name").map(|s| s.as_str())).unwrap_or_else(|| iata.clone()),
            municipality: row
                .get("municipality")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            iso_country: row
                .get("iso_country")
                .map(|s| s.trim().to_uppercase())
                .unwrap_or_default(),
            iso_region: row
                .get("iso_region")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            continent: row
                .get("continent")
                .map(|s| s.trim().to_uppercase())
                .unwrap_or_default(),
            lat,
            lon,
            elevation_ft: parse_int(row.get("elevation_ft").map(|s| s.as_str())),
            type_: if typ.is_empty() {
                "unknown".into()
            } else {
                typ
            },
            scheduled_service: scheduled,
            wikipedia: nonempty(row.get("wikipedia_link").map(|s| s.as_str())),
            home_link: nonempty(row.get("home_link").map(|s| s.as_str())),
            gps_code: nonempty(row.get("gps_code").map(|s| s.as_str())).map(|s| s.to_uppercase()),
            local_code: nonempty(row.get("local_code").map(|s| s.as_str())),
            keywords: nonempty(row.get("keywords").map(|s| s.as_str())),
            rank,
        };
        match best.get(&iata) {
            Some(prev) if prev.rank <= cand.rank => {}
            _ => {
                best.insert(iata, cand);
            }
        }
    }
    Ok(best)
}

async fn count_table(pool: &PgPool, table: &str) -> anyhow::Result<i64> {
    let sql = format!("SELECT count(*) FROM {table}");
    let n: (i64,) = sqlx::query_as(&sql).fetch_one(pool).await?;
    Ok(n.0)
}

pub async fn ingest(pool: &PgPool) -> anyhow::Result<Value> {
    let now = Utc::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let countries_url = format!("{OURAIRPORTS}/countries.csv");
    let regions_url = format!("{OURAIRPORTS}/regions.csv");
    let airports_url = format!("{OURAIRPORTS}/airports.csv");
    let runways_url = format!("{OURAIRPORTS}/runways.csv");
    let navaids_url = format!("{OURAIRPORTS}/navaids.csv");
    let (oa_countries, oa_regions, oa_airports, oa_runways, oa_navaids, geo_raw, airlines_dat, routes_dat) =
        tokio::try_join!(
            fetch_text(&client, &countries_url),
            fetch_text(&client, &regions_url),
            fetch_text(&client, &airports_url),
            fetch_text(&client, &runways_url),
            fetch_text(&client, &navaids_url),
            fetch_text(&client, GEONAMES_COUNTRIES),
            fetch_text(&client, OPENFLIGHTS_AIRLINES),
            fetch_text(&client, OPENFLIGHTS_ROUTES),
        )?;
    let geo = geonames(&geo_raw);
    let parsed = best_airports(&oa_airports)?;

    let mut tx = pool.begin().await?;
    for table in [
        "navaids",
        "runways",
        "routes",
        "airlines",
        "airports",
        "regions",
        "countries",
        "continents",
    ] {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&mut *tx)
            .await?;
    }

    let mut countries: HashMap<String, CountryRec> = HashMap::new();
    {
        for row in csv_rows(&oa_countries)? {
            let iso2 = row
                .get("code")
                .map(|s| s.trim().to_uppercase())
                .unwrap_or_default();
            if iso2.len() != 2 {
                continue;
            }
            let g = geo.get(&iso2);
            let sources = if g.is_some() {
                "ourairports+geonames"
            } else {
                "ourairports"
            };
            let name = row
                .get("name")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| g.map(|x| x.name.clone()))
                .unwrap_or_else(|| iso2.clone());
            countries.insert(
                iso2.clone(),
                CountryRec {
                    iso2,
                    iso3: g.and_then(|x| x.iso3.clone()),
                    iso_numeric: g.and_then(|x| x.iso_numeric.clone()),
                    name,
                    continent: row
                        .get("continent")
                        .map(|s| s.trim().to_uppercase())
                        .filter(|s| !s.is_empty())
                        .or_else(|| g.map(|x| x.continent.clone()))
                        .unwrap_or_default(),
                    capital: g.map(|x| x.capital.clone()).unwrap_or_default(),
                    currency_code: g.map(|x| x.currency_code.clone()).unwrap_or_default(),
                    currency_name: g.map(|x| x.currency_name.clone()).unwrap_or_default(),
                    tld: g.map(|x| x.tld.clone()).unwrap_or_default(),
                    phone: g.map(|x| x.phone.clone()).unwrap_or_default(),
                    languages: g.map(|x| x.languages.clone()).unwrap_or_default(),
                    population: g.and_then(|x| x.population),
                    area_km2: g.and_then(|x| x.area_km2),
                    geoname_id: g.and_then(|x| x.geoname_id),
                    wikipedia: nonempty(row.get("wikipedia_link").map(|s| s.as_str())),
                    sources: sources.to_string(),
                },
            );
        }
    }
    for (iso2, g) in &geo {
        if countries.contains_key(iso2) {
            continue;
        }
        countries.insert(
            iso2.clone(),
            CountryRec {
                iso2: iso2.clone(),
                iso3: g.iso3.clone(),
                iso_numeric: g.iso_numeric.clone(),
                name: if g.name.is_empty() {
                    iso2.clone()
                } else {
                    g.name.clone()
                },
                continent: g.continent.clone(),
                capital: g.capital.clone(),
                currency_code: g.currency_code.clone(),
                currency_name: g.currency_name.clone(),
                tld: g.tld.clone(),
                phone: g.phone.clone(),
                languages: g.languages.clone(),
                population: g.population,
                area_km2: g.area_km2,
                geoname_id: g.geoname_id,
                wikipedia: None,
                sources: "geonames".into(),
            },
        );
    }

    insert_countries(&mut *tx, countries.values().collect(), now).await?;

    let mut continent_codes: Vec<String> = countries
        .values()
        .map(|c| c.continent.clone())
        .filter(|c| !c.is_empty())
        .collect();
    continent_codes.sort();
    continent_codes.dedup();
    if !continent_codes.is_empty() {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("INSERT INTO continents (code, name, source) ");
        qb.push_values(continent_codes.iter(), |mut b, code| {
            b.push_bind(code)
                .push_bind(continent_name(code))
                .push_bind("ourairports+geonames");
        });
        qb.build().execute(&mut *tx).await?;
    }

    let mut regions: HashMap<String, RegionRec> = HashMap::new();
    {
        for row in csv_rows(&oa_regions)? {
            let code = row.get("code").map(|s| s.trim().to_string()).unwrap_or_default();
            if code.is_empty() {
                continue;
            }
            regions.insert(
                code.clone(),
                RegionRec {
                    code: code.clone(),
                    local_code: row
                        .get("local_code")
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                    name: nonempty(row.get("name").map(|s| s.as_str())).unwrap_or(code),
                    iso_country: row
                        .get("iso_country")
                        .map(|s| s.trim().to_uppercase())
                        .unwrap_or_default(),
                    continent: row
                        .get("continent")
                        .map(|s| s.trim().to_uppercase())
                        .unwrap_or_default(),
                    wikipedia: nonempty(row.get("wikipedia_link").map(|s| s.as_str())),
                },
            );
        }
    }
    insert_regions(&mut *tx, regions.values().collect()).await?;

    let mut ident_to_iata: HashMap<String, String> = HashMap::new();
    insert_airports(&mut *tx, parsed.values().collect(), &countries, &regions, now).await?;
    for a in parsed.values() {
        if let Some(ident) = &a.ident {
            ident_to_iata.insert(ident.clone(), a.iata.clone());
        }
    }
    let iata_set: HashSet<String> = parsed.keys().cloned().collect();

    insert_runways(&mut *tx, &oa_runways, &ident_to_iata).await?;
    insert_navaids(&mut *tx, &oa_navaids, &ident_to_iata).await?;
    insert_airlines(&mut *tx, &airlines_dat).await?;
    insert_routes(&mut *tx, &routes_dat, &iata_set).await?;
    tx.commit().await?;

    for table in [
        "continents",
        "countries",
        "regions",
        "airports",
        "runways",
        "navaids",
        "airlines",
        "routes",
    ] {
        sqlx::query(&format!("ANALYZE {table}"))
            .execute(pool)
            .await?;
    }

    Ok(json!({
        "continents": count_table(pool, "continents").await?,
        "countries": count_table(pool, "countries").await?,
        "regions": count_table(pool, "regions").await?,
        "airports": count_table(pool, "airports").await?,
        "runways": count_table(pool, "runways").await?,
        "navaids": count_table(pool, "navaids").await?,
        "airlines": count_table(pool, "airlines").await?,
        "routes": count_table(pool, "routes").await?,
    }))
}

async fn insert_countries(
    conn: &mut PgConnection,
    rows: Vec<&CountryRec>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    for chunk in rows.chunks(500) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO countries (iso2, iso3, iso_numeric, name, continent, capital, \
             currency_code, currency_name, tld, phone, languages, population, area_km2, \
             geoname_id, wikipedia, sources, ingested_at) ",
        );
        qb.push_values(chunk.iter(), |mut b, r| {
            b.push_bind(&r.iso2)
                .push_bind(&r.iso3)
                .push_bind(&r.iso_numeric)
                .push_bind(&r.name)
                .push_bind(&r.continent)
                .push_bind(&r.capital)
                .push_bind(&r.currency_code)
                .push_bind(&r.currency_name)
                .push_bind(&r.tld)
                .push_bind(&r.phone)
                .push_bind(&r.languages)
                .push_bind(r.population)
                .push_bind(r.area_km2)
                .push_bind(r.geoname_id)
                .push_bind(&r.wikipedia)
                .push_bind(&r.sources)
                .push_bind(now);
        });
        qb.build().execute(&mut *conn).await?;
    }
    Ok(())
}

async fn insert_regions(conn: &mut PgConnection, rows: Vec<&RegionRec>) -> anyhow::Result<()> {
    for chunk in rows.chunks(1000) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO regions (code, local_code, name, iso_country, continent, wikipedia, source) ",
        );
        qb.push_values(chunk.iter(), |mut b, r| {
            b.push_bind(&r.code)
                .push_bind(&r.local_code)
                .push_bind(&r.name)
                .push_bind(&r.iso_country)
                .push_bind(&r.continent)
                .push_bind(&r.wikipedia)
                .push_bind("ourairports");
        });
        qb.build().execute(&mut *conn).await?;
    }
    Ok(())
}

async fn insert_airports(
    conn: &mut PgConnection,
    rows: Vec<&AirportCand>,
    countries: &HashMap<String, CountryRec>,
    regions: &HashMap<String, RegionRec>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    for chunk in rows.chunks(500) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO airports (iata, icao, ident, name, municipality, iso_country, iso_region, \
             continent, country_name, region_name, lat, lon, elevation_ft, \"type\", scheduled_service, \
             wikipedia, home_link, gps_code, local_code, keywords, source, ingested_at) ",
        );
        qb.push_values(chunk.iter(), |mut b, a| {
            let country = countries.get(&a.iso_country);
            let region = regions.get(&a.iso_region);
            let continent = if a.continent.is_empty() {
                country.map(|c| c.continent.clone()).unwrap_or_default()
            } else {
                a.continent.clone()
            };
            let country_name = country
                .map(|c| c.name.clone())
                .unwrap_or_else(|| a.iso_country.clone());
            let region_name = region.map(|r| r.name.clone()).unwrap_or_default();
            b.push_bind(&a.iata)
                .push_bind(&a.icao)
                .push_bind(&a.ident)
                .push_bind(&a.name)
                .push_bind(&a.municipality)
                .push_bind(&a.iso_country)
                .push_bind(&a.iso_region)
                .push_bind(continent)
                .push_bind(country_name)
                .push_bind(region_name)
                .push_bind(a.lat)
                .push_bind(a.lon)
                .push_bind(a.elevation_ft)
                .push_bind(&a.type_)
                .push_bind(a.scheduled_service)
                .push_bind(&a.wikipedia)
                .push_bind(&a.home_link)
                .push_bind(&a.gps_code)
                .push_bind(&a.local_code)
                .push_bind(&a.keywords)
                .push_bind("ourairports")
                .push_bind(now);
        });
        qb.build().execute(&mut *conn).await?;
    }
    Ok(())
}

async fn insert_runways(
    conn: &mut PgConnection,
    text: &str,
    ident_to_iata: &HashMap<String, String>,
) -> anyhow::Result<()> {
    let mut batch: Vec<(
        i32,
        String,
        String,
        Option<i32>,
        Option<i32>,
        String,
        bool,
        bool,
        String,
        String,
    )> = Vec::new();
    for row in csv_rows(text)? {
        let ident = row
            .get("airport_ident")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let Some(iata) = ident_to_iata.get(&ident) else {
            continue;
        };
        let Some(rid) = parse_int(row.get("id").map(|s| s.as_str())) else {
            continue;
        };
        batch.push((
            rid,
            iata.clone(),
            ident,
            parse_int(row.get("length_ft").map(|s| s.as_str())),
            parse_int(row.get("width_ft").map(|s| s.as_str())),
            row.get("surface")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            row.get("lighted").map(|s| s.trim() == "1").unwrap_or(false),
            row.get("closed").map(|s| s.trim() == "1").unwrap_or(false),
            row.get("le_ident")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            row.get("he_ident")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
        ));
        if batch.len() >= 3000 {
            flush_runways(conn, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        flush_runways(conn, &batch).await?;
    }
    Ok(())
}

async fn flush_runways(
    conn: &mut PgConnection,
    batch: &[(
        i32,
        String,
        String,
        Option<i32>,
        Option<i32>,
        String,
        bool,
        bool,
        String,
        String,
    )],
) -> anyhow::Result<()> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO runways (id, airport_iata, airport_ident, length_ft, width_ft, surface, \
         lighted, closed, le_ident, he_ident, source) ",
    );
    qb.push_values(batch, |mut b, r| {
        b.push_bind(r.0)
            .push_bind(&r.1)
            .push_bind(&r.2)
            .push_bind(r.3)
            .push_bind(r.4)
            .push_bind(&r.5)
            .push_bind(r.6)
            .push_bind(r.7)
            .push_bind(&r.8)
            .push_bind(&r.9)
            .push_bind("ourairports");
    });
    qb.build().execute(&mut *conn).await?;
    Ok(())
}

async fn insert_navaids(
    conn: &mut PgConnection,
    text: &str,
    ident_to_iata: &HashMap<String, String>,
) -> anyhow::Result<()> {
    let mut batch: Vec<(
        i32,
        String,
        String,
        String,
        Option<i32>,
        String,
        Option<String>,
        Option<f64>,
        Option<f64>,
    )> = Vec::new();
    for row in csv_rows(text)? {
        let assoc = row
            .get("associated_airport")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if assoc.is_empty() || !ident_to_iata.contains_key(&assoc) {
            continue;
        }
        let Some(nid) = parse_int(row.get("id").map(|s| s.as_str())) else {
            continue;
        };
        batch.push((
            nid,
            row.get("ident")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            row.get("name")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            row.get("type")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            parse_int(row.get("frequency_khz").map(|s| s.as_str())),
            row.get("iso_country")
                .map(|s| s.trim().to_uppercase())
                .unwrap_or_default(),
            if assoc.is_empty() {
                None
            } else {
                Some(assoc)
            },
            parse_float(row.get("latitude_deg").map(|s| s.as_str())),
            parse_float(row.get("longitude_deg").map(|s| s.as_str())),
        ));
        if batch.len() >= 3000 {
            flush_navaids(conn, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        flush_navaids(conn, &batch).await?;
    }
    Ok(())
}

async fn flush_navaids(
    conn: &mut PgConnection,
    batch: &[(
        i32,
        String,
        String,
        String,
        Option<i32>,
        String,
        Option<String>,
        Option<f64>,
        Option<f64>,
    )],
) -> anyhow::Result<()> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO navaids (id, ident, name, \"type\", frequency_khz, iso_country, \
         associated_airport, lat, lon, source) ",
    );
    qb.push_values(batch, |mut b, r| {
        b.push_bind(r.0)
            .push_bind(&r.1)
            .push_bind(&r.2)
            .push_bind(&r.3)
            .push_bind(r.4)
            .push_bind(&r.5)
            .push_bind(&r.6)
            .push_bind(r.7)
            .push_bind(r.8)
            .push_bind("ourairports");
    });
    qb.build().execute(&mut *conn).await?;
    Ok(())
}

async fn insert_airlines(conn: &mut PgConnection, text: &str) -> anyhow::Result<()> {
    let mut batch: Vec<(Option<String>, Option<String>, String, String, bool)> = Vec::new();
    for line in text.lines() {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(Cursor::new(line));
        let Some(Ok(cols)) = rdr.records().next() else {
            continue;
        };
        if cols.len() < 8 {
            continue;
        }
        let mut iata = cols[3].trim().to_uppercase();
        let mut icao = cols[4].trim().to_uppercase();
        if iata.is_empty() || iata == "\\N" || iata == "N/A" {
            iata.clear();
        }
        if icao.is_empty() || icao == "\\N" || icao == "N/A" {
            icao.clear();
        }
        if iata.is_empty() && icao.is_empty() {
            continue;
        }
        let name = {
            let n = cols[1].trim();
            if n.is_empty() {
                if !iata.is_empty() {
                    iata.clone()
                } else if !icao.is_empty() {
                    icao.clone()
                } else {
                    "unknown".into()
                }
            } else {
                n.to_string()
            }
        };
        let country = if cols.len() > 6 {
            cols[6].trim().to_string()
        } else {
            String::new()
        };
        let active = if cols.len() > 7 {
            cols[7].trim().eq_ignore_ascii_case("Y")
        } else {
            true
        };
        batch.push((
            if iata.is_empty() { None } else { Some(iata) },
            if icao.is_empty() { None } else { Some(icao) },
            name,
            country,
            active,
        ));
        if batch.len() >= 2000 {
            flush_airlines(conn, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        flush_airlines(conn, &batch).await?;
    }
    Ok(())
}

async fn flush_airlines(
    conn: &mut PgConnection,
    batch: &[(Option<String>, Option<String>, String, String, bool)],
) -> anyhow::Result<()> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO airlines (iata, icao, name, country, active, source, source_note) ",
    );
    qb.push_values(batch, |mut b, r| {
        b.push_bind(&r.0)
            .push_bind(&r.1)
            .push_bind(&r.2)
            .push_bind(&r.3)
            .push_bind(r.4)
            .push_bind("openflights")
            .push_bind(AIRLINE_NOTE);
    });
    qb.build().execute(&mut *conn).await?;
    Ok(())
}

async fn insert_routes(
    conn: &mut PgConnection,
    text: &str,
    iata_set: &HashSet<String>,
) -> anyhow::Result<()> {
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut batch: Vec<(String, String, String, bool, i32, String)> = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 6 {
            continue;
        }
        let airline = cols[0].trim().to_uppercase();
        let origin = cols[2].trim().to_uppercase();
        let dest = cols[4].trim().to_uppercase();
        if origin.len() != 3 || dest.len() != 3 {
            continue;
        }
        if !iata_set.contains(&origin) || !iata_set.contains(&dest) || origin == dest {
            continue;
        }
        if airline.len() > 3 || airline.is_empty() || airline == "\\N" {
            continue;
        }
        let key = (airline.clone(), origin.clone(), dest.clone());
        if !seen.insert(key) {
            continue;
        }
        let codeshare = cols.len() > 6 && cols[6].trim() == "Y";
        let stops = if cols.len() > 7 {
            parse_int(Some(cols[7])).unwrap_or(0)
        } else {
            0
        };
        let equipment = if cols.len() > 8 {
            cols[8].trim().to_string()
        } else {
            String::new()
        };
        batch.push((airline, origin, dest, codeshare, stops, equipment));
        if batch.len() >= 2000 {
            flush_routes(conn, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        flush_routes(conn, &batch).await?;
    }
    Ok(())
}

async fn flush_routes(
    conn: &mut PgConnection,
    batch: &[(String, String, String, bool, i32, String)],
) -> anyhow::Result<()> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO routes (airline_iata, origin_iata, dest_iata, codeshare, stops, equipment, source, source_note) ",
    );
    qb.push_values(batch, |mut b, r| {
        b.push_bind(&r.0)
            .push_bind(&r.1)
            .push_bind(&r.2)
            .push_bind(r.3)
            .push_bind(r.4)
            .push_bind(&r.5)
            .push_bind("openflights")
            .push_bind(ROUTE_NOTE);
    });
    qb.build().execute(&mut *conn).await?;
    Ok(())
}
