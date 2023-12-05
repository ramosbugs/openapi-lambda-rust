use aws_lambda_events::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use futures::FutureExt;
use lambda_runtime::{service_fn, LambdaEvent};

use std::future::Future;

/// Start the Lambda runtime to handle requests for the specified API using the specified
/// middleware.
///
/// # Example
///
/// ```rust,ignore
/// // Replace `my_api` with the name of your crate and `backend` with the name of the module
/// // passed to `ApiLambda::new()`.
/// use my_api::backend::Api;
/// use my_api::backend_handler::BackendApiHandler;
/// use openapi_lambda::run_lambda;
///
/// #[tokio::main]
/// pub async fn main() {
///   let api = BackendApiHandler::new(...);
///   let middleware = ...; // Instantiate your middleware here.
///
///   run_lambda(|event| api.dispatch_request(event, &middleware)).await
/// }
/// ```
pub async fn run_lambda<F, Fut>(mut dispatch_event: F)
where
  F: FnMut(LambdaEvent<ApiGatewayProxyRequest>) -> Fut,
  Fut: Future<Output = ApiGatewayProxyResponse>,
{
  lambda_runtime::run(service_fn(|event: LambdaEvent<ApiGatewayProxyRequest>| {
    dispatch_event(event).map(Result::<_, std::convert::Infallible>::Ok)
  }))
  .await
  .expect("Lambda run loop should never exit")
}
