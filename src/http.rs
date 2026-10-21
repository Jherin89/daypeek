//! A tiny blocking HTTP layer over `ureq`. Every request has a hard timeout so
//! a slow API can never hang your shell prompt.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

const USER_AGENT: &str = concat!(
    "today/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Jherin89/today)"
);

#[derive(Clone)]
pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .build();
        Http { agent }
    }

    pub fn get(&self, url: &str) -> ureq::Request {
        self.agent.get(url)
    }

    pub fn post(&self, url: &str) -> ureq::Request {
        self.agent.post(url)
    }

    /// GET and decode JSON, optionally with a bearer token.
    pub fn get_json<T: DeserializeOwned>(&self, url: &str, bearer: Option<&str>) -> Result<T> {
        let mut req = self.get(url).set("Accept", "application/json");
        if let Some(t) = bearer {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        decode(req.call()?)
    }

    /// GET a text body (used for .ics feeds).
    pub fn get_text(&self, url: &str) -> Result<String> {
        let resp = self.get(url).call()?;
        resp.into_string().map_err(|e| Error::Http(e.to_string()))
    }
}

/// Decode a JSON response body.
pub fn decode<T: DeserializeOwned>(resp: ureq::Response) -> Result<T> {
    let body = resp.into_string().map_err(|e| Error::Http(e.to_string()))?;
    serde_json::from_str(&body).map_err(|e| Error::Parse(e.to_string()))
}

/// Percent-encode a query-string component.
pub fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
