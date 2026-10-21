//! Per-source "last known good" cache under `~/.cache/daypeek/`.
//!
//! Every section goes through [`Cache::fetch`]: on success the result is
//! written to disk, on failure the previous result (if any) is returned and
//! marked stale. A dashboard that runs on shell startup must never crash
//! because the Wi-Fi is down.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Freshness, Sourced};

#[derive(Serialize, Deserialize)]
struct Entry<T> {
    fetched_at: DateTime<Utc>,
    data: T,
}

/// Where cache files live and whether network fetches are allowed.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: Option<PathBuf>,
    /// `--offline`: skip the fetch and serve whatever is cached.
    pub offline: bool,
}

impl Cache {
    pub fn new(dir: Option<PathBuf>, offline: bool) -> Self {
        Cache { dir, offline }
    }

    /// A cache that never reads or writes (used by `--demo` and tests).
    pub fn disabled() -> Self {
        Cache {
            dir: None,
            offline: false,
        }
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.as_ref().map(|d| d.join(format!("{safe}.json")))
    }

    /// Read a cached value regardless of age.
    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Option<(DateTime<Utc>, T)> {
        let raw = fs::read_to_string(self.path(key)?).ok()?;
        let e: Entry<T> = serde_json::from_str(&raw).ok()?;
        Some((e.fetched_at, e.data))
    }

    /// Write a value; failures are ignored (the cache is best-effort).
    pub fn store<T: Serialize>(&self, key: &str, data: &T) {
        let Some(path) = self.path(key) else { return };
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let entry = Entry {
            fetched_at: Utc::now(),
            data,
        };
        if let Ok(json) = serde_json::to_string(&entry) {
            let tmp = path.with_extension("json.tmp");
            if fs::write(&tmp, json).is_ok() {
                restrict_permissions(&tmp);
                let _ = fs::rename(&tmp, &path);
            }
        }
    }

    /// Run `fetch`, caching success and falling back to the cache on error.
    pub fn fetch<T, F>(&self, key: &str, source: &str, fetch: F) -> Sourced<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Result<T>,
    {
        self.fetch_with(key, source, true, fetch)
    }

    /// Like [`Cache::fetch`]; `network = false` marks a local source that
    /// still runs in `--offline` mode.
    pub fn fetch_with<T, F>(&self, key: &str, source: &str, network: bool, fetch: F) -> Sourced<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Result<T>,
    {
        let result = if self.offline && network {
            Err(Error::Http("offline mode".into()))
        } else {
            fetch()
        };
        match result {
            Ok(data) => {
                self.store(key, &data);
                Sourced {
                    source: source.to_string(),
                    freshness: Freshness::Live,
                    data: Some(data),
                }
            }
            Err(err) => match self.load::<T>(key) {
                Some((fetched_at, data)) => Sourced {
                    source: source.to_string(),
                    freshness: Freshness::Stale {
                        fetched_at,
                        error: err.to_string(),
                    },
                    data: Some(data),
                },
                None => Sourced {
                    source: source.to_string(),
                    freshness: Freshness::Failed {
                        error: err.to_string(),
                    },
                    data: None,
                },
            },
        }
    }
}

/// Cached task lists may contain personal data; keep them owner-only.
#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_is_live_and_cached() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(Some(dir.path().to_path_buf()), false);
        let s = cache.fetch("k", "test", || Ok(vec![1, 2, 3]));
        assert_eq!(s.freshness, Freshness::Live);
        assert_eq!(s.data, Some(vec![1, 2, 3]));
        let (_, cached): (_, Vec<i32>) = cache.load("k").unwrap();
        assert_eq!(cached, vec![1, 2, 3]);
    }

    #[test]
    fn failure_falls_back_to_stale_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(Some(dir.path().to_path_buf()), false);
        cache.fetch("k", "test", || Ok("old".to_string()));
        let s: Sourced<String> = cache.fetch("k", "test", || {
            Err(Error::Http("connection refused".into()))
        });
        assert_eq!(s.data.as_deref(), Some("old"));
        match s.freshness {
            Freshness::Stale { error, .. } => assert!(error.contains("connection refused")),
            other => panic!("expected stale, got {other:?}"),
        }
    }

    #[test]
    fn failure_without_cache_is_failed_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(Some(dir.path().to_path_buf()), false);
        let s: Sourced<String> = cache.fetch("missing", "test", || Err(Error::Http("x".into())));
        assert!(s.data.is_none());
        assert!(matches!(s.freshness, Freshness::Failed { .. }));
    }

    #[test]
    fn offline_mode_serves_cache_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let online = Cache::new(Some(dir.path().to_path_buf()), false);
        online.fetch("k", "test", || Ok(7u32));
        let offline = Cache::new(Some(dir.path().to_path_buf()), true);
        let s = offline.fetch("k", "test", || -> Result<u32> { panic!("must not fetch") });
        assert_eq!(s.data, Some(7));
        assert!(!s.freshness.is_live());
        // Local sources still run offline.
        let s = offline.fetch_with("local", "test", false, || Ok(1u8));
        assert!(s.freshness.is_live());
    }

    #[test]
    fn corrupt_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("k.json"), "not json").unwrap();
        let cache = Cache::new(Some(dir.path().to_path_buf()), false);
        let s: Sourced<u32> = cache.fetch("k", "test", || Err(Error::Http("down".into())));
        assert!(matches!(s.freshness, Freshness::Failed { .. }));
    }
}
