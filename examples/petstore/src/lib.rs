use openapi_lambda::{Body, HeaderName, HttpResponse, StatusCode};
use thiserror::Error;

use std::error::Error;

// Include the generated code produced by build.rs.
include!(concat!(env!("OUT_DIR"), "/out.rs"));

/// Pet API implementation.
pub mod pet_handler;

/// Store API implementation.
pub mod store_handler;

/// User API implementation.
pub mod user_handler;

/// API middleware implementation.
pub mod middleware;

/// Example [`AuthOk`](openapi_lambda::Middleware::AuthOk) type.
#[derive(Debug)]
pub struct AuthenticatedUser {
  pub user_id: String,
}

/// Example handler error type used by each API implementation.
#[derive(Debug, Error)]
pub enum HandlerError {
  #[error("access denied")]
  AccessDenied,
  #[error("missing bearer token")]
  BearerTokenRequired,
  #[error("failed to connect to database")]
  DatabaseConnectionError(#[source] Box<dyn Error + Send + Sync + 'static>),
  #[error("invalid bearer token")]
  InvalidBearerToken,
  #[error("failed to parse request header `{0}`")]
  RequestHeaderParse(HeaderName, #[source] Box<dyn Error + Send + Sync + 'static>),
}

impl From<HandlerError> for HttpResponse {
  fn from(err: HandlerError) -> Self {
    let mut response = HttpResponse::new(Body::Empty);
    *response.status_mut() = match err {
      HandlerError::AccessDenied => StatusCode::FORBIDDEN,
      HandlerError::BearerTokenRequired | HandlerError::InvalidBearerToken => {
        StatusCode::UNAUTHORIZED
      }
      HandlerError::DatabaseConnectionError(_) => StatusCode::INTERNAL_SERVER_ERROR,
      // TIP: Consider returning a helpful response body for 400 client errors (see
      // `impl From<openapi_lambda::EventError> for HttpResponse`). This could be plaintext, JSON,
      // etc. depending on the API.
      HandlerError::RequestHeaderParse(_, _) => StatusCode::BAD_REQUEST,
    };

    response
  }
}
