//! `today` — a fast one-screen daily dashboard for the terminal.
//!
//! The binary is a thin CLI over this library: [`snapshot::gather`] fetches
//! every configured source concurrently (with per-source caching), and
//! [`render`] turns the resulting [`snapshot::Snapshot`] into text.

pub mod cache;
pub mod calendar;
pub mod config;
pub mod demo;
pub mod error;
pub mod http;
pub mod model;
pub mod oauth;
pub mod providers;
pub mod render;
pub mod secrets;
pub mod snapshot;
pub mod tasks;
pub mod weather;
