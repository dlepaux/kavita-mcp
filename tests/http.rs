//! The listener: which routes need what, and the whole tool path end to end through rmcp's own client
//! (Streamable HTTP, bearer, form elicitation) against a wiremock Kavita.

use std::sync::Arc;
use std::time::Duration;

use kavita_mcp::config::{Identity, Language};
use kavita_mcp::handles::Handles;
use kavita_mcp::http::{build_router, App};
use kavita_mcp::kavita::Kavita;
use kavita_mcp::metrics::Metrics;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, ElicitRequestParams, ElicitResult,
    ElicitationAction, ElicitationCapability, FormElicitationCapability, Implementation,
};
use rmcp::service::{RequestContext, RoleClient};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientHandler, ErrorData, ServiceExt};
use serde_json::json;
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ALICE_TOKEN: &str = "token-alice";
const BOB_TOKEN: &str = "token-bob";

fn person(profile: &str, token: &str, language: Language) -> Identity {
    Identity {
        profile: profile.into(),
        token_sha256: Sha256::digest(token.as_bytes()).into(),
        language,
        libraries: vec!["Fiction".into()],
        kavita_key: format!("key-{profile}"),
        admin: None,
    }
}

fn app(kavita_url: &str) -> Arc<App> {
    Arc::new(App {
        kavita: Kavita::new(kavita_url, Duration::from_secs(5)),
        metrics: Metrics::new(),
        handles: Handles::default(),
        identities: vec![
            person("alice", ALICE_TOKEN, Language::parse("fr").unwrap()),
            person("bob", BOB_TOKEN, Language::parse("en").unwrap()),
        ],
        send_size_limit: 26_214_400,
    })
}

async fn serve(app: Arc<App>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, build_router(app)).await.unwrap() });
    format!("http://{addr}")
}

fn counter(app: &App, name: &str, kind: &str) -> String {
    let prefix = format!("{name}{{kind=\"{kind}\"}} ");
    app.metrics
        .render()
        .lines()
        .find_map(|l| l.strip_prefix(&prefix).map(str::to_string))
        .unwrap()
}

#[tokio::test]
async fn health_and_metrics_need_no_token() {
    let base = serve(app("http://127.0.0.1:9")).await;
    let health = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(health.status(), 200);
    let metrics = reqwest::get(format!("{base}/metrics")).await.unwrap();
    assert_eq!(metrics.status(), 200);
    assert!(metrics
        .text()
        .await
        .unwrap()
        .contains("kavita_mcp_auth_failures_total"));
}

#[tokio::test]
async fn mcp_refuses_a_missing_or_unknown_token_and_counts_it() {
    let app = app("http://127.0.0.1:9");
    let base = serve(app.clone()).await;
    let client = reqwest::Client::new();
    let none = client.post(format!("{base}/mcp")).send().await.unwrap();
    assert_eq!(none.status(), 401);
    let wrong = client
        .post(format!("{base}/mcp"))
        .bearer_auth("not-a-token")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    let scheme = client
        .post(format!("{base}/mcp"))
        .header("Authorization", ALICE_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(scheme.status(), 401);
    assert_eq!(
        counter(&app, "kavita_mcp_auth_failures_total", "mcp_token"),
        "3"
    );
}

/// A chat client that answers every confirmation with `answer`, or declares no elicitation at all.
#[derive(Clone)]
struct Chat {
    answer: Option<ElicitationAction>,
    asked: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ClientHandler for Chat {
    fn get_info(&self) -> ClientConfig {
        let mut capabilities = ClientCapabilities::default();
        if self.answer.is_some() {
            capabilities.elicitation =
                Some(ElicitationCapability::new().with_form(FormElicitationCapability::new()));
        }
        ClientConfig::new(capabilities, Implementation::new("test-chat", "0"))
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, ErrorData> {
        if let ElicitRequestParams::FormElicitationParams { message, .. } = request {
            self.asked.lock().unwrap().push(message);
        }
        Ok(ElicitResult::new(
            self.answer.clone().expect("declared elicitation"),
        ))
    }
}

async fn call(
    base: &str,
    token: &str,
    chat: Chat,
    tool: &'static str,
    args: serde_json::Value,
) -> rmcp::model::CallToolResult {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp")).auth_header(token),
    );
    let client = chat.serve(transport).await.expect("initialize");
    let result = client
        .call_tool(
            CallToolRequestParams::new(tool).with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .expect("tool call");
    client.cancel().await.ok();
    result
}

fn text(result: &rmcp::model::CallToolResult) -> String {
    serde_json::to_string(&result.content).unwrap()
}

/// Alice's Kavita: one EPUB in Fiction, one Kindle, email on.
async fn alices_kavita() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/Search/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"series": [
            {"seriesId": 7, "name": "Le Bleu des abysses", "libraryName": "Fiction"}]})))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/Search/chapters-by-series"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "titleName": "", "files": [{"format": 3, "bytes": 2_202_009}]}])))
        .mount(&s)
        .await;
    Mock::given(method("GET")).and(path("/api/Device"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 3, "name": "Kindle d'Alice", "emailAddress": "secret@kindle.com", "platform": 2}]))).mount(&s).await;
    Mock::given(method("GET"))
        .and(path("/api/Settings/is-email-setup"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(true)))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/Device/send-to"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s)
        .await;
    s
}

fn handle_in(result: &rmcp::model::CallToolResult) -> String {
    let t = text(result);
    let i = t.find("handle=").expect("a handle") + 7;
    t[i..i + 6].to_string()
}

async fn search_then_send(
    s: &MockServer,
    token_search: &str,
    token_send: &str,
    answer: Option<ElicitationAction>,
) -> (rmcp::model::CallToolResult, Vec<String>) {
    let base = serve(app(&s.uri())).await;
    let chat = Chat {
        answer,
        asked: Arc::default(),
    };
    let found = call(
        &base,
        token_search,
        chat.clone(),
        "search_library",
        json!({"query": "abysses"}),
    )
    .await;
    let sent = call(
        &base,
        token_send,
        chat.clone(),
        "send_to_kindle",
        json!({"handle": handle_in(&found)}),
    )
    .await;
    let asked = chat.asked.lock().unwrap().clone();
    (sent, asked)
}

async fn send_bodies(s: &MockServer) -> Vec<serde_json::Value> {
    s.received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/api/Device/send-to")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

#[tokio::test]
async fn end_to_end_an_accept_sends_one_chapter_as_the_token_holder() {
    let s = alices_kavita().await;
    let (sent, asked) = search_then_send(
        &s,
        ALICE_TOKEN,
        ALICE_TOKEN,
        Some(ElicitationAction::Accept),
    )
    .await;
    assert!(text(&sent).contains("Envoyé"), "{}", text(&sent));
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].contains("Le Bleu des abysses")
            && asked[0].contains("EPUB")
            && asked[0].contains("2,1 Mo")
            && asked[0].contains("Kindle d'Alice"),
        "{asked:?}"
    );
    assert_eq!(
        send_bodies(&s).await,
        vec![json!({"deviceId": 3, "chapterIds": [10]})]
    );
    let keys: Vec<_> = s
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .map(|r| {
            r.headers
                .get("x-api-key")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(keys.iter().all(|k| k == "key-alice"), "{keys:?}");
}

#[tokio::test]
async fn end_to_end_a_decline_sends_nothing() {
    let s = alices_kavita().await;
    let (sent, _) = search_then_send(
        &s,
        ALICE_TOKEN,
        ALICE_TOKEN,
        Some(ElicitationAction::Decline),
    )
    .await;
    assert!(text(&sent).contains("Pas envoyé"));
    assert!(send_bodies(&s).await.is_empty());
}

#[tokio::test]
async fn end_to_end_another_bearer_cannot_use_the_handle() {
    let s = alices_kavita().await;
    let (sent, asked) =
        search_then_send(&s, ALICE_TOKEN, BOB_TOKEN, Some(ElicitationAction::Accept)).await;
    assert!(
        text(&sent).contains("Search for it again"),
        "{}",
        text(&sent)
    );
    assert!(asked.is_empty() && send_bodies(&s).await.is_empty());
}

#[tokio::test]
async fn end_to_end_a_client_without_elicitation_sends_nothing() {
    let s = alices_kavita().await;
    let (sent, _) = search_then_send(&s, ALICE_TOKEN, ALICE_TOKEN, None).await;
    assert!(
        text(&sent).contains("rien n'a été envoyé"),
        "{}",
        text(&sent)
    );
    assert!(send_bodies(&s).await.is_empty());
}

#[tokio::test]
async fn no_output_ever_carries_an_email_address() {
    let s = alices_kavita().await;
    let base = serve(app(&s.uri())).await;
    let chat = Chat {
        answer: Some(ElicitationAction::Accept),
        asked: Arc::default(),
    };
    let found = call(
        &base,
        ALICE_TOKEN,
        chat.clone(),
        "search_library",
        json!({"query": "abysses"}),
    )
    .await;
    let sent = call(
        &base,
        ALICE_TOKEN,
        chat.clone(),
        "send_to_kindle",
        json!({"handle": handle_in(&found)}),
    )
    .await;
    for out in [text(&found), text(&sent)]
        .iter()
        .chain(chat.asked.lock().unwrap().iter())
    {
        assert!(!out.contains("secret@kindle.com"), "{out}");
    }
}

#[tokio::test]
async fn the_tools_are_listed_with_their_hints() {
    let base = serve(app("http://127.0.0.1:9")).await;
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp")).auth_header(BOB_TOKEN),
    );
    let client = Chat {
        answer: None,
        asked: Arc::default(),
    }
    .serve(transport)
    .await
    .unwrap();
    let tools = client.list_all_tools().await.unwrap();
    client.cancel().await.ok();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let search = tools.iter().find(|t| t.name == "search_library").unwrap();
    assert_eq!(
        search.annotations.as_ref().and_then(|a| a.read_only_hint),
        Some(true)
    );
    // Kavita's series search ignores people: the schema must not promise an author search.
    let query = search.input_schema["properties"]["query"]["description"].to_string();
    assert!(!query.contains("author"), "{query}");
    let send = tools.iter().find(|t| t.name == "send_to_kindle").unwrap();
    assert_ne!(
        send.annotations.as_ref().and_then(|a| a.read_only_hint),
        Some(true)
    );
}
