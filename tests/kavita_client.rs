//! The Kavita client against wiremock: the key travels only as `x-api-key`, and every status maps to
//! one of our error kinds.

use std::time::Duration;

use kavita_mcp::kavita::{Format, Kavita, KavitaError};
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn kavita() -> (MockServer, Kavita) {
    let server = MockServer::start().await;
    let client = Kavita::new(&server.uri(), Duration::from_secs(5));
    (server, client)
}

#[tokio::test]
async fn every_request_carries_the_key_as_a_header_and_never_in_the_query() {
    let (server, k) = kavita().await;
    Mock::given(method("GET"))
        .and(path("/api/Search/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"series": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/Search/chapters-by-series"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/Device"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/Settings/is-email-setup"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(true)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/Device/send-to"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    k.search("key-a", "abysses").await.unwrap();
    k.chapters("key-a", 7).await.unwrap();
    k.devices("key-a").await.unwrap();
    k.email_setup("key-a").await.unwrap();
    k.send_to("key-a", 3, 10).await.unwrap();

    let got = server.received_requests().await.unwrap();
    assert_eq!(got.len(), 5);
    for r in got {
        assert_eq!(r.headers.get("x-api-key").unwrap(), "key-a", "{}", r.url);
        assert!(
            r.url
                .query_pairs()
                .all(|(k, _)| !k.eq_ignore_ascii_case("apikey")),
            "{}",
            r.url
        );
        assert!(!r.url.path().contains("key-a"), "{}", r.url);
    }
}

#[tokio::test]
async fn search_parses_series_with_their_library() {
    let (server, k) = kavita().await;
    Mock::given(method("GET")).and(path("/api/Search/search"))
        .and(query_param("queryString", "abysses")).and(query_param("includeChapterAndFiles", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"series": [
            {"seriesId": 7, "name": "Le Bleu des abysses", "libraryId": 2, "libraryName": "Fiction", "format": 3}
        ], "chapters": [], "files": []})))
        .mount(&server).await;
    let hits = k.search("key-a", "abysses").await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (hits[0].series_id, hits[0].library_name.as_str()),
        (7, "Fiction")
    );
}

#[tokio::test]
async fn chapters_parse_files_and_both_enum_forms() {
    let (server, k) = kavita().await;
    Mock::given(method("GET")).and(path("/api/Search/chapters-by-series")).and(query_param("seriesId", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "titleName": "", "range": "1", "files": [{"format": 3, "bytes": 2_200_000}]},
            {"id": 11, "titleName": "Tome 2", "range": "2", "files": [{"format": "Pdf", "bytes": 5}]},
            {"id": 12, "titleName": "x", "range": "3", "files": [{"format": 9, "bytes": 5}]}
        ])))
        .mount(&server).await;
    let ch = k.chapters("key-a", 7).await.unwrap();
    assert_eq!(ch[0].files[0].format, Format::Epub);
    assert_eq!(ch[1].files[0].format, Format::Pdf);
    assert_eq!(ch[2].files[0].format, Format::Unknown);
}

#[tokio::test]
async fn devices_never_keep_the_email_and_accept_a_string_platform() {
    let (server, k) = kavita().await;
    Mock::given(method("GET"))
        .and(path("/api/Device"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 3, "name": "Kindle d'Alice", "emailAddress": "secret@kindle.com", "platform": 2},
            {"id": 4, "name": "Kobo", "emailAddress": "k@x", "platform": "Kindle"}
        ])))
        .mount(&server)
        .await;
    let d = k.devices("key-a").await.unwrap();
    assert_eq!((d[0].id, d[0].platform), (3, 2));
    assert_eq!(d[1].platform, 2, "string enum form");
    assert!(!format!("{d:?}").contains("secret@kindle.com"));
}

#[tokio::test]
async fn send_to_posts_exactly_one_chapter() {
    let (server, k) = kavita().await;
    Mock::given(method("POST"))
        .and(path("/api/Device/send-to"))
        .and(body_json(json!({"deviceId": 3, "chapterIds": [10]})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    k.send_to("key-a", 3, 10).await.unwrap();
}

#[tokio::test]
async fn statuses_map_to_error_kinds() {
    for (status, kind) in [
        (400, "kavita_refused"),
        (401, "key_rejected"),
        (403, "kavita_refused"),
        (404, "kavita_refused"),
        (500, "http_5xx"),
        (503, "http_5xx"),
    ] {
        let (server, k) = kavita().await;
        Mock::given(method("POST"))
            .and(path("/api/Device/send-to"))
            .respond_with(ResponseTemplate::new(status).set_body_string("send-to-size-limit"))
            .mount(&server)
            .await;
        assert_eq!(
            k.send_to("key-a", 3, 10).await.unwrap_err().kind(),
            kind,
            "status {status}"
        );
    }
}

#[tokio::test]
async fn an_undecodable_body_is_refused_not_a_panic() {
    let (server, k) = kavita().await;
    Mock::given(method("GET"))
        .and(path("/api/Device"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
        .mount(&server)
        .await;
    assert_eq!(
        k.devices("key-a").await.unwrap_err().kind(),
        "kavita_refused"
    );
}

#[tokio::test]
async fn no_answer_in_time_is_unreachable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/Device"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(500))
                .set_body_json(json!([])),
        )
        .mount(&server)
        .await;
    let k = Kavita::new(&server.uri(), Duration::from_millis(100));
    assert!(matches!(
        k.devices("key-a").await,
        Err(KavitaError::Unreachable)
    ));
    let closed = Kavita::new("http://127.0.0.1:9", Duration::from_secs(1));
    assert!(matches!(
        closed.devices("key-a").await,
        Err(KavitaError::Unreachable)
    ));
}
