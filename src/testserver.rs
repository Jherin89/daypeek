//! A throwaway HTTP server for tests: records every request and answers
//! with whatever the handler returns. Std only, one thread per server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Path and query, e.g. `/api/v1/tasks?x=1`.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Response {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Response {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
}

pub struct Server {
    /// `http://127.0.0.1:<port>`
    pub url: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
}

impl Server {
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

pub fn serve<F>(handler: F) -> Server
where
    F: Fn(&Request) -> Response + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_string();
            let path = parts.next().unwrap_or_default().to_string();
            let mut headers = Vec::new();
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            let len = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = vec![0; len];
            let _ = reader.read_exact(&mut body);
            let req = Request {
                method,
                path,
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
            };
            let resp = handler(&req);
            log.lock().unwrap().push(req);
            let mut out = format!(
                "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                resp.status,
                resp.body.len()
            );
            for (k, v) in &resp.headers {
                out.push_str(&format!("{k}: {v}\r\n"));
            }
            out.push_str("\r\n");
            out.push_str(&resp.body);
            let _ = stream.write_all(out.as_bytes());
        }
    });
    Server { url, requests }
}
