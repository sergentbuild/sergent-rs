//! Scripted HTTP boundary invariants.

use std::time::Duration;

use bytes::Bytes;
use reqwest::header::HeaderMap;
use serde_json::json;

use super::response;
use crate::http::{HttpClient, HttpPostRequest, ScriptedHttpClient};

#[tokio::test]
async fn scripted_http_records_requests_and_returns_fifo_typed_outcomes() {
    let http = ScriptedHttpClient::new([Ok(response(201, "first")), Ok(response(202, "second"))]);
    let first = HttpPostRequest {
        url: reqwest::Url::parse("https://first.scripted.invalid").unwrap(),
        headers: HeaderMap::new(),
        body: Bytes::from(serde_json::to_vec(&json!({ "index": 1 })).unwrap()),
        timeout: Duration::from_secs(1),
    };
    let second = HttpPostRequest {
        url: reqwest::Url::parse("https://second.scripted.invalid").unwrap(),
        headers: HeaderMap::new(),
        body: Bytes::from(serde_json::to_vec(&json!({ "index": 2 })).unwrap()),
        timeout: Duration::from_secs(2),
    };

    assert_eq!(http.post(&first).await.expect("first outcome").status, 201);
    assert_eq!(
        http.post(&second).await.expect("second outcome").status,
        202
    );
    assert_eq!(http.recorded_requests(), [first, second]);
}

#[tokio::test]
#[should_panic(expected = "scripted HTTP outcomes exhausted after request 1")]
async fn scripted_http_exhaustion_is_deterministic() {
    let http = ScriptedHttpClient::new([]);
    let request = HttpPostRequest {
        url: reqwest::Url::parse("https://exhausted.scripted.invalid").unwrap(),
        headers: HeaderMap::new(),
        body: Bytes::from_static(b"{}"),
        timeout: Duration::from_secs(1),
    };

    let _ = http.post(&request).await;
}
