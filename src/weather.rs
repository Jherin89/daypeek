//! Weather via Open-Meteo (free, no API key): geocoding + forecast.

use serde::{Deserialize, Serialize};

use crate::cache::Cache;
use crate::config::{Units, WeatherConfig};
use crate::error::{Error, Result};
use crate::http::{enc, Http};
use crate::model::Weather;

/// Human-readable condition and a single-width glyph for a WMO weather code.
pub fn describe(code: u8) -> (&'static str, &'static str) {
    match code {
        0 => ("Clear", "☀"),
        1 => ("Mostly clear", "☀"),
        2 => ("Partly cloudy", "⛅"),
        3 => ("Overcast", "☁"),
        45 | 48 => ("Fog", "≋"),
        51 | 53 | 55 => ("Drizzle", "☂"),
        56 | 57 => ("Freezing drizzle", "☂"),
        61 => ("Light rain", "☂"),
        63 => ("Rain", "☂"),
        65 => ("Heavy rain", "☂"),
        66 | 67 => ("Freezing rain", "☂"),
        71 => ("Light snow", "❄"),
        73 => ("Snow", "❄"),
        75 => ("Heavy snow", "❄"),
        77 => ("Snow grains", "❄"),
        80..=82 => ("Showers", "☂"),
        85 | 86 => ("Snow showers", "❄"),
        95 => ("Thunderstorm", "⚡"),
        96 | 99 => ("Thunderstorm, hail", "⚡"),
        _ => ("Unknown", "?"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Deserialize)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<GeoResult>,
}

#[derive(Deserialize, Clone)]
struct GeoResult {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    admin1: Option<String>,
}

const US_STATES: &[(&str, &str)] = &[
    ("AL", "Alabama"),
    ("AK", "Alaska"),
    ("AZ", "Arizona"),
    ("AR", "Arkansas"),
    ("CA", "California"),
    ("CO", "Colorado"),
    ("CT", "Connecticut"),
    ("DE", "Delaware"),
    ("FL", "Florida"),
    ("GA", "Georgia"),
    ("HI", "Hawaii"),
    ("ID", "Idaho"),
    ("IL", "Illinois"),
    ("IN", "Indiana"),
    ("IA", "Iowa"),
    ("KS", "Kansas"),
    ("KY", "Kentucky"),
    ("LA", "Louisiana"),
    ("ME", "Maine"),
    ("MD", "Maryland"),
    ("MA", "Massachusetts"),
    ("MI", "Michigan"),
    ("MN", "Minnesota"),
    ("MS", "Mississippi"),
    ("MO", "Missouri"),
    ("MT", "Montana"),
    ("NE", "Nebraska"),
    ("NV", "Nevada"),
    ("NH", "New Hampshire"),
    ("NJ", "New Jersey"),
    ("NM", "New Mexico"),
    ("NY", "New York"),
    ("NC", "North Carolina"),
    ("ND", "North Dakota"),
    ("OH", "Ohio"),
    ("OK", "Oklahoma"),
    ("OR", "Oregon"),
    ("PA", "Pennsylvania"),
    ("RI", "Rhode Island"),
    ("SC", "South Carolina"),
    ("SD", "South Dakota"),
    ("TN", "Tennessee"),
    ("TX", "Texas"),
    ("UT", "Utah"),
    ("VT", "Vermont"),
    ("VA", "Virginia"),
    ("WA", "Washington"),
    ("WV", "West Virginia"),
    ("WI", "Wisconsin"),
    ("WY", "Wyoming"),
    ("DC", "District of Columbia"),
];

/// `"Evans, Georgia"` → (`"Evans"`, `["georgia"]`); `"Evans, GA, US"` works too.
fn split_city(city: &str) -> (String, Vec<String>) {
    let mut parts = city.split(',').map(|s| s.trim()).filter(|s| !s.is_empty());
    let name = parts.next().unwrap_or("").to_string();
    let quals = parts
        .map(|q| {
            US_STATES
                .iter()
                .find(|(abbr, _)| abbr.eq_ignore_ascii_case(q))
                .map(|(_, full)| full.to_string())
                .unwrap_or_else(|| q.to_string())
                .to_lowercase()
        })
        .collect();
    (name, quals)
}

/// Choose the best geocoding hit for the user's qualifiers.
fn pick(results: &[GeoResult], quals: &[String]) -> Option<GeoResult> {
    let score = |r: &GeoResult| -> usize {
        let fields = [&r.admin1, &r.country, &r.country_code];
        quals
            .iter()
            .filter(|q| {
                fields
                    .iter()
                    .any(|f| f.as_deref().is_some_and(|v| v.eq_ignore_ascii_case(q)))
                    || (q.as_str() == "usa" && r.country_code.as_deref() == Some("US"))
            })
            .count()
    };
    // Results arrive ranked by population; keep that order among equal scores.
    let best = results.iter().map(score).max()?;
    results.iter().find(|r| score(r) == best).cloned()
}

fn geocode(http: &Http, city: &str) -> Result<Place> {
    let (name, quals) = split_city(city);
    if name.is_empty() {
        return Err(Error::Config("weather.city is empty".into()));
    }
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=10&language=en&format=json",
        enc(&name)
    );
    let resp: GeoResponse = http.get_json(&url, None)?;
    let hit = pick(&resp.results, &quals)
        .ok_or_else(|| Error::Config(format!("no place found for {city:?}")))?;
    Ok(Place {
        name: city.to_string(),
        latitude: hit.latitude,
        longitude: hit.longitude,
    })
    .map(|mut p| {
        if p.name.is_empty() {
            p.name = hit.name;
        }
        p
    })
}

#[derive(Deserialize)]
pub struct ForecastResponse {
    current: Current,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    weather_code: u8,
}

#[derive(Deserialize)]
struct Daily {
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
}

/// Turn a forecast response into our model.
pub fn from_forecast(place: &str, units: Units, f: ForecastResponse) -> Weather {
    let (condition, _) = describe(f.current.weather_code);
    let first = |v: &Vec<Option<f64>>| v.first().copied().flatten();
    Weather {
        place: place.to_string(),
        temperature: f.current.temperature_2m,
        high: first(&f.daily.temperature_2m_max).unwrap_or(f.current.temperature_2m),
        low: first(&f.daily.temperature_2m_min).unwrap_or(f.current.temperature_2m),
        code: f.current.weather_code,
        condition: condition.to_string(),
        precip_chance: first(&f.daily.precipitation_probability_max)
            .map(|p| p.round().clamp(0.0, 100.0) as u8),
        unit: match units {
            Units::Imperial => "°F".into(),
            Units::Metric => "°C".into(),
        },
    }
}

/// Resolve the place (cached for good) and fetch today's forecast.
pub fn fetch(http: &Http, cache: &Cache, cfg: &WeatherConfig) -> Result<Weather> {
    let place = match (cfg.latitude, cfg.longitude) {
        (Some(latitude), Some(longitude)) => Place {
            name: cfg
                .city
                .clone()
                .unwrap_or_else(|| format!("{latitude:.2}, {longitude:.2}")),
            latitude,
            longitude,
        },
        _ => {
            let city = cfg.city.as_deref().unwrap_or_default();
            let key = format!("geocode-{}", city.to_lowercase());
            match cache.load::<Place>(&key) {
                Some((_, p)) => p,
                None => {
                    let p = geocode(http, city)?;
                    cache.store(&key, &p);
                    p
                }
            }
        }
    };
    let unit = match cfg.units {
        Units::Imperial => "fahrenheit",
        Units::Metric => "celsius",
    };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}\
         &current=temperature_2m,weather_code\
         &daily=temperature_2m_max,temperature_2m_min,precipitation_probability_max\
         &temperature_unit={unit}&timezone=auto&forecast_days=1",
        place.latitude, place.longitude
    );
    let f: ForecastResponse = http.get_json(&url, None)?;
    Ok(from_forecast(&place.name, cfg.units, f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_code_mapping() {
        assert_eq!(describe(0).0, "Clear");
        assert_eq!(describe(2).0, "Partly cloudy");
        assert_eq!(describe(3).0, "Overcast");
        assert_eq!(describe(48).0, "Fog");
        assert_eq!(describe(63).0, "Rain");
        assert_eq!(describe(81).0, "Showers");
        assert_eq!(describe(75).0, "Heavy snow");
        assert_eq!(describe(95).1, "⚡");
        assert_eq!(describe(42).0, "Unknown");
        // Every documented WMO code has a real description.
        for c in [
            0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82,
            85, 86, 95, 96, 99,
        ] {
            assert_ne!(describe(c).0, "Unknown", "code {c}");
        }
    }

    #[test]
    fn forecast_fixture_parses() {
        let f: ForecastResponse =
            serde_json::from_str(include_str!("../tests/fixtures/open_meteo_forecast.json"))
                .unwrap();
        let w = from_forecast("Evans, Georgia", Units::Imperial, f);
        assert_eq!(w.temperature, 68.4);
        assert_eq!(w.high, 76.1);
        assert_eq!(w.low, 51.3);
        assert_eq!(w.condition, "Partly cloudy");
        assert_eq!(w.precip_chance, Some(10));
        assert_eq!(w.unit, "°F");
    }

    #[test]
    fn geocode_prefers_matching_state() {
        let r: GeoResponse =
            serde_json::from_str(include_str!("../tests/fixtures/open_meteo_geocode.json"))
                .unwrap();
        let (_, q) = split_city("Evans, Georgia");
        assert_eq!(
            pick(&r.results, &q).unwrap().admin1.as_deref(),
            Some("Georgia")
        );
        let (_, q) = split_city("Evans, GA");
        assert_eq!(
            pick(&r.results, &q).unwrap().admin1.as_deref(),
            Some("Georgia")
        );
        let (_, q) = split_city("Evans, Colorado");
        assert_eq!(
            pick(&r.results, &q).unwrap().admin1.as_deref(),
            Some("Colorado")
        );
        // No qualifier → most populous (first) result.
        assert_eq!(
            pick(&r.results, &[]).unwrap().admin1.as_deref(),
            Some("Colorado")
        );
    }
}
