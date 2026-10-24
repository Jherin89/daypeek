//! Trello: open cards assigned to you (optionally only on some boards), or
//! every open card on the boards you list.
//!
//! Needs an API key and a token: create a Power-Up at
//! <https://trello.com/power-ups/admin>, open its API key page, copy the key,
//! then follow the "Token" link to generate a token for your account.
//! The key goes in `$TRELLO_API_KEY` (Keychain `daypeek-trello-api-key`), the
//! token in `$TRELLO_TOKEN` (Keychain `daypeek-trello`). Both are sent in an
//! `Authorization` header, never in a URL. Read-only.
//! API docs: <https://developer.atlassian.com/cloud/trello/rest/>

use std::collections::HashMap;

use chrono::DateTime;
use serde::Deserialize;

use super::{settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::model::{Priority, Task};
use crate::secrets;

pub const KEY_ENV: &str = "TRELLO_API_KEY";
pub const TOKEN_ENV: &str = "TRELLO_TOKEN";

const CARD_FIELDS: &str = "fields=name,due,dueComplete,idBoard,closed";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Inline key/token (discouraged; prefer env vars or the Keychain).
    pub key: Option<String>,
    pub key_env: String,
    pub token: Option<String>,
    pub token_env: String,
    /// Board names (or ids) to read; empty = every board.
    pub boards: Vec<String>,
    /// Only cards you're a member of. With `boards` set and this off, every
    /// open card on those boards is shown.
    pub mine_only: bool,
    /// Also show cards with no due date.
    pub include_undated: bool,
    pub base_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            key: None,
            key_env: KEY_ENV.into(),
            token: None,
            token_env: TOKEN_ENV.into(),
            boards: Vec::new(),
            mine_only: true,
            include_undated: false,
            base_url: "https://api.trello.com".into(),
        }
    }
}

pub struct Trello {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    let s: Settings = settings(id, table)?;
    if !s.mine_only && s.boards.is_empty() {
        return Err(Error::Config(format!(
            "[providers.{id}]: mine_only = false needs a list of boards"
        )));
    }
    Ok(Box::new(Trello { id: id.into(), s }))
}

#[derive(Debug, Deserialize)]
pub struct Board {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub due_complete: bool,
    #[serde(default)]
    pub id_board: String,
    #[serde(default)]
    pub closed: bool,
}

/// The header Trello accepts instead of `?key=…&token=…`.
pub fn auth_header(key: &str, token: &str) -> String {
    format!("OAuth oauth_consumer_key=\"{key}\", oauth_token=\"{token}\"")
}

/// Cards → tasks, with the board name as context.
pub fn to_tasks(
    cards: Vec<Card>,
    boards: &HashMap<String, String>,
    include_undated: bool,
    source: &str,
    tz: chrono_tz::Tz,
) -> Vec<Task> {
    cards
        .into_iter()
        .filter(|c| !c.closed && !c.due_complete)
        .filter_map(|c| {
            let due = c
                .due
                .as_deref()
                .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
                .map(|d| d.with_timezone(&tz).date_naive());
            if due.is_none() && !include_undated {
                return None;
            }
            let board = boards.get(&c.id_board).cloned().unwrap_or_default();
            let mut task = Task::new(c.name, due, Priority::None)
                .with_context(board)
                .with_ref(source, &c.id, None);
            task.sources.push(source.to_string());
            Some(task)
        })
        .collect()
}

/// Which of `boards` the settings select, by name (case-insensitive) or id.
pub fn pick_boards<'a>(boards: &'a [Board], wanted: &[String]) -> Result<Vec<&'a Board>> {
    let mut out = Vec::new();
    for w in wanted {
        match boards
            .iter()
            .find(|b| b.id == *w || b.name.eq_ignore_ascii_case(w.trim()))
        {
            Some(b) => out.push(b),
            None => return Err(Error::Config(format!("no Trello board named {w:?}"))),
        }
    }
    Ok(out)
}

impl Trello {
    fn key(&self) -> Option<String> {
        secrets::resolve(&self.id, "api_key", &self.s.key_env, self.s.key.as_deref())
    }

    fn token(&self) -> Option<String> {
        secrets::resolve(
            &self.id,
            "token",
            &self.s.token_env,
            self.s.token.as_deref(),
        )
    }
}

impl Provider for Trello {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "trello"
    }
    fn name(&self) -> &'static str {
        "Trello"
    }
    fn kind(&self) -> Kind {
        Kind::Cloud
    }

    fn check(&self) -> Result<()> {
        let mut missing = Vec::new();
        if self.key().is_none() {
            missing.push(secrets::hint(&self.id, "api_key", &self.s.key_env));
        }
        if self.token().is_none() {
            missing.push(secrets::hint(&self.id, "token", &self.s.token_env));
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::AuthMissing(missing.join("; ")))
        }
    }

    fn fetch(&self, ctx: &Ctx) -> Result<Output> {
        self.check()?;
        let auth = auth_header(
            &self.key().unwrap_or_default(),
            &self.token().unwrap_or_default(),
        );
        let base = format!("{}/1", self.s.base_url.trim_end_matches('/'));
        let boards: Vec<Board> = ctx.http.get_json_auth(
            &format!("{base}/members/me/boards?filter=open&fields=name"),
            &auth,
        )?;
        let names: HashMap<String, String> = boards
            .iter()
            .map(|b| (b.id.clone(), b.name.clone()))
            .collect();
        let picked = pick_boards(&boards, &self.s.boards)
            .map_err(|e| Error::Config(format!("[providers.{}]: {e}", self.id)))?;

        let cards: Vec<Card> = if self.s.mine_only {
            let mine: Vec<Card> = ctx.http.get_json_auth(
                &format!("{base}/members/me/cards?filter=open&{CARD_FIELDS}"),
                &auth,
            )?;
            if picked.is_empty() {
                mine
            } else {
                mine.into_iter()
                    .filter(|c| picked.iter().any(|b| b.id == c.id_board))
                    .collect()
            }
        } else {
            let mut all = Vec::new();
            for b in picked {
                let cards: Vec<Card> = ctx.http.get_json_auth(
                    &format!("{base}/boards/{}/cards/open?{CARD_FIELDS}", b.id),
                    &auth,
                )?;
                all.extend(cards);
            }
            all
        };
        Ok(Output::tasks(to_tasks(
            cards,
            &names,
            self.s.include_undated,
            &self.id,
            ctx.tz,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Http;
    use crate::testserver::{serve, Response as R};
    use chrono::NaiveDate;

    const CARDS: &str = include_str!("../../tests/fixtures/trello_cards.json");
    const BOARDS: &str = r#"[{"id":"b1","name":"Work"},{"id":"b2","name":"Home"}]"#;

    #[test]
    fn parses_fixture() {
        let cards: Vec<Card> = serde_json::from_str(CARDS).unwrap();
        let boards: Vec<Board> = serde_json::from_str(BOARDS).unwrap();
        let names = boards.into_iter().map(|b| (b.id, b.name)).collect();
        let tasks = to_tasks(cards, &names, false, "trello", chrono_tz::America::New_York);
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Update pricing page", "Schedule furnace service"]
        );
        // 02:00Z on the 22nd is the evening of the 21st in New York.
        assert_eq!(tasks[0].due, NaiveDate::from_ymd_opt(2026, 10, 21));
        assert_eq!(tasks[0].context.as_deref(), Some("Work"));
        assert_eq!(tasks[1].context.as_deref(), Some("Home"));
        assert_eq!(tasks[0].refs[0].id, "c1");
    }

    #[test]
    fn needs_boards_when_not_mine_only() {
        let mut t = toml::Table::new();
        t.insert("mine_only".into(), toml::Value::Boolean(false));
        assert!(build("trello", t).is_err());
    }

    fn ctx(http: &Http) -> Ctx<'_> {
        let today = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        Ctx {
            http,
            today,
            until: today,
            tz: chrono_tz::UTC,
        }
    }

    #[test]
    fn fetch_uses_the_auth_header_never_the_url() {
        let server = serve(|req| {
            if req.path.starts_with("/1/members/me/boards") {
                R::new(200, BOARDS)
            } else if req.path.starts_with("/1/members/me/cards") {
                R::new(200, CARDS)
            } else if req.path.starts_with("/1/boards/b2/cards/open") {
                R::new(
                    200,
                    r#"[{"id":"c9","name":"Someone else's card","due":"2026-10-21T12:00:00.000Z","dueComplete":false,"idBoard":"b2","closed":false}]"#,
                )
            } else {
                R::new(404, "")
            }
        });
        let http = Http::new(std::time::Duration::from_secs(5));
        let mk = |boards: Vec<&str>, mine_only: bool| Trello {
            id: "trello".into(),
            s: Settings {
                key: Some("k3y".into()),
                key_env: "DAYPEEK_TEST_TRELLO_KEY_UNSET".into(),
                token: Some("t0ken".into()),
                token_env: "DAYPEEK_TEST_TRELLO_TOKEN_UNSET".into(),
                boards: boards.into_iter().map(String::from).collect(),
                mine_only,
                base_url: server.url.clone(),
                ..Settings::default()
            },
        };
        assert_eq!(mk(vec![], true).fetch(&ctx(&http)).unwrap().tasks.len(), 2);
        let home = mk(vec!["home"], true).fetch(&ctx(&http)).unwrap();
        assert_eq!(home.tasks.len(), 1);
        let all = mk(vec!["Home"], false).fetch(&ctx(&http)).unwrap();
        assert_eq!(all.tasks[0].title, "Someone else's card");
        let err = mk(vec!["Nope"], true).fetch(&ctx(&http)).unwrap_err();
        assert!(err.to_string().contains("no Trello board named"));
        for req in server.requests() {
            assert!(!req.path.contains("k3y") && !req.path.contains("t0ken"));
            assert_eq!(
                req.header("authorization"),
                Some(r#"OAuth oauth_consumer_key="k3y", oauth_token="t0ken""#)
            );
        }
    }

    #[test]
    fn missing_key_or_token_is_auth_missing() {
        let p = Trello {
            id: "trello-x".into(),
            s: Settings {
                key: Some("k".into()),
                key_env: "DAYPEEK_TEST_TRELLO_KEY_UNSET".into(),
                token_env: "DAYPEEK_TEST_TRELLO_TOKEN_UNSET".into(),
                ..Settings::default()
            },
        };
        match p.check() {
            Err(Error::AuthMissing(m)) => assert!(m.contains("DAYPEEK_TEST_TRELLO_TOKEN_UNSET")),
            other => panic!("{other:?}"),
        }
    }
}
