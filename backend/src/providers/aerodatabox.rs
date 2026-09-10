use std::time::Duration;

use serde_json::Value;

use crate::config::Settings;
use crate::models::BoardFlight;

pub struct AeroDataBoxProvider {
    key: String,
    client: reqwest::Client,
}

impl AeroDataBoxProvider {
    pub fn new(settings: &Settings, client: reqwest::Client) -> Self {
        Self {
            key: settings.rapidapi_key.clone(),
            client,
        }
    }

    pub async fn board(&self, iata: &str, direction: &str) -> anyhow::Result<Vec<BoardFlight>> {
        let r = self
            .client
            .get(format!("https://aerodatabox.p.rapidapi.com/flights/airports/iata/{iata}"))
            .query(&[
                ("offsetMinutes", "-120"),
                ("durationMinutes", "720"),
                ("withLeg", "true"),
                ("direction", direction),
                ("withCancelled", "true"),
                ("withCodeshared", "true"),
                ("withCargo", "false"),
                ("withPrivate", "false"),
            ])
            .header("X-RapidAPI-Key", &self.key)
            .header("X-RapidAPI-Host", "aerodatabox.p.rapidapi.com")
            .timeout(Duration::from_secs(25))
            .send()
            .await?;
        if r.status().as_u16() >= 400 {
            return Ok(vec![]);
        }
        let body: Value = r.json().await.unwrap_or(serde_json::json!({}));
        let key = if direction.to_lowercase().starts_with("dep") {
            "departures"
        } else {
            "arrivals"
        };
        let mut flights = Vec::new();
        for raw in body.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            if let Some(parsed) = one(&raw, iata, direction) {
                flights.push(parsed);
            }
        }
        flights.truncate(40);
        Ok(flights)
    }
}

fn one(raw: &Value, iata: &str, direction: &str) -> Option<BoardFlight> {
    let number = raw
        .get("number")
        .or_else(|| raw.get("callSign"))
        .and_then(|v| v.as_str())?;
    let airline = raw
        .get("airline")
        .and_then(|a| a.get("iata").or_else(|| a.get("name")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let dep = raw.get("departure").or_else(|| raw.get("movement")).cloned().unwrap_or(serde_json::json!({}));
    let arr = raw.get("arrival").cloned().unwrap_or(serde_json::json!({}));
    let (origin, dest, sched, est, terminal, gate) = if direction.to_lowercase().starts_with("dep") {
        let other = arr.get("airport").and_then(|a| a.get("iata")).and_then(|v| v.as_str());
        let sched = dep
            .get("scheduledTime")
            .and_then(|t| t.get("local"))
            .or_else(|| dep.get("scheduledTimeLocal"));
        let est = dep
            .get("revisedTime")
            .and_then(|t| t.get("local"))
            .or_else(|| dep.get("revisedTimeLocal"));
        (
            Some(iata.to_string()),
            other.map(|s| s.to_string()),
            sched.and_then(|v| v.as_str()).map(|s| s.chars().take(19).collect()),
            est.and_then(|v| v.as_str()).map(|s| s.chars().take(19).collect()),
            dep.get("terminal").and_then(|v| v.as_str()).map(|s| s.to_string()),
            dep.get("gate").and_then(|v| v.as_str()).map(|s| s.to_string()),
        )
    } else {
        let other = dep.get("airport").and_then(|a| a.get("iata")).and_then(|v| v.as_str());
        let sched = arr
            .get("scheduledTime")
            .and_then(|t| t.get("local"))
            .or_else(|| arr.get("scheduledTimeLocal"));
        let est = arr
            .get("revisedTime")
            .and_then(|t| t.get("local"))
            .or_else(|| arr.get("revisedTimeLocal"));
        (
            other.map(|s| s.to_string()),
            Some(iata.to_string()),
            sched.and_then(|v| v.as_str()).map(|s| s.chars().take(19).collect()),
            est.and_then(|v| v.as_str()).map(|s| s.chars().take(19).collect()),
            arr.get("terminal").and_then(|v| v.as_str()).map(|s| s.to_string()),
            arr.get("gate").and_then(|v| v.as_str()).map(|s| s.to_string()),
        )
    };
    let status = raw.get("status").and_then(|v| {
        if v.is_string() {
            v.as_str().map(|s| s.to_string())
        } else {
            Some(v.to_string())
        }
    });
    Some(BoardFlight {
        flight_number: number.to_string(),
        carrier: airline,
        origin,
        dest,
        scheduled: sched,
        estimated: est,
        status: status.filter(|s| !s.is_empty()),
        terminal: terminal.filter(|s| !s.is_empty()),
        gate: gate.filter(|s| !s.is_empty()),
        source: "aerodatabox".into(),
        layer: "schedule-status".into(),
    })
}
