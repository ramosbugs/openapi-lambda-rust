use crate::{ApiGatewayProxyRequestContext, HeaderMap, HttpResponse, LambdaContext};

use async_trait::async_trait;

use std::future::Future;

/// Middleware interface for handling request authentication and optionally wrapping each request
/// (e.g., to perform logging/telemetry).
///
/// This trait is intended to be used with the [`#[async_trait]`](async_trait::async_trait)
/// attribute.
#[async_trait]
pub trait Middleware {
  /// Type returned by a successful call to [`authenticate`](Middleware::authenticate).
  ///
  /// This might represent a user, authentication session, or other abstraction relevant to
  /// your API. If none of the API endpoints require authentication, simply use the unit type
  /// (`()`).
  type AuthOk: Send;

  /// Authenticate the current request.
  ///
  /// # Arguments
  ///
  /// * `operation_id` - Operation ID associated with the current request (as defined in the OpenAPI
  ///   definition).
  /// * `headers` - HTTP request headers (e.g., `Authorization`, `Cookie`, etc.).
  /// * `request_context` - Amazon API Gateway request context containing information to identify
  ///   the AWS account and resources invoking the Lambda function. It also includes Cognito
  ///   identity information for the caller (see the
  ///   [`identity`](ApiGatewayProxyRequestContext::identity) field).
  /// * `lambda_context` - Lambda function execution context.
  async fn authenticate(
    &self,
    operation_id: &str,
    headers: &HeaderMap,
    request_context: &ApiGatewayProxyRequestContext,
    lambda_context: &LambdaContext,
  ) -> Result<Self::AuthOk, HttpResponse>;

  /// Wrap an authenticated request.
  ///
  /// This method serves as an optional hook for running arbitrary code before and/or after each
  /// request handler is invoked. For example, it may be used to implement logging or telemetry, or
  /// to add HTTP response headers prior to returning the handler's [`HttpResponse`] to the client.
  ///
  /// If implemented, this method should invoke the `api_handler` argument as follows:
  /// ```rust,ignore
  /// api_handler(headers, request_context, lambda_context, auth_ok)
  /// ```
  ///
  /// # Arguments
  ///
  /// * `api_handler` - API handler function to invoke.
  /// * `operation_id` - Operation ID associated with the current request (as defined in the OpenAPI
  ///   definition).
  /// * `headers` - HTTP request headers (e.g., `Authorization`, `Cookie`, etc.).
  /// * `request_context` - Amazon API Gateway request context containing information to identify
  ///   the AWS account and resources invoking the Lambda function. It also includes Cognito
  ///   identity information for the caller (see the
  ///   [`identity`](ApiGatewayProxyRequestContext::identity) field).
  /// * `lambda_context` - Lambda function execution context.
  /// * `auth_ok` - Output of successful call to [`authenticate`](Middleware::authenticate) method.
  async fn wrap_handler_authed<F, Fut>(
    &self,
    api_handler: F,
    operation_id: &str,
    headers: HeaderMap,
    request_context: ApiGatewayProxyRequestContext,
    lambda_context: LambdaContext,
    auth_ok: Self::AuthOk,
  ) -> HttpResponse
  where
    F: FnOnce(HeaderMap, ApiGatewayProxyRequestContext, LambdaContext, Self::AuthOk) -> Fut + Send,
    Fut: Future<Output = HttpResponse> + Send,
  {
    let _ = operation_id;
    api_handler(headers, request_context, lambda_context, auth_ok).await
  }

  /// Wrap an unauthenticated request.
  ///
  /// This method serves as an optional hook for running arbitrary code before and/or after each
  /// request handler is invoked. For example, it may be used to implement logging or telemetry, or
  /// to add HTTP response headers prior to returning the handler's [`HttpResponse`] to the client.
  ///
  /// If implemented, this method should invoke the `api_handler` argument as follows:
  /// ```rust,ignore
  /// api_handler(headers, request_context, lambda_context)
  /// ```
  ///
  /// # Arguments
  ///
  /// * `api_handler` - API handler function to invoke.
  /// * `operation_id` - Operation ID associated with the current request (as defined in the OpenAPI
  ///   definition).
  /// * `headers` - HTTP request headers (e.g., `Authorization`, `Cookie`, etc.).
  /// * `request_context` - Amazon API Gateway request context containing information to identify
  ///   the AWS account and resources invoking the Lambda function. It also includes Cognito
  ///   identity information for the caller (see the
  ///   [`identity`](ApiGatewayProxyRequestContext::identity) field).
  /// * `lambda_context` - Lambda function execution context.
  async fn wrap_handler_unauthed<F, Fut>(
    &self,
    api_handler: F,
    operation_id: &str,
    headers: HeaderMap,
    request_context: ApiGatewayProxyRequestContext,
    lambda_context: LambdaContext,
  ) -> HttpResponse
  where
    F: FnOnce(HeaderMap, ApiGatewayProxyRequestContext, LambdaContext) -> Fut + Send,
    Fut: Future<Output = HttpResponse> + Send,
  {
    let _ = operation_id;
    api_handler(headers, request_context, lambda_context).await
  }
}

/// Convenience middleware that performs no request authentication.
///
/// This middleware is intended for two use cases:
///  * APIs without any authenticated endpoints.
///  * APIs with authentication requirements that cannot be handled by
///    [`authenticate`](Middleware::authenticate)
///    (e.g., webhook handlers that require access to the raw request body in order to compute an
///    HMAC). For this use case, each handler function should perform its own authentication rather
///    than via the middleware.
pub struct UnauthenticatedMiddleware;

#[async_trait]
impl Middleware for UnauthenticatedMiddleware {
  type AuthOk = ();

  async fn authenticate(
    &self,
    _operation_id: &str,
    _headers: &HeaderMap,
    _request_context: &ApiGatewayProxyRequestContext,
    _lambda_context: &LambdaContext,
  ) -> Result<Self::AuthOk, HttpResponse> {
    Ok(())
  }
}
