use crate::{AuthenticatedUser, HandlerError};

use headers::authorization::Bearer;
use headers::{Authorization, Header};
use openapi_lambda::async_trait::async_trait;
use openapi_lambda::{
  ApiGatewayProxyRequestContext, HeaderMap, HttpResponse, LambdaContext, Middleware,
};

pub struct ApiMiddleware {
  // Store any middleware state (e.g., DB client) here.
  _state: (),
}

impl ApiMiddleware {
  pub fn new(state: ()) -> Self {
    Self { _state: state }
  }
}

#[async_trait]
impl Middleware for ApiMiddleware {
  type AuthOk = AuthenticatedUser;

  async fn authenticate(
    &self,
    _operation_id: &str,
    headers: &HeaderMap,
    _request_context: &ApiGatewayProxyRequestContext,
    _lambda_context: &LambdaContext,
  ) -> Result<Self::AuthOk, HttpResponse> {
    let bearer_token = headers
      .get(Authorization::<Bearer>::name())
      .map(|header| {
        header.to_str().map_err(|err| {
          HandlerError::RequestHeaderParse(
            Authorization::<Bearer>::name().to_owned(),
            Box::new(err),
          )
        })
      })
      .transpose()?
      .map(|header| {
        header.strip_prefix("Bearer ").ok_or_else(|| {
          HandlerError::RequestHeaderParse(
            Authorization::<Bearer>::name().to_owned(),
            Box::new(headers::Error::invalid()),
          )
        })
      })
      .transpose()?
      .ok_or(HandlerError::BearerTokenRequired)?;

    // PLACEHOLDER ONLY: be sure to parse/validate/lookup the bearer token as appropriate for the
    // type of authentication used for your api.
    if bearer_token == "foobar" {
      Ok(AuthenticatedUser {
        user_id: "123".to_string(),
      })
    } else {
      Err(HandlerError::InvalidBearerToken.into())
    }
  }
}
