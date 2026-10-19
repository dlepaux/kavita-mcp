//! The two tools as plain functions against a wiremock Kavita. No test reaches a
//! real Kavita: nothing here can send a book.

use std::time::{Duration, Instant};

use kavita_mcp::config::{Identity, Language};
use kavita_mcp::handles::Handles;
use kavita_mcp::kavita::Kavita;
use kavita_mcp::metrics::Metrics;
use kavita_mcp::tools::{search_library, send_to_kindle, Answer, SendArgs};
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LIMIT: u64 = 26_214_400;

fn who(profile: &str, language: Language, libraries: &[&str]) -> Identity {
    Identity {
        profile: profile.into(),
        token_sha256: [0; 32],
        language,
        libraries: libraries.iter().map(|s| s.to_string()).collect(),
        kavita_key: format!("key-{profile}"),
        admin: Some("Carol".into()),
    }
}

fn counter(m: &Metrics, name: &str, kind: &str) -> String {
    let prefix = format!("{name}{{kind=\"{kind}\"}} ");
    m.render()
        .lines()
        .find_map(|l| l.strip_prefix(&prefix).map(str::to_string))
        .unwrap()
}

/// Series 7 in Fiction (one EPUB chapter), series 8 in Comics (one CBZ chapter).
async fn library() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/Search/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"series": [
            {"seriesId": 7, "name": "Le Bleu des abysses", "libraryName": "Fiction"},
            {"seriesId": 8, "name": "Atlas\u{7}", "libraryName": "Comics"}
        ]})))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/Search/chapters-by-series"))
        .and(query_param("seriesId", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "titleName": "", "range": "1", "files": [{"format": 3, "bytes": 2_202_009}]}
        ])))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/Search/chapters-by-series"))
        .and(query_param("seriesId", "8"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 20, "titleName": "", "range": "1", "files": [{"format": 1, "bytes": 5_000_000}]}
        ])))
        .mount(&s)
        .await;
    s
}

#[tokio::test]
async fn search_lists_books_with_handles_and_no_ids() {
    let s = library().await;
    let (k, m, h) = (
        Kavita::new(&s.uri(), Duration::from_secs(5)),
        Metrics::new(),
        Handles::default(),
    );
    let me = who(
        "alice",
        Language::parse("fr").unwrap(),
        &["Fiction", "Comics"],
    );
    let out = search_library(&k, &m, &h, &me, "abysses", LIMIT)
        .await
        .unwrap();
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(
        lines[0].starts_with("handle=") && lines[0].contains("« Le Bleu des abysses »"),
        "{out}"
    );
    assert!(
        lines[0].contains("library: Fiction")
            && lines[0].contains("EPUB")
            && lines[0].contains("2,1 Mo")
    );
    assert!(
        lines[0].contains("kindle: yes") && lines[1].contains("kindle: no"),
        "{out}"
    );
    assert!(!out.contains("\u{7}"), "control character leaked");
    for id in ["=10", "=20", " 10 ", " 20 ", "seriesId", "chapter"] {
        assert!(!out.contains(id), "an id leaked ({id}): {out}");
    }
    let handle = &lines[0][7..13];
    assert_eq!(
        h.resolve("alice", handle, Instant::now())
            .unwrap()
            .chapter_id,
        10
    );
}

#[tokio::test]
async fn a_result_outside_the_configured_libraries_is_dropped_and_counted() {
    let s = library().await;
    let (k, m, h) = (
        Kavita::new(&s.uri(), Duration::from_secs(5)),
        Metrics::new(),
        Handles::default(),
    );
    let me = who("alice", Language::parse("fr").unwrap(), &["Fiction"]);
    let out = search_library(&k, &m, &h, &me, "abysses", LIMIT)
        .await
        .unwrap();
    assert!(!out.contains("Atlas") && !out.contains("Comics"), "{out}");
    assert_eq!(
        counter(&m, "kavita_mcp_search_skipped_total", "library_not_granted"),
        "1"
    );
    // The dropped series' chapters were never even fetched.
    let asked: Vec<_> = s
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.url.to_string())
        .collect();
    assert!(!asked.iter().any(|u| u.contains("seriesId=8")), "{asked:?}");
}

#[tokio::test]
async fn no_hit_says_so_in_the_persons_language() {
    let s = library().await;
    let (k, m, h) = (
        Kavita::new(&s.uri(), Duration::from_secs(5)),
        Metrics::new(),
        Handles::default(),
    );
    // Every hit outside the grant: the person still gets a sentence, not an empty answer.
    let me = who("bob", Language::parse("en").unwrap(), &["Classics"]);
    let out = search_library(&k, &m, &h, &me, "abysses", LIMIT)
        .await
        .unwrap();
    assert_eq!(out, "I found nothing for \"abysses\" in your libraries.");
}

#[tokio::test]
async fn a_multi_file_chapter_is_sendable_only_when_every_file_matches() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/Search/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"series": [
            {"seriesId": 9, "name": "Split", "libraryName": "Classics"}]})))
        .mount(&s)
        .await;
    Mock::given(method("GET")).and(path("/api/Search/chapters-by-series"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 30, "titleName": "Two PDFs", "files": [{"format": 4, "bytes": 1_048_576}, {"format": 4, "bytes": 1_048_576}]},
            {"id": 31, "titleName": "Mixed", "files": [{"format": 3, "bytes": 1}, {"format": 1, "bytes": 1}]}
        ]))).mount(&s).await;
    let (k, m, h) = (
        Kavita::new(&s.uri(), Duration::from_secs(5)),
        Metrics::new(),
        Handles::default(),
    );
    let out = search_library(
        &k,
        &m,
        &h,
        &who("bob", Language::parse("en").unwrap(), &["Classics"]),
        "split",
        LIMIT,
    )
    .await
    .unwrap();
    let lines: Vec<_> = out.lines().collect();
    assert!(
        lines[0].contains("PDF") && lines[0].contains("2 MB") && lines[0].contains("kindle: yes"),
        "{out}"
    );
    assert!(
        lines[1].contains("unknown") && lines[1].contains("kindle: no"),
        "{out}"
    );
}

#[tokio::test]
async fn a_kavita_fault_during_search_is_counted_and_worded() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/Search/search"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&s)
        .await;
    let (k, m, h) = (
        Kavita::new(&s.uri(), Duration::from_secs(5)),
        Metrics::new(),
        Handles::default(),
    );
    let err = search_library(
        &k,
        &m,
        &h,
        &who("bob", Language::parse("en").unwrap(), &["Classics"]),
        "x",
        LIMIT,
    )
    .await
    .unwrap_err();
    assert_eq!(err, "Kavita didn't accept my access. Tell Carol.");
    assert_eq!(
        counter(&m, "kavita_mcp_kavita_failures_total", "key_rejected"),
        "1"
    );
}

#[tokio::test]
async fn a_kavita_5xx_during_search_is_not_worded_as_a_refused_send() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/Search/search"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&s)
        .await;
    let (k, m, h) = (
        Kavita::new(&s.uri(), Duration::from_secs(5)),
        Metrics::new(),
        Handles::default(),
    );
    let err = search_library(
        &k,
        &m,
        &h,
        &who("bob", Language::parse("en").unwrap(), &["Classics"]),
        "x",
        LIMIT,
    )
    .await
    .unwrap_err();
    assert_eq!(err, "Kavita ran into a problem. Tell Carol.");
}

#[tokio::test]
async fn a_kavita_5xx_on_the_device_list_is_not_worded_as_a_refused_send() {
    let s = library().await;
    Mock::given(method("GET"))
        .and(path("/api/Device"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&s)
        .await;
    let (m, h, me) = (
        Metrics::new(),
        Handles::default(),
        who("bob", Language::parse("en").unwrap(), &["Fiction"]),
    );
    let x = handle_for(
        &Kavita::new(&s.uri(), Duration::from_secs(5)),
        &m,
        &h,
        &me,
        "abysses",
    )
    .await;
    let r = send(&s, &me, x, None, Some(Answer::Accepted), &m, &h).await;
    assert_eq!(r.out.unwrap_err(), "Kavita ran into a problem. Tell Carol.");
    assert!(r.asked.is_empty());
}

/// A Kavita for one send: the given devices, email setup on or off, and a send-to that answers `status`.
async fn sendable(devices: serde_json::Value, email: bool, status: u16) -> MockServer {
    let s = library().await;
    Mock::given(method("GET"))
        .and(path("/api/Device"))
        .respond_with(ResponseTemplate::new(200).set_body_json(devices))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/Settings/is-email-setup"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(email)))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/Device/send-to"))
        .respond_with(
            ResponseTemplate::new(status).set_body_string("send-to-permission secret@kindle.com"),
        )
        .mount(&s)
        .await;
    s
}

fn one_kindle() -> serde_json::Value {
    json!([{"id": 3, "name": "Kindle d'Alice", "emailAddress": "secret@kindle.com", "platform": 2}])
}

async fn sends(s: &MockServer) -> Vec<serde_json::Value> {
    s.received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/api/Device/send-to")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

/// Searches as `me`, then returns the handle of the book whose line contains `title`.
async fn handle_for(k: &Kavita, m: &Metrics, h: &Handles, me: &Identity, title: &str) -> String {
    let out = search_library(k, m, h, me, "q", LIMIT).await.unwrap();
    out.lines()
        .find(|l| l.contains(title))
        .map(|l| l[7..13].to_string())
        .unwrap()
}

struct Run {
    out: Result<String, String>,
    asked: Vec<String>,
}

async fn send(
    s: &MockServer,
    me: &Identity,
    handle: String,
    kindle: Option<&str>,
    answer: Option<Answer>,
    m: &Metrics,
    h: &Handles,
) -> Run {
    let k = Kavita::new(&s.uri(), Duration::from_secs(5));
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = asked.clone();
    let args = SendArgs {
        handle,
        kindle: kindle.map(str::to_string),
    };
    let out = send_to_kindle(
        &k,
        m,
        h,
        me,
        args,
        LIMIT,
        answer.is_some(),
        |q: String| async move {
            log.lock().unwrap().push(q);
            answer.unwrap()
        },
        || false,
    )
    .await;
    let asked = asked.lock().unwrap().clone();
    Run { out, asked }
}

fn alice() -> Identity {
    who(
        "alice",
        Language::parse("fr").unwrap(),
        &["Fiction", "Comics"],
    )
}

#[tokio::test]
async fn the_happy_path_asks_then_sends_exactly_one_chapter() {
    let s = sendable(one_kindle(), true, 200).await;
    let (m, h, me) = (Metrics::new(), Handles::default(), alice());
    let x = handle_for(
        &Kavita::new(&s.uri(), Duration::from_secs(5)),
        &m,
        &h,
        &me,
        "abysses",
    )
    .await;
    let r = send(&s, &me, x, None, Some(Answer::Accepted), &m, &h).await;
    assert_eq!(
        r.out.unwrap(),
        "Envoyé. Il arrive sur ta Kindle d'ici quelques minutes."
    );
    assert_eq!(r.asked, vec!["Envoyer « Le Bleu des abysses » (EPUB, 2,1 Mo) sur ta Kindle « Kindle d'Alice » ? ✅ pour confirmer, ❌ pour annuler."]);
    assert_eq!(
        sends(&s).await,
        vec![json!({"deviceId": 3, "chapterIds": [10]})]
    );
}

#[tokio::test]
async fn a_decline_or_no_answer_sends_nothing() {
    for answer in [Answer::Declined, Answer::NoAnswer] {
        let s = sendable(one_kindle(), true, 200).await;
        let (m, h, me) = (Metrics::new(), Handles::default(), alice());
        let x = handle_for(
            &Kavita::new(&s.uri(), Duration::from_secs(5)),
            &m,
            &h,
            &me,
            "abysses",
        )
        .await;
        let r = send(&s, &me, x, None, Some(answer), &m, &h).await;
        assert_eq!(r.out.unwrap(), "Pas envoyé.");
        assert!(sends(&s).await.is_empty());
        assert_eq!(
            counter(&m, "kavita_mcp_send_skipped_total", "declined"),
            "1"
        );
    }
}

#[tokio::test]
async fn an_accept_after_the_call_was_cancelled_sends_nothing() {
    let s = sendable(one_kindle(), true, 200).await;
    let (m, h, me) = (Metrics::new(), Handles::default(), alice());
    let k = Kavita::new(&s.uri(), Duration::from_secs(5));
    let x = handle_for(&k, &m, &h, &me, "abysses").await;
    let out = send_to_kindle(
        &k,
        &m,
        &h,
        &me,
        SendArgs {
            handle: x,
            kindle: None,
        },
        LIMIT,
        true,
        |_| async { Answer::Accepted },
        || true,
    )
    .await;
    assert_eq!(out.unwrap(), "Pas envoyé.");
    assert!(sends(&s).await.is_empty());
}

#[tokio::test]
async fn another_persons_handle_or_a_chapter_id_is_refused_without_a_send() {
    let s = sendable(one_kindle(), true, 200).await;
    let (m, h) = (Metrics::new(), Handles::default());
    let x = handle_for(
        &Kavita::new(&s.uri(), Duration::from_secs(5)),
        &m,
        &h,
        &alice(),
        "abysses",
    )
    .await;
    let bob = who("bob", Language::parse("en").unwrap(), &["Fiction"]);
    for guess in [x, "10".to_string()] {
        let r = send(&s, &bob, guess, None, Some(Answer::Accepted), &m, &h).await;
        assert_eq!(
            r.out.unwrap(),
            "I can only send a book from your last search. Search for it again."
        );
        assert!(r.asked.is_empty());
    }
    assert!(sends(&s).await.is_empty());
    assert_eq!(
        counter(&m, "kavita_mcp_send_skipped_total", "unknown_handle"),
        "2"
    );
}

#[tokio::test]
async fn no_kindle_is_answered_without_asking() {
    for devices in [
        json!([]),
        json!([{"id": 4, "name": "Kobo", "emailAddress": "k@x", "platform": 3}]),
    ] {
        let s = sendable(devices, true, 200).await;
        let (m, h, me) = (
            Metrics::new(),
            Handles::default(),
            who("bob", Language::parse("en").unwrap(), &["Fiction"]),
        );
        let x = handle_for(
            &Kavita::new(&s.uri(), Duration::from_secs(5)),
            &m,
            &h,
            &me,
            "abysses",
        )
        .await;
        let r = send(&s, &me, x, None, Some(Answer::Accepted), &m, &h).await;
        assert_eq!(
            r.out.unwrap(),
            "You don't have a Kindle set up in Kavita. Carol can add one."
        );
        assert!(r.asked.is_empty() && sends(&s).await.is_empty());
        assert_eq!(
            counter(&m, "kavita_mcp_send_skipped_total", "no_kindle"),
            "1"
        );
    }
}

#[tokio::test]
async fn several_kindles_need_a_name_matched_case_insensitively() {
    let two = json!([{"id": 3, "name": "Kindle d'Alice", "emailAddress": "a@kindle.com", "platform": 2},
                     {"id": 5, "name": "Paperwhite", "emailAddress": "b@kindle.com", "platform": 2}]);
    let s = sendable(two, true, 200).await;
    let (m, h, me) = (Metrics::new(), Handles::default(), alice());
    let k = Kavita::new(&s.uri(), Duration::from_secs(5));
    let x = handle_for(&k, &m, &h, &me, "abysses").await;
    for kindle in [None, Some("Kobo")] {
        let r = send(&s, &me, x.clone(), kindle, Some(Answer::Accepted), &m, &h).await;
        let out = r.out.unwrap();
        assert!(
            out.contains("Paperwhite") && out.contains("Kindle d'Alice"),
            "{out}"
        );
        assert!(!out.contains("@"), "an email leaked: {out}");
        assert!(r.asked.is_empty());
    }
    assert!(sends(&s).await.is_empty());
    let r = send(
        &s,
        &me,
        x,
        Some("paperwhite"),
        Some(Answer::Accepted),
        &m,
        &h,
    )
    .await;
    assert!(r.out.is_ok());
    assert_eq!(
        sends(&s).await,
        vec![json!({"deviceId": 5, "chapterIds": [10]})]
    );
}

#[tokio::test]
async fn a_cbz_is_refused_before_asking() {
    let s = sendable(one_kindle(), true, 200).await;
    let (m, h, me) = (Metrics::new(), Handles::default(), alice());
    let x = handle_for(
        &Kavita::new(&s.uri(), Duration::from_secs(5)),
        &m,
        &h,
        &me,
        "Atlas",
    )
    .await;
    let r = send(&s, &me, x, None, Some(Answer::Accepted), &m, &h).await;
    assert_eq!(
        r.out.unwrap(),
        "« Atlas » est un fichier CBZ ; une Kindle ne prend que l'EPUB ou le PDF."
    );
    assert!(r.asked.is_empty() && sends(&s).await.is_empty());
    assert_eq!(counter(&m, "kavita_mcp_send_skipped_total", "format"), "1");
}

#[tokio::test]
async fn a_file_at_exactly_the_limit_is_refused() {
    let s = sendable(one_kindle(), true, 200).await;
    let (m, h, me) = (Metrics::new(), Handles::default(), alice());
    let k = Kavita::new(&s.uri(), Duration::from_secs(5));
    let x = handle_for(&k, &m, &h, &me, "abysses").await; // 2_202_009 bytes
    let out = send_to_kindle(
        &k,
        &m,
        &h,
        &me,
        SendArgs {
            handle: x,
            kindle: None,
        },
        2_202_009,
        true,
        |_| async { Answer::Accepted },
        || false,
    )
    .await;
    assert!(out
        .unwrap()
        .starts_with("« Le Bleu des abysses » fait 2,1 Mo"));
    assert!(sends(&s).await.is_empty());
    assert_eq!(
        counter(&m, "kavita_mcp_send_skipped_total", "too_large"),
        "1"
    );
}

#[tokio::test]
async fn email_off_is_refused_and_counted_as_a_fault() {
    let s = sendable(one_kindle(), false, 200).await;
    let (m, h, me) = (
        Metrics::new(),
        Handles::default(),
        who("bob", Language::parse("en").unwrap(), &["Fiction"]),
    );
    let x = handle_for(
        &Kavita::new(&s.uri(), Duration::from_secs(5)),
        &m,
        &h,
        &me,
        "abysses",
    )
    .await;
    let r = send(&s, &me, x, None, Some(Answer::Accepted), &m, &h).await;
    assert_eq!(
        r.out.unwrap_err(),
        "Sending to a Kindle is switched off in Kavita right now. Tell Carol."
    );
    assert!(r.asked.is_empty() && sends(&s).await.is_empty());
    assert_eq!(
        counter(&m, "kavita_mcp_kavita_failures_total", "email_off"),
        "1"
    );
}

#[tokio::test]
async fn kavitas_send_errors_map_to_kinds_and_never_echo_the_body() {
    for (status, kind) in [
        (400, "kavita_refused"),
        (401, "key_rejected"),
        (502, "http_5xx"),
    ] {
        let s = sendable(one_kindle(), true, status).await;
        let (m, h, me) = (
            Metrics::new(),
            Handles::default(),
            who("bob", Language::parse("en").unwrap(), &["Fiction"]),
        );
        let x = handle_for(
            &Kavita::new(&s.uri(), Duration::from_secs(5)),
            &m,
            &h,
            &me,
            "abysses",
        )
        .await;
        let r = send(&s, &me, x, None, Some(Answer::Accepted), &m, &h).await;
        let err = r.out.unwrap_err();
        assert!(
            !err.contains("send-to-permission") && !err.contains("@"),
            "{err}"
        );
        if status != 401 {
            assert_eq!(
                err, "Kavita refused to send it. Tell Carol.",
                "status {status}"
            );
        }
        assert_eq!(
            counter(&m, "kavita_mcp_kavita_failures_total", kind),
            "1",
            "status {status}"
        );
    }
}

#[tokio::test]
async fn without_elicitation_nothing_is_resolved_or_sent() {
    let s = sendable(one_kindle(), true, 200).await;
    let (m, h, me) = (
        Metrics::new(),
        Handles::default(),
        who("bob", Language::parse("en").unwrap(), &["Fiction"]),
    );
    let x = handle_for(
        &Kavita::new(&s.uri(), Duration::from_secs(5)),
        &m,
        &h,
        &me,
        "abysses",
    )
    .await;
    let r = send(&s, &me, x, None, None, &m, &h).await;
    assert_eq!(
        r.out.unwrap(),
        "This chat cannot ask you to confirm, so nothing was sent."
    );
    assert_eq!(
        counter(
            &m,
            "kavita_mcp_request_failures_total",
            "elicitation_unsupported"
        ),
        "1"
    );
    let devices = s
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/api/Device")
        .count();
    assert_eq!(devices, 0, "step 1 must stop before any Kavita call");
}
