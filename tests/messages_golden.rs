//! Every sentence a person can read, in every language, with and without an admin name, against a
//! checked-in golden file. It pins the wording byte for byte, so moving the sentences into locale files
//! (or any later refactor) must leave `tests/messages.golden` untouched.
//!
//! Languages are parsed from YAML, as the identities file does, so this file never names how the
//! code represents a language.

use kavita_mcp::config::Language;
use kavita_mcp::kavita::KavitaError;
use kavita_mcp::messages as m;

fn render(l: Language, admin: Option<&str>) -> Vec<String> {
    // A title holding braces must come out verbatim, never read as a placeholder.
    let odd = "{title} {admin}";
    let one = vec!["Kindle".to_string()];
    let two = vec!["Kindle d'Alice".to_string(), "Bob's Kindle".to_string()];
    vec![
        m::size(l, 0),
        m::size(l, 2_202_009),
        m::size(l, 26_214_400),
        m::size(l, 43_000_000),
        m::nothing_found(l, "Le Petit Prince"),
        m::nothing_found(l, odd),
        m::unknown_handle(l),
        m::no_kindle(l, admin),
        m::which_kindle(l, &one),
        m::which_kindle(l, &two),
        m::bad_format(l, "Watchmen", "CBZ"),
        m::bad_format(l, odd, "MOBI"),
        m::too_large(l, "Les Misérables", 43_000_000, 26_214_400),
        m::email_off(l, admin),
        m::confirm(
            l,
            "The Midnight Library",
            "EPUB",
            2_202_009,
            "Alice's Kindle",
        ),
        m::confirm(l, odd, "PDF", 1_048_576, odd),
        m::not_sent(l),
        m::sent(l),
        m::kavita_fault(l, admin, &KavitaError::Unreachable),
        m::kavita_fault(l, admin, &KavitaError::KeyRejected),
        m::kavita_fault(l, admin, &KavitaError::Http5xx),
        m::kavita_fault(l, admin, &KavitaError::Refused("body".into())),
        m::send_refused(l, admin),
        m::no_elicitation(l),
    ]
}

fn all() -> String {
    let mut out = String::new();
    for code in ["en", "fr"] {
        let l: Language = serde_norway::from_str(code).unwrap();
        for admin in [None, Some("Bob")] {
            out.push_str(&format!("## {code} admin={admin:?}\n"));
            for line in render(l, admin) {
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
    out
}

#[test]
fn every_sentence_matches_the_golden_file() {
    assert_eq!(all(), include_str!("messages.golden"));
}
