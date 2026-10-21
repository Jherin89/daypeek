//! Credential lookup: environment variable → macOS Keychain → config file.
//!
//! `daypeek` never ships, logs, or caches secrets. On macOS a secret can live in
//! the login Keychain as a generic password whose *service* is
//! `daypeek-<provider>` (or `daypeek-<provider>-<name>` for secondary values like
//! refresh tokens):
//!
//! ```sh
//! security add-generic-password -s daypeek-todoist -a "$USER" -w
//! ```

/// Resolve a secret for provider instance `instance`.
///
/// * `name`: `"token"`, `"refresh_token"`, `"client_secret"`, ...
/// * `env_var`: environment variable checked first.
/// * `inline`: value from the config file, used last.
pub fn resolve(instance: &str, name: &str, env_var: &str, inline: Option<&str>) -> Option<String> {
    if let Ok(v) = std::env::var(env_var) {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    if let Some(v) = keychain(&keychain_service(instance, name)) {
        return Some(v);
    }
    inline
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Keychain service name for a secret.
pub fn keychain_service(instance: &str, name: &str) -> String {
    if name == "token" {
        format!("daypeek-{instance}")
    } else {
        format!("daypeek-{instance}-{}", name.replace('_', "-"))
    }
}

/// Read a generic password from the macOS login Keychain.
#[cfg(target_os = "macos")]
pub fn keychain(service: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-w"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!v.is_empty()).then_some(v)
}

#[cfg(not(target_os = "macos"))]
pub fn keychain(_service: &str) -> Option<String> {
    None
}

/// Save a secret in the macOS login Keychain (replacing any existing item),
/// then read it back to confirm.
///
/// The value is passed on `security -i`'s stdin rather than as an argument, so
/// it never shows up in the process list.
#[cfg(target_os = "macos")]
pub fn store_keychain(service: &str, value: &str) -> Result<(), String> {
    use std::io::Write;
    if value.is_empty()
        || value
            .chars()
            .any(|c| c == '"' || c == '\\' || c.is_control())
    {
        return Err("this value has characters the Keychain helper can't pass safely".into());
    }
    let account = std::env::var("USER").unwrap_or_else(|_| "daypeek".into());
    let mut child = std::process::Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("security: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(
            stdin,
            "add-generic-password -U -s \"{service}\" -a \"{account}\" -l \"{service}\" -w \"{value}\""
        )
        .map_err(|e| format!("security: {e}"))?;
    }
    child.wait().map_err(|e| format!("security: {e}"))?;
    match keychain(service) {
        Some(v) if v == value => Ok(()),
        _ => Err(format!("couldn't confirm Keychain item \"{service}\"")),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn store_keychain(_service: &str, _value: &str) -> Result<(), String> {
    Err("the Keychain is only available on macOS".into())
}

/// Short description of where a secret can come from, for error hints.
pub fn hint(instance: &str, name: &str, env_var: &str) -> String {
    if cfg!(target_os = "macos") {
        format!(
            "set ${env_var} or Keychain item \"{}\"",
            keychain_service(instance, name)
        )
    } else {
        format!("set ${env_var}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keychain_service_names() {
        assert_eq!(keychain_service("todoist", "token"), "daypeek-todoist");
        assert_eq!(
            keychain_service("gtasks", "refresh_token"),
            "daypeek-gtasks-refresh-token"
        );
    }

    #[test]
    fn env_beats_inline() {
        // Unique var name so parallel tests don't collide.
        std::env::set_var("DAYPEEK_TEST_SECRET_A", "from-env");
        assert_eq!(
            resolve("x", "token", "DAYPEEK_TEST_SECRET_A", Some("inline")).as_deref(),
            Some("from-env")
        );
        std::env::remove_var("DAYPEEK_TEST_SECRET_A");
    }

    #[test]
    fn inline_is_fallback_and_blank_is_none() {
        assert_eq!(
            resolve("nope-zz", "token", "DAYPEEK_TEST_SECRET_UNSET", Some(" t ")).as_deref(),
            Some("t")
        );
        assert_eq!(
            resolve("nope-zz", "token", "DAYPEEK_TEST_SECRET_UNSET", Some("  ")),
            None
        );
    }
}
