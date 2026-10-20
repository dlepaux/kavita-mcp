//! The six counters. Each (counter, kind) series exists from start-up at 0, so the first
//! failure is an `increase()` the alert rules can see.

use prometheus::{Encoder, IntCounterVec, Opts, Registry, TextEncoder};

pub const KAVITA_FAILURES: &str = "kavita_mcp_kavita_failures_total";
pub const REQUEST_FAILURES: &str = "kavita_mcp_request_failures_total";
pub const AUTH_FAILURES: &str = "kavita_mcp_auth_failures_total";
pub const SEARCH_SKIPPED: &str = "kavita_mcp_search_skipped_total";
pub const SEND_SKIPPED: &str = "kavita_mcp_send_skipped_total";
pub const LANGUAGE_SKIPPED: &str = "kavita_mcp_language_skipped_total";

/// send_skipped is user outcomes, not worth an alert; search_skipped is worth one, because a drop means
/// Kavita and the configured grant disagree. language_skipped is a person answered in their profile's
/// language instead of the one the model named: rare, and each one is a language this build could add.
const COUNTERS: [(&str, &str, &[&str]); 6] = [
    (
        KAVITA_FAILURES,
        "Kavita calls that failed",
        &[
            "unreachable",
            "http_5xx",
            "key_rejected",
            "kavita_refused",
            "email_off",
        ],
    ),
    (
        REQUEST_FAILURES,
        "send_to_kindle calls that could not ask",
        &["elicitation_unsupported"],
    ),
    (AUTH_FAILURES, "Refused bearers on /mcp", &["mcp_token"]),
    (
        SEARCH_SKIPPED,
        "Search results dropped by the configured library check",
        &["library_not_granted"],
    ),
    (
        SEND_SKIPPED,
        "send_to_kindle calls that sent nothing, by user outcome",
        &[
            "unknown_handle",
            "no_kindle",
            "format",
            "too_large",
            "declined",
        ],
    ),
    (
        LANGUAGE_SKIPPED,
        "Tool calls whose named language this build has no locale for, so the profile's was used",
        &["unsupported_language"],
    ),
];

pub struct Metrics {
    registry: Registry,
    counters: Vec<(&'static str, IntCounterVec)>,
}

impl Metrics {
    #[must_use]
    pub fn new() -> Self {
        let registry = Registry::new();
        let counters = COUNTERS
            .iter()
            .map(|(name, help, kinds)| {
                let vec = IntCounterVec::new(Opts::new(*name, *help), &["kind"])
                    .expect("static counter definition is valid");
                registry
                    .register(Box::new(vec.clone()))
                    .expect("each counter is registered once");
                for kind in *kinds {
                    vec.with_label_values(&[*kind]);
                }
                (*name, vec)
            })
            .collect();
        Self { registry, counters }
    }

    /// `kind` must be one of the counter's declared kinds: an undeclared one would make a series that
    /// no alert and no pre-registration knows about. Debug builds and tests panic on it.
    pub fn inc(&self, counter: &str, kind: &str) {
        debug_assert!(
            COUNTERS
                .iter()
                .any(|(n, _, kinds)| *n == counter && kinds.contains(&kind)),
            "undeclared counter kind {counter}{{{kind}}}"
        );
        if let Some((_, vec)) = self.counters.iter().find(|(n, _)| *n == counter) {
            vec.with_label_values(&[kind]).inc();
        }
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut out = Vec::new();
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut out)
            .expect("text encoding of counters cannot fail");
        String::from_utf8(out).expect("the text format is UTF-8")
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_series_exists_at_zero_from_start() {
        let text = Metrics::new().render();
        for (name, _, kinds) in COUNTERS {
            for kind in kinds {
                let line = format!("{name}{{kind=\"{kind}\"}} 0");
                assert!(text.contains(&line), "missing {line}");
            }
        }
        assert_eq!(COUNTERS.iter().map(|c| c.2.len()).sum::<usize>(), 14);
    }

    #[test]
    fn inc_counts_one() {
        let m = Metrics::new();
        m.inc(SEND_SKIPPED, "declined");
        assert!(m
            .render()
            .contains("kavita_mcp_send_skipped_total{kind=\"declined\"} 1"));
    }

    #[test]
    #[should_panic(expected = "undeclared")]
    fn an_undeclared_kind_panics_in_tests() {
        Metrics::new().inc(SEND_SKIPPED, "made_up");
    }
}
