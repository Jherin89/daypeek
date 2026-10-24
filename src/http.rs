//! A tiny blocking HTTP layer over `ureq`. Every request has a hard timeout so
//! a slow API can never hang your shell prompt.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

const USER_AGENT: &str = concat!(
    "daypeek/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Jherin89/daypeek)"
);

#[derive(Clone)]
pub struct Http {
    agent: ureq::Agent,
    /// Same timeout, but redirects are returned instead of followed (CalDAV
    /// follows them itself so PROPFIND/REPORT keep their method and body).
    manual: ureq::Agent,
}

impl Http {
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .build();
        let manual = ureq::AgentBuilder::new()
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .redirects(0)
            .build();
        Http { agent, manual }
    }

    pub fn get(&self, url: &str) -> ureq::Request {
        self.agent.get(url)
    }

    pub fn post(&self, url: &str) -> ureq::Request {
        self.agent.post(url)
    }

    /// Any method (`PROPFIND`, `REPORT`, ...), without following redirects.
    pub fn request(&self, method: &str, url: &str) -> ureq::Request {
        self.manual.request(method, url)
    }

    /// GET and decode JSON, optionally with a bearer token.
    pub fn get_json<T: DeserializeOwned>(&self, url: &str, bearer: Option<&str>) -> Result<T> {
        match bearer {
            Some(t) => self.get_json_auth(url, &format!("Bearer {t}")),
            None => decode(self.get(url).set("Accept", "application/json").call()?),
        }
    }

    /// GET and decode JSON with a raw `Authorization` header value (Linear
    /// keys have no `Bearer` prefix; Trello uses an `OAuth ...` header).
    pub fn get_json_auth<T: DeserializeOwned>(&self, url: &str, auth: &str) -> Result<T> {
        let req = self
            .get(url)
            .set("Accept", "application/json")
            .set("Authorization", auth);
        decode(req.call()?)
    }

    /// POST a JSON body and decode the JSON reply.
    pub fn post_json_auth<T: DeserializeOwned>(
        &self,
        url: &str,
        auth: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let req = self
            .post(url)
            .set("Accept", "application/json")
            .set("Authorization", auth);
        decode(
            req.set("Content-Type", "application/json")
                .send_string(&body.to_string())?,
        )
    }

    /// POST with no body, ignoring the reply (used to complete a task).
    pub fn post_empty(&self, url: &str, auth: &str) -> Result<()> {
        self.post(url)
            .set("Authorization", auth)
            .set("Content-Length", "0")
            .call()?;
        Ok(())
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
