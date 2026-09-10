use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::Settings;
use crate::models::{Airport, LiveTraffic, TrackedAircraft, TrackerLink};

const POSITION_SOURCE: &[(i64, &str)] = &[(0, "ADS-B"), (1, "ASTERIX"), (2, "MLAT"), (3, "FLARM")];
const TOKEN_URL: &str =
    "https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token";

static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

pub struct OpenSkyProvider {
    client_id: String,
    client_secret: String,
    client: reqwest::Client,
}

impl OpenSkyProvider {
    pub fn new(settings: &Settings, client: reqwest::Client) -> Self {
        Self {
            client_id: settings.opensky_client_id.clone(),
            client_secret: settings.opensky_client_secret.clone(),
            client,
        }
    }

    pub async fn traffic_near(&self, airport: &Airport) -> anyhow::Result<LiveTraffic> {
        self.traffic_near_radius(airport, 0.55).await
    }

    pub async fn traffic_near_radius(
        &self,
        airport: &Airport,
        radius_deg: f64,
    ) -> anyhow::Result<LiveTraffic> {
        let mut params = HashMap::new();
        params.insert("lamin", (airport.lat - radius_deg).to_string());
        params.insert("lamax", (airport.lat + radius_deg).to_string());
        params.insert("lomin", (airport.lon - radius_deg).to_string());
        params.insert("lomax", (airport.lon + radius_deg).to_string());
        let headers = self.headers().await;
        let mut req = self
            .client
            .get("https://opensky-network.org/api/states/all")
            .query(&params)
            .timeout(Duration::from_secs(25));
        if let Some(h) = headers {
            req = req.header("Authorization", h);
        }
        let r = req.send().await?;
        let note = "OpenSky state vectors: live transponders, not tickets. Anonymous access is current time only (~10s resolution). position_source 0=ADS-B 1=ASTERIX 2=MLAT 3=FLARM.";
        if r.status().as_u16() == 429 {
            return Ok(self.empty(
                airport,
                "OpenSky rate-limited (anonymous credit bucket). Retry shortly.",
            ));
        }
        if r.status().as_u16() >= 400 {
            return Ok(self.empty(
                airport,
                &format!("OpenSky /states/all returned HTTP {}.", r.status().as_u16()),
            ));
        }
        let body: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
        let api_time = body.get("time").and_then(|v| v.as_i64());
        let mut aircraft = Vec::new();
        let states = body.get("states").and_then(|v| v.as_array());
        for row in states.into_iter().flatten() {
            let arr = match row.as_array() {
                Some(a) if a.len() >= 12 => a,
                _ => continue,
            };
            let src = if arr.len() > 16 {
                arr[16].as_i64()
            } else {
                None
            };
            aircraft.push(TrackedAircraft {
                icao24: arr[0].as_str().unwrap_or("").to_lowercase(),
                callsign: arr[1]
                    .as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                origin_country: arr[2].as_str().map(|s| s.to_string()),
                lon: arr[5].as_f64(),
                lat: arr[6].as_f64(),
                baro_altitude_m: arr[7].as_f64(),
                on_ground: arr[8].as_bool().unwrap_or(false),
                velocity_ms: arr[9].as_f64(),
                true_track: arr[10].as_f64(),
                position_source: src.map(|n| n as i32),
                position_source_name: src
                    .and_then(|n| {
                        POSITION_SOURCE
                            .iter()
                            .find(|(k, _)| *k == n)
                            .map(|(_, n)| (*n).to_string())
                    })
                    .unwrap_or_else(|| "unknown".into()),
                airline_iata: None,
                airline_name: None,
            });
        }
        aircraft.sort_by(|a, b| {
            a.on_ground.cmp(&b.on_ground).then_with(|| {
                b.baro_altitude_m
                    .unwrap_or(0.0)
                    .partial_cmp(&a.baro_altitude_m.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        aircraft.truncate(40);
        let icao = airport
            .icao
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| airport.iata.clone());
        Ok(LiveTraffic {
            airport: airport.iata.clone(),
            source: "opensky".into(),
            layer: "live-track".into(),
            api_time,
            note: note.into(),
            aircraft,
            trackers: tracker_links(&airport.iata, &icao),
        })
    }

    async fn headers(&self) -> Option<String> {
        if self.client_id.is_empty() || self.client_secret.is_empty() {
            return None;
        }
        {
            let g = crate::mutex_lock(&TOKEN);
            if let Some((tok, exp)) = g.as_ref() {
                if Instant::now() < *exp {
                    return Some(format!("Bearer {tok}"));
                }
            }
        }
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
        ];
        let r = self
            .client
            .post(TOKEN_URL)
            .form(&form)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .ok()?;
        if r.status().as_u16() >= 400 {
            return None;
        }
        let body: serde_json::Value = r.json().await.ok()?;
        let token = body.get("access_token")?.as_str()?.to_string();
        let expires = body
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(1700);
        *crate::mutex_lock(&TOKEN) = Some((
            token.clone(),
            Instant::now() + Duration::from_secs(expires as u64) - Duration::from_secs(30),
        ));
        Some(format!("Bearer {token}"))
    }

    fn empty(&self, airport: &Airport, note: &str) -> LiveTraffic {
        let icao = airport
            .icao
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| airport.iata.clone());
        LiveTraffic {
            airport: airport.iata.clone(),
            source: "opensky".into(),
            layer: "live-track".into(),
            api_time: Some(chrono::Utc::now().timestamp()),
            note: note.into(),
            aircraft: vec![],
            trackers: tracker_links(&airport.iata, &icao),
        }
    }
}

pub fn tracker_links(iata: &str, icao: &str) -> Vec<TrackerLink> {
    let iata = iata.to_uppercase();
    let icao = if icao.is_empty() {
        iata.clone()
    } else {
        icao.to_uppercase()
    };
    vec![
        TrackerLink {
            id: "opensky".into(),
            name: "OpenSky Network".into(),
            layer: "live-track".into(),
            role: "Crowd ADS-B/MLAT state vectors. This is tracking, not booking.".into(),
            url: "https://opensky-network.org/".into(),
        },
        TrackerLink {
            id: "flightradar24".into(),
            name: "Flightradar24 departures".into(),
            layer: "live-track-link".into(),
            role: "Commercial tracker board. We do not ingest their feed.".into(),
            url: format!(
                "https://www.flightradar24.com/airport/{}/departures",
                iata.to_lowercase()
            ),
        },
        TrackerLink {
            id: "flightaware".into(),
            name: "FlightAware airport".into(),
            layer: "live-track-link".into(),
            role: "Commercial status/tracker. We do not call AeroAPI.".into(),
            url: format!("https://www.flightaware.com/live/airport/{icao}"),
        },
    ]
}
