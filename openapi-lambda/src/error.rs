use crate::{HeaderName, HttpResponse, StatusCode};

use aws_lambda_events::encodings::Body;
// Until std::error::Backtrace is fully stabilized, we can't embed a type named `Backtrace` within
// a thiserror::Error (see https://github.com/dtolnay/thiserror/issues/204).
use backtrace::Backtrace as _Backtrace;
use headers::{ContentType, Header};
use itertools::Itertools;
use log::error;
use thiserror::Error;

use std::borrow::Cow;
use std::string::FromUtf8Error;

/// Error that occurred while processing an AWS Lambda event.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum EventError {
  /// Failed to prepare HTTP response.
  #[error("failed to prepare HTTP response")]
  HttpResponse(#[source] Box<http::Error>, _Backtrace),
  /// Invalid base64 encoding for request body.
  // The base64 encoding comes from AWS, so this is actually an internal error.
  #[error("invalid base64 encoding for request body")]
  InvalidBodyBase64(#[source] Box<base64::DecodeError>, _Backtrace),
  /// Failed to JSON deserialize request body.
  #[error("failed to JSON deserialize request body")]
  InvalidBodyJson(
    #[source] Box<serde_path_to_error::Error<serde_json::Error>>,
    _Backtrace,
  ),
  /// Invalid UTF-8 encoding for request body.
  #[error("invalid UTF-8 encoding for request body")]
  InvalidBodyUtf8(#[source] Box<FromUtf8Error>, _Backtrace),
  /// Invalid UTF-8 encoding for request header.
  #[error("invalid UTF-8 encoding for request header `{0}`")]
  InvalidHeaderUtf8(
    HeaderName,
    // We don't use `http::header::ToStrError` here since aws_lambda_events uses a different version
    // of `http`.
    #[source] Box<dyn std::error::Error + Send + Sync + 'static>,
    _Backtrace,
  ),
  /// Failed to parse request path parameter.
  #[error("failed to parse request path parameter `{param_name}`")]
  InvalidRequestPathParam {
    /// Name of the parameter that failed to parse.
    param_name: Cow<'static, str>,
    /// Underlying error that occurred while parsing the param.
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    /// Stack trace indicating where the error occurred.
    backtrace: _Backtrace,
  },
  /// Failed to parse request query param.
  #[error("failed to parse request query param `{param_name}`")]
  InvalidRequestQueryParam {
    /// Name of the parameter that failed to parse.
    param_name: Cow<'static, str>,
    /// Underlying error that occurred while parsing the param.
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    /// Stack trace indicating where the error occurred.
    backtrace: _Backtrace,
  },
  /// Missing required request body.
  #[error("missing required request body")]
  MissingRequestBody(_Backtrace),
  /// Missing required request header.
  #[error("missing required request header `{0}`")]
  MissingRequestHeader(Cow<'static, str>, _Backtrace),
  /// Missing required request param.
  #[error("missing required request param `{0}`")]
  MissingRequestParam(Cow<'static, str>, _Backtrace),
  /// Request handler panicked.
  #[error("request handler panicked: {0}")]
  Panic(String, _Backtrace),
  /// Failed to serialize response body to JSON.
  #[error("failed to serialize {type_name} response to JSON")]
  ToJsonResponse {
    /// Name of the response body type that failed to serialize.
    type_name: Cow<'static, str>,
    /// Underlying error that occurred while serializing the response body.
    #[source]
    source: Box<serde_path_to_error::Error<serde_json::Error>>,
    /// Stack trace indicating where the error occurred.
    backtrace: _Backtrace,
  },
  /// Unexpected request Content-Type.
  #[error("unexpected Content-Type `{0}`")]
  UnexpectedContentType(String, _Backtrace),
  /// Unexpected operation ID.
  #[error("unexpected operation ID: {0}")]
  UnexpectedOperationId(String, _Backtrace),
}

impl EventError {
  /// Return the backtrace associated with the error, if known.
  pub fn backtrace(&self) -> Option<&_Backtrace> {
    match self {
      EventError::HttpResponse(_, backtrace)
      | EventError::InvalidBodyBase64(_, backtrace)
      | EventError::InvalidBodyJson(_, backtrace)
      | EventError::InvalidBodyUtf8(_, backtrace)
      | EventError::InvalidHeaderUtf8(_, _, backtrace)
      | EventError::InvalidRequestPathParam { backtrace, .. }
      | EventError::InvalidRequestQueryParam { backtrace, .. }
      | EventError::MissingRequestBody(backtrace)
      | EventError::MissingRequestHeader(_, backtrace)
      | EventError::MissingRequestParam(_, backtrace)
      | EventError::Panic(_, backtrace)
      | EventError::ToJsonResponse { backtrace, .. }
      | EventError::UnexpectedContentType(_, backtrace)
      | EventError::UnexpectedOperationId(_, backtrace) => Some(backtrace),
    }
  }

  /// Return the name of the error variant (e.g., `InvalidBodyBase64`).
  pub fn name(&self) -> &str {
    match self {
      EventError::HttpResponse(_, _) => "HttpResponse",
      EventError::InvalidBodyBase64(_, _) => "InvalidBodyBase64",
      EventError::InvalidBodyJson(_, _) => "InvalidBodyJson",
      EventError::InvalidBodyUtf8(_, _) => "InvalidBodyUtf8",
      EventError::InvalidHeaderUtf8(_, _, _) => "InvalidHeaderUtf8",
      EventError::InvalidRequestPathParam { .. } => "InvalidRequestPathParam",
      EventError::InvalidRequestQueryParam { .. } => "InvalidRequestQueryParam",
      EventError::MissingRequestBody(_) => "MissingRequestBody",
      EventError::MissingRequestHeader(_, _) => "MissingRequestHeader",
      EventError::MissingRequestParam(_, _) => "MissingRequestParam",
      EventError::Panic(_, _) => "Panic",
      EventError::ToJsonResponse { .. } => "ToJsonResponse",
      EventError::UnexpectedContentType(_, _) => "UnexpectedContentType",
      EventError::UnexpectedOperationId(_, _) => "UnexpectedOperationId",
    }
  }
}

impl From<EventError> for HttpResponse {
  /// Build a client-facing [`HttpResponse`] appropriate for the error that occurred.
  ///
  /// This function will set the appropriate HTTP status code (400 or 500) depending on whether the
  /// error is internal (500) or caused by the client (400). For client errors, the
  /// response body contains a human-readable description of the error and the `Content-Type`
  /// response header is set to `text/plain`. For internal errors, no response body is returned to
  /// the client.
  fn from(err: EventError) -> HttpResponse {
    let (status_code, body) = match err {
      // 400
      EventError::InvalidBodyJson(err, _) => (
        StatusCode::BAD_REQUEST,
        // We expose parse errors to the client to provide better 400 Bad Request diagnostics.
        Some(if err.path().iter().next().is_none() {
          format!("Invalid request body: {}", err.inner())
        } else {
          format!(
            "Invalid request body (path: `{}`): {}",
            err.path(),
            err.inner()
          )
        }),
      ),
      EventError::InvalidBodyUtf8(_, _) => (
        StatusCode::BAD_REQUEST,
        Some("Request body must be UTF-8 encoded".to_string()),
      ),
      EventError::InvalidHeaderUtf8(header_name, _, _) => (
        StatusCode::BAD_REQUEST,
        Some(format!(
          "Invalid value for header `{header_name}`: must be UTF-8 encoded"
        )),
      ),
      EventError::InvalidRequestPathParam { param_name, .. } => (
        StatusCode::BAD_REQUEST,
        Some(format!("Invalid `{param_name}` request path parameter")),
      ),
      EventError::InvalidRequestQueryParam { param_name, .. } => (
        StatusCode::BAD_REQUEST,
        Some(format!("Invalid `{param_name}` query parameter")),
      ),
      EventError::MissingRequestBody(_) => (
        StatusCode::BAD_REQUEST,
        Some("Missing request body".to_string()),
      ),
      EventError::MissingRequestHeader(header_name, _) => (
        StatusCode::BAD_REQUEST,
        Some(format!("Missing request header `{header_name}`")),
      ),
      EventError::MissingRequestParam(param_name, _) => (
        StatusCode::BAD_REQUEST,
        Some(format!("Missing required parameter `{param_name}`")),
      ),
      EventError::UnexpectedContentType(content_type, _) => (
        StatusCode::BAD_REQUEST,
        Some(format!("Unexpected content type `{content_type}`")),
      ),
      // 500
      EventError::HttpResponse(_, _)
      | EventError::InvalidBodyBase64(_, _)
      | EventError::Panic(_, _)
      | EventError::ToJsonResponse { .. }
      | EventError::UnexpectedOperationId(_, _) => (StatusCode::INTERNAL_SERVER_ERROR, None),
    };

    let mut response = if let Some(body_str) = body {
      error!("Responding with error status {status_code}: {body_str}");

      let mut response = HttpResponse::new(Body::Text(body_str));
      response.headers_mut().insert(
        ContentType::name().to_owned(),
        ContentType::text()
          .to_string()
          .try_into()
          .expect("MIME type should be a valid header"),
      );

      response
    } else {
      error!("Responding with error status {status_code}");

      HttpResponse::new(Body::Empty)
    };

    *response.status_mut() = status_code;

    response
  }
}

/// Helper function for formatting an error as a string containing a human-readable chain of causes.
///
/// This function will walk over the chain of causes returned by
/// [`Error::source`](std::error::Error::source) and append each underlying error (using the
/// [`Display`](std::fmt::Display) trait).
///
/// # Arguments
///
/// * `err` - Error to format.
/// * `name` - Optional name of the error type/variant (e.g., `EventError::InvalidBodyJson`).
/// * `backtrace` - Optional [`Backtrace`](backtrace::Backtrace) indicating where the top-level
///   error occurred.
pub fn format_error(
  err: &(dyn std::error::Error),
  name: Option<&str>,
  backtrace: Option<&_Backtrace>,
) -> String {
  let err_line = name
    .map(|n| format!("{}: {}", n, err))
    .unwrap_or_else(|| err.to_string());

  let top_error = if let Some(bt) = backtrace {
    format!("{err_line}\n  stack trace:\n{}", format_backtrace(bt, 4))
  } else {
    err_line
  };

  let cause_str = ErrorCauseIterator(err.source())
    .map(|cause| format!("  caused by: {cause}"))
    .join("\n");

  if !cause_str.is_empty() {
    format!("{top_error}\n{cause_str}")
  } else {
    top_error
  }
}

struct ErrorCauseIterator<'a>(Option<&'a (dyn std::error::Error + 'static)>);

impl<'a> Iterator for ErrorCauseIterator<'a> {
  type Item = &'a (dyn std::error::Error + 'static);

  fn next(&mut self) -> Option<Self::Item> {
    let current = self.0;
    self.0 = current.and_then(|err| err.source());
    current
  }
}

fn format_backtrace(backtrace: &_Backtrace, indent: usize) -> String {
  let indent_str = " ".repeat(indent);
  format!("{backtrace:?}")
    .lines()
    .join(&format!("{indent_str}\n"))
}
