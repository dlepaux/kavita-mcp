//! Start-up configuration. kavita-mcp refuses to start rather than serve a person it cannot act for:
//! every identity needs its Kavita key, and every key needs its identity.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

#[derive(Debug)]
pub struct ConfigError(String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

fn err(m: impl Into<String>) -> ConfigError {
    ConfigError(m.into())
}

/// Kavita's own default `SmtpConfig.SizeLimit`; an admin-only setting kavita-mcp mirrors.
const DEFAULT_SIZE_LIMIT: u64 = 26_214_400;
const KEY_PREFIX: &str = "KAVITA_MCP_KEY_";
const MAX_ADMIN_NAME: usize = 64;

pub use crate::locale::Language;

#[derive(Clone)]
pub struct Identity {
    pub profile: String,
    pub token_sha256: [u8; 32],
    pub language: Language,
    /// The libraries this person may search and send from: the second check on every search result.
    pub libraries: Vec<String>,
    pub kavita_key: String,
    /// Who the person is told to turn to when Kavita needs fixing (`KAVITA_MCP_ADMIN_NAME`), the
    /// same for everyone; `None` words it as "your Kavita admin".
    pub admin: Option<String>,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("profile", &self.profile)
            .field("language", &self.language)
            .field("libraries", &self.libraries)
            .field("kavita_key", &"<redacted>")
            .field("admin", &self.admin)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawIdentity {
    pub profile: String,
    pub token_sha256: String,
    pub language: Language,
    pub libraries: Vec<String>,
}

/// Profiles are `[a-z0-9]+`, so the upper-cased profile is an unambiguous variable name.
#[must_use]
pub fn key_var(profile: &str) -> String {
    format!("{KEY_PREFIX}{}", profile.to_ascii_uppercase())
}

fn is_id(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn decode_hash(raw: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    hex::decode_to_slice(raw, &mut out).ok()?;
    Some(out)
}

/// # Errors
/// Malformed YAML, an empty list, a bad profile or hash, no library, or a duplicate profile or token.
pub fn parse_identities(yaml: &str) -> Result<Vec<RawIdentity>, ConfigError> {
    let raw: Vec<RawIdentity> =
        serde_norway::from_str(yaml).map_err(|e| err(format!("identities: {e}")))?;
    if raw.is_empty() {
        return Err(err(
            "identities: the list is empty, nobody could use the server",
        ));
    }
    let (mut profiles, mut tokens) = (HashSet::new(), HashSet::new());
    for r in &raw {
        if !is_id(&r.profile) {
            return Err(err(format!(
                "identities: profile {:?} is not [a-z0-9]{{1,32}}",
                r.profile
            )));
        }
        let hash = decode_hash(&r.token_sha256).ok_or_else(|| {
            err(format!(
                "identities: {}'s token_sha256 is not 64 hex chars",
                r.profile
            ))
        })?;
        if r.libraries.is_empty() {
            return Err(err(format!("identities: {} has no library", r.profile)));
        }
        if !profiles.insert(r.profile.clone()) || !tokens.insert(hash) {
            return Err(err(format!(
                "identities: {} duplicates another entry",
                r.profile
            )));
        }
    }
    Ok(raw)
}

pub struct Config {
    pub addr: String,
    pub kavita_url: String,
    pub identities: Vec<Identity>,
    pub send_size_limit: u64,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("addr", &self.addr)
            .field("kavita_url", &self.kavita_url)
            .field("identities", &self.identities)
            .field("send_size_limit", &self.send_size_limit)
            .finish()
    }
}

impl Config {
    /// `vars` is the whole environment, so a key with no identity can be found too.
    ///
    /// # Errors
    /// A required variable unset or empty, a bad identities file, a missing or orphan key, a bad limit.
    pub fn from_vars(vars: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let get = |k: &str| vars.get(k).filter(|v| !v.is_empty()).cloned();
        let required = |k: &str| {
            get(k).ok_or_else(|| err(format!("{k} is unset or empty; refusing to start")))
        };
        let path = required("KAVITA_MCP_IDENTITIES")?;
        let yaml = std::fs::read_to_string(&path).map_err(|e| err(format!("{path}: {e}")))?;
        let raw = parse_identities(&yaml)?;

        let admin = match get("KAVITA_MCP_ADMIN_NAME") {
            None => None,
            Some(a) if a.chars().count() <= MAX_ADMIN_NAME && !a.chars().any(char::is_control) => {
                Some(a.trim().to_string()).filter(|a| !a.is_empty())
            }
            Some(_) => {
                return Err(err(format!(
                    "KAVITA_MCP_ADMIN_NAME must be at most {MAX_ADMIN_NAME} characters, no control characters"
                )))
            }
        };

        let wanted: HashSet<String> = raw.iter().map(|r| key_var(&r.profile)).collect();
        if let Some(orphan) = vars
            .keys()
            .filter(|k| k.starts_with(KEY_PREFIX))
            .find(|k| !wanted.contains(*k))
        {
            return Err(err(format!("{orphan} has no identity; refusing to start")));
        }
        let identities = raw
            .into_iter()
            .map(|r| {
                let var = key_var(&r.profile);
                Ok(Identity {
                    kavita_key: required(&var)?,
                    token_sha256: decode_hash(&r.token_sha256)
                        .expect("checked by parse_identities"),
                    profile: r.profile,
                    language: r.language,
                    libraries: r.libraries,
                    admin: admin.clone(),
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;

        let send_size_limit = match get("KAVITA_SEND_SIZE_LIMIT_BYTES") {
            None => DEFAULT_SIZE_LIMIT,
            Some(v) => v.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                err(format!(
                    "KAVITA_SEND_SIZE_LIMIT_BYTES={v:?} is not a positive byte count"
                ))
            })?,
        };
        Ok(Self {
            addr: get("KAVITA_MCP_ADDR").unwrap_or_else(|| "0.0.0.0:8080".to_string()),
            kavita_url: required("KAVITA_URL")?.trim_end_matches('/').to_string(),
            identities,
            send_size_limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TOKEN_A: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
    const TOKEN_B: &str = "60303ae22b998861bce3b28f33eec1be758a213c86c93c076dbe9f558c11c752";

    fn two_people() -> String {
        format!(
            "- profile: bob\n  token_sha256: {TOKEN_A}\n  language: en\n  libraries: [Classics, Fiction]\n\
             - profile: alice\n  token_sha256: {TOKEN_B}\n  language: fr\n  libraries: [Fiction]\n"
        )
    }

    fn vars(path: &str) -> HashMap<String, String> {
        [
            ("KAVITA_URL", "https://kavita.example.org/"),
            ("KAVITA_MCP_IDENTITIES", path),
            ("KAVITA_MCP_KEY_BOB", "key-bob"),
            ("KAVITA_MCP_KEY_ALICE", "key-alice"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn file(name: &str, body: &str) -> String {
        let p = std::env::temp_dir().join(format!("kavita-mcp-{name}-{}.yml", std::process::id()));
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn loads_people_with_their_keys_and_the_defaults() {
        let c = Config::from_vars(&vars(&file("ok", &two_people()))).unwrap();
        assert_eq!(c.addr, "0.0.0.0:8080");
        assert_eq!(c.kavita_url, "https://kavita.example.org");
        assert_eq!(c.send_size_limit, 26_214_400);
        assert_eq!(c.identities[1].profile, "alice");
        assert_eq!(c.identities[1].language, Language::parse("fr").unwrap());
        assert_eq!(c.identities[1].libraries, vec!["Fiction".to_string()]);
        assert_eq!(c.identities[1].kavita_key, "key-alice");
    }

    #[test]
    fn an_identity_without_its_key_refuses_to_start() {
        let mut v = vars(&file("nokey", &two_people()));
        v.remove("KAVITA_MCP_KEY_ALICE");
        let e = Config::from_vars(&v).unwrap_err().to_string();
        assert!(e.contains("KAVITA_MCP_KEY_ALICE"), "{e}");
    }

    #[test]
    fn a_key_without_an_identity_refuses_to_start() {
        let mut v = vars(&file("orphan", &two_people()));
        v.insert("KAVITA_MCP_KEY_CAROL".into(), "key-n".into());
        let e = Config::from_vars(&v).unwrap_err().to_string();
        assert!(e.contains("KAVITA_MCP_KEY_CAROL"), "{e}");
    }

    #[test]
    fn an_empty_key_refuses_to_start() {
        let mut v = vars(&file("empty", &two_people()));
        v.insert("KAVITA_MCP_KEY_BOB".into(), String::new());
        assert!(Config::from_vars(&v).is_err());
    }

    #[test]
    fn the_size_limit_is_overridable_and_must_be_a_positive_number() {
        let mut v = vars(&file("limit", &two_people()));
        v.insert("KAVITA_SEND_SIZE_LIMIT_BYTES".into(), "52428800".into());
        assert_eq!(Config::from_vars(&v).unwrap().send_size_limit, 52_428_800);
        for bad in ["0", "-1", "25MB"] {
            v.insert("KAVITA_SEND_SIZE_LIMIT_BYTES".into(), bad.into());
            assert!(Config::from_vars(&v).is_err(), "{bad} accepted");
        }
    }

    #[test]
    fn refuses_bad_identity_files() {
        let cases = [
            ("empty list", "[]".to_string()),
            ("bad hash", two_people().replace(TOKEN_B, "abc")),
            ("dup token", two_people().replace(TOKEN_B, TOKEN_A)),
            (
                "dup profile",
                two_people().replace("profile: alice", "profile: bob"),
            ),
            (
                "profile not [a-z0-9]",
                two_people().replace("profile: alice", "profile: al-ice"),
            ),
            (
                "unknown language",
                two_people().replace("language: fr", "language: tr"),
            ),
            (
                "unknown field",
                two_people().replace("language: fr", "language: fr\n  admin: true"),
            ),
            (
                "no library",
                two_people().replace("libraries: [Fiction]", "libraries: []"),
            ),
        ];
        for (label, yaml) in cases {
            assert!(parse_identities(&yaml).is_err(), "{label} accepted");
        }
    }

    #[test]
    fn the_admin_name_is_optional_and_bounded() {
        let mut v = vars(&file("admin", &two_people()));
        assert_eq!(Config::from_vars(&v).unwrap().identities[0].admin, None);
        v.insert("KAVITA_MCP_ADMIN_NAME".into(), "Bob".into());
        let c = Config::from_vars(&v).unwrap();
        assert!(c
            .identities
            .iter()
            .all(|i| i.admin.as_deref() == Some("Bob")));
        for bad in ["Bo\nb".to_string(), "x".repeat(65)] {
            v.insert("KAVITA_MCP_ADMIN_NAME".into(), bad.clone());
            assert!(Config::from_vars(&v).is_err(), "{bad:?} accepted");
        }
    }

    /// The readme points strangers at these files: they must start a server as they are.
    #[test]
    fn the_examples_load() {
        let env = include_str!("../examples/kavita-mcp.env");
        let mut v: HashMap<String, String> = env
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let path = file("examples", include_str!("../examples/identities.yml"));
        v.insert("KAVITA_MCP_IDENTITIES".into(), path);
        let c = Config::from_vars(&v).unwrap();
        assert_eq!(c.identities.len(), 2);
        assert_eq!(c.identities[0].admin.as_deref(), Some("Bob"));
    }

    #[test]
    fn debug_never_prints_a_key() {
        let c = Config::from_vars(&vars(&file("debug", &two_people()))).unwrap();
        assert!(!format!("{c:?}").contains("key-bob"));
    }

    #[test]
    fn every_required_variable_is_required() {
        for missing in ["KAVITA_URL", "KAVITA_MCP_IDENTITIES"] {
            let mut v = vars(&file("req", &two_people()));
            v.remove(missing);
            assert!(Config::from_vars(&v).is_err(), "{missing} unset accepted");
        }
    }
}
