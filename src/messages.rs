//! Every sentence a person reads, from `locales/<code>.json` (see `locale`), one language per message.
//! Titles and names passed in are already `clean`ed by the caller. `admin` is the person to turn to
//! when Kavita needs fixing (`KAVITA_MCP_ADMIN_NAME`); unset, the sentences name the role instead.

use crate::config::Language;
use crate::kavita::KavitaError;

/// "Tell X." with the configured name, or the role when none is set: two keys, because the two forms
/// are worded differently in each language.
fn tell(l: Language, admin: Option<&str>) -> String {
    match admin {
        Some(a) => l.render("admin_named", &[("admin", a)]),
        None => l.text("admin_unnamed").into(),
    }
}

/// A sentence that ends by telling the person whom to turn to.
fn with_tell(l: Language, key: &str, admin: Option<&str>) -> String {
    l.render(key, &[("tell", &tell(l, admin))])
}

/// Untrusted text from Kavita's file metadata: no control characters, bounded length.
#[must_use]
pub fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// MiB with one decimal, ".0" dropped; the decimal mark and the unit follow the language.
#[must_use]
pub fn size(l: Language, bytes: u64) -> String {
    let tenths = (bytes * 10 + 524_288) / 1_048_576;
    let n = if tenths.is_multiple_of(10) {
        format!("{}", tenths / 10)
    } else {
        format!("{}{}{}", tenths / 10, l.text("decimal_mark"), tenths % 10)
    };
    l.render("size", &[("n", &n)])
}

pub fn nothing_found(l: Language, query: &str) -> String {
    l.render("nothing_found", &[("query", query)])
}

pub fn unknown_handle(l: Language) -> String {
    l.text("unknown_handle").into()
}

pub fn no_kindle(l: Language, admin: Option<&str>) -> String {
    match admin {
        Some(a) => l.render("no_kindle_named", &[("admin", a)]),
        None => l.text("no_kindle_unnamed").into(),
    }
}

/// Read by the model, which then asks the person: in the person's language all the same.
pub fn which_kindle(l: Language, names: &[String]) -> String {
    let list = names
        .iter()
        .map(|n| l.render("kindle_in_list", &[("name", n)]))
        .collect::<Vec<_>>()
        .join(", ");
    l.render("which_kindle", &[("list", &list)])
}

pub fn bad_format(l: Language, title: &str, format: &str) -> String {
    l.render("bad_format", &[("title", title), ("format", format)])
}

pub fn too_large(l: Language, title: &str, bytes: u64, limit: u64) -> String {
    l.render(
        "too_large",
        &[
            ("title", title),
            ("size", &size(l, bytes)),
            ("limit", &size(l, limit)),
        ],
    )
}

pub fn email_off(l: Language, admin: Option<&str>) -> String {
    with_tell(l, "email_off", admin)
}

pub fn confirm(l: Language, title: &str, format: &str, bytes: u64, device: &str) -> String {
    l.render(
        "confirm",
        &[
            ("title", title),
            ("format", format),
            ("size", &size(l, bytes)),
            ("device", device),
        ],
    )
}

pub fn not_sent(l: Language) -> String {
    l.text("not_sent").into()
}

pub fn sent(l: Language) -> String {
    l.text("sent").into()
}

/// Never Kavita's own text: a 400's body is in the person's Kavita locale and is logged, not shown.
pub fn kavita_fault(l: Language, admin: Option<&str>, e: &KavitaError) -> String {
    match e {
        KavitaError::Unreachable => l.text("kavita_unreachable").into(),
        KavitaError::KeyRejected => with_tell(l, "kavita_key_rejected", admin),
        KavitaError::Http5xx | KavitaError::Refused(_) => with_tell(l, "kavita_problem", admin),
    }
}

/// The send itself failed on Kavita's side: only then is "refused to send it" true.
pub fn send_refused(l: Language, admin: Option<&str>) -> String {
    with_tell(l, "send_refused", admin)
}

pub fn no_elicitation(l: Language) -> String {
    l.text("no_elicitation").into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kavita::KavitaError;

    fn fr() -> Language {
        Language::parse("fr").unwrap()
    }

    fn en() -> Language {
        Language::parse("en").unwrap()
    }

    /// Every sentence, rendered in both languages: none may be empty or identical across languages.
    fn all(l: Language, admin: Option<&str>) -> Vec<String> {
        let names = vec!["A".to_string(), "B".to_string()];
        vec![
            nothing_found(l, "q"),
            unknown_handle(l),
            no_kindle(l, admin),
            which_kindle(l, &names),
            bad_format(l, "T", "CBZ"),
            too_large(l, "T", 43_000_000, 26_214_400),
            email_off(l, admin),
            confirm(l, "T", "EPUB", 2_200_000, "K"),
            not_sent(l),
            sent(l),
            kavita_fault(l, admin, &KavitaError::Unreachable),
            kavita_fault(l, admin, &KavitaError::KeyRejected),
            kavita_fault(l, admin, &KavitaError::Http5xx),
            kavita_fault(l, admin, &KavitaError::Refused("x".into())),
            send_refused(l, admin),
            no_elicitation(l),
        ]
    }

    #[test]
    fn every_sentence_exists_in_both_languages() {
        for admin in [None, Some("Bob")] {
            for (fr, en) in all(fr(), admin).iter().zip(all(en(), admin).iter()) {
                assert!(!fr.is_empty() && !en.is_empty());
                assert_ne!(fr, en, "same text in fr and en: {en}");
            }
        }
    }

    #[test]
    fn sizes_follow_the_language() {
        assert_eq!(size(fr(), 2_202_009), "2,1 Mo");
        assert_eq!(size(en(), 2_202_009), "2.1 MB");
        assert_eq!(size(en(), 26_214_400), "25 MB");
    }

    #[test]
    fn the_sentences_are_verbatim() {
        assert_eq!(
            no_kindle(fr(), Some("Bob")),
            "Tu n'as pas de Kindle enregistrée dans Kavita. Bob peut en ajouter une."
        );
        assert_eq!(
            no_kindle(en(), Some("Bob")),
            "You don't have a Kindle set up in Kavita. Bob can add one."
        );
        assert_eq!(
            send_refused(en(), Some("Bob")),
            "Kavita refused to send it. Tell Bob."
        );
        assert_eq!(
            send_refused(fr(), Some("Bob")),
            "Kavita a refusé de l'envoyer. Préviens Bob."
        );
        assert_eq!(
            unknown_handle(en()),
            "I can only send a book from your last search. Search for it again."
        );
        assert_eq!(not_sent(fr()), "Pas envoyé.");
        assert_eq!(
            sent(en()),
            "Sent. It will be on your Kindle in a few minutes."
        );
        assert_eq!(
            confirm(en(), "The Midnight Library", "EPUB", 2_202_009, "Alice's Kindle"),
            "Send \"The Midnight Library\" (EPUB, 2.1 MB) to your Kindle \"Alice's Kindle\"? ✅ to confirm, ❌ to cancel."
        );
        assert_eq!(
            confirm(fr(), "Le Bleu des abysses", "EPUB", 2_202_009, "Kindle d'Alice"),
            "Envoyer « Le Bleu des abysses » (EPUB, 2,1 Mo) sur ta Kindle « Kindle d'Alice » ? ✅ pour confirmer, ❌ pour annuler."
        );
    }

    #[test]
    fn a_refusal_never_echoes_kavitas_body() {
        let body = "send-to-size-limit secret@kindle.com";
        assert!(!kavita_fault(en(), None, &KavitaError::Refused(body.into())).contains("secret"));
    }

    #[test]
    fn without_an_admin_name_the_sentences_name_the_role() {
        assert_eq!(
            no_kindle(en(), None),
            "You don't have a Kindle set up in Kavita. Your Kavita admin can add one."
        );
        assert_eq!(
            no_kindle(fr(), None),
            "Tu n'as pas de Kindle enregistrée dans Kavita. L'admin de Kavita peut en ajouter une."
        );
        assert_eq!(
            email_off(en(), None),
            "Sending to a Kindle is switched off in Kavita right now. Tell your Kavita admin."
        );
        assert_eq!(
            kavita_fault(fr(), None, &KavitaError::Http5xx),
            "Kavita a rencontré un problème. Préviens l'admin de Kavita."
        );
    }

    #[test]
    fn clean_strips_controls_and_caps() {
        assert_eq!(clean("a\u{7}b\nc", 2), "ab");
    }
}
