use openapi_lambda::run_lambda;
use petstore::middleware::ApiMiddleware;
use petstore::user::Api;
use petstore::user_handler::UserApiHandler;

#[tokio::main]
pub async fn main() {
  // TIP: Use the `log4rs` crate for more fine-grained control over logging.
  env_logger::init();

  let api = UserApiHandler::new(());
  let middleware = ApiMiddleware::new(());

  run_lambda(|event| api.dispatch_request(event, &middleware)).await
}
