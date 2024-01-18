#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use aws_lambda_events::apigw::ApiGatewayProxyResponse;

// These are documented public exports since either the generated `Api` traits or the `Middleware`
// depends on them.
pub use async_trait;
pub use aws_lambda_events::apigw::ApiGatewayProxyRequestContext;
pub use aws_lambda_events::encodings::Body;
pub use aws_lambda_events::http::{HeaderMap, HeaderName};
pub use http::{Response, StatusCode};
pub use lambda_runtime::{Context as LambdaContext, LambdaEvent};

/// Error handling.
pub mod error;

pub use error::EventError;

mod middleware;

pub use middleware::{Middleware, UnauthenticatedMiddleware};

/// Request/response model-related types and re-exports.
pub mod models;

mod runtime;

pub use runtime::run_lambda;

/// HTTP response.
pub type HttpResponse = Response<Body>;

/// Serialize an [`HttpResponse`] as an [`ApiGatewayProxyResponse`].
pub fn http_response_to_apigw(response: HttpResponse) -> ApiGatewayProxyResponse {
  let (parts, body) = response.into_parts();
  ApiGatewayProxyResponse {
    status_code: parts.status.as_u16() as i64,
    headers: Default::default(),
    multi_value_headers: parts.headers,
    body: Some(body),
    is_base64_encoded: false,
  }
}

// Used by generated code. Not part of the public API. Not bound by SemVer. Each release of
// `openapi-lambda-codegen` is guaranteed to be compatible only with the identical version number
// of `openapi-lambda`.
#[doc(hidden)]
#[path = "private/mod.rs"]
pub mod __private;
