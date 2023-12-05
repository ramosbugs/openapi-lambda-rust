use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode, Url};
use serde_json::json;

// FIXME: run this in CI.
#[tokio::test]
// Since this test depends on the API running separately (either locally or in AWS), we only run
// the test when specifically requested (see
// https://doc.rust-lang.org/book/ch11-02-running-tests.html#ignoring-some-tests-unless-specifically-requested).
#[ignore]
async fn test_integration() {
  env_logger::init();

  let base_url = Url::parse(
    &std::env::var("PETSTORE_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string()),
  )
  .unwrap();

  let client = Client::new();

  // Success.
  {
    let response = client
      .post(base_url.join("pet").unwrap())
      .header("Authorization", "Bearer foobar")
      .header("Content-Type", "application/json")
      .body(r#"{"name": "foo", "photoUrls": []}"#)
      .send()
      .await
      .unwrap_or_else(|err| panic!("request failed: {}", err));

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
      response.json::<serde_json::Value>().await.unwrap(),
      json!({
        "name": "foo",
        "photoUrls": []
      })
    );
  }

  // Missing `Authorization` header.
  {
    let response = client
      .post(base_url.join("pet").unwrap())
      .header("Content-Type", "application/json")
      .body(r#"{"name": "foo", "photoUrls": []}"#)
      .send()
      .await
      .unwrap_or_else(|err| panic!("request failed: {}", err));

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
  }

  // Invalid bearer token.
  {
    let response = client
      .post(base_url.join("pet").unwrap())
      .header("Authorization", "Bearer baz")
      .header("Content-Type", "application/json")
      .body(r#"{"name": "foo", "photoUrls": []}"#)
      .send()
      .await
      .unwrap_or_else(|err| panic!("request failed: {}", err));

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
  }

  // Invalid request body.
  {
    let response = client
      .post(base_url.join("pet").unwrap())
      .header("Authorization", "Bearer foobar")
      .header("Content-Type", "application/json")
      .body(r#"{}"#)
      .send()
      .await
      .unwrap_or_else(|err| panic!("request failed: {}", err));

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }

  // Wrong Content-Type.
  {
    let response = client
      .post(base_url.join("pet").unwrap())
      .header("Authorization", "Bearer foobar")
      .header("Content-Type", "test/plain")
      .body(r#"{"name": "foo", "photoUrls": []}"#)
      .send()
      .await
      .unwrap_or_else(|err| panic!("request failed: {}", err));

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }

  // Invalid endpoint.
  {
    let response = client
      .post(base_url.join("pets").unwrap())
      .send()
      .await
      .unwrap_or_else(|err| panic!("request failed: {}", err));

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
  }
}
