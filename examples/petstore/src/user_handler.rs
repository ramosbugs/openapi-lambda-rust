#![allow(unused_imports)]

use crate::user::{
  Api, CreateUserResponse, CreateUsersWithListInputResponse, DeleteUserResponse,
  GetUserByNameResponse, LoginUserResponse, LogoutUserResponse, UpdateUserResponse,
};
use crate::{AuthenticatedUser, HandlerError};

use openapi_lambda::__private::anyhow;
use openapi_lambda::__private::aws_lambda_events::encodings::Body;
use openapi_lambda::async_trait::async_trait;
use openapi_lambda::{
  ApiGatewayProxyRequestContext, HeaderMap, HttpResponse, LambdaContext, StatusCode,
};

pub struct UserApiHandler {
  // Store any handler state (e.g., DB client) here.
  _state: (),
}

impl UserApiHandler {
  pub fn new(state: ()) -> Self {
    Self { _state: state }
  }
}

#[async_trait]
impl Api for UserApiHandler {
  // Define a type here to represent a successfully authenticated user.
  type AuthOk = AuthenticatedUser;

  // Define an error type to capture the errors produced by your API handler methods.
  type HandlerError = HandlerError;

  // Return an error response depending on the nature of the error (e.g., 400 Bad Request for
  // errors caused by a client sending an invalid request, or 500 Internal Server Error for
  // internal errors such as failing to connect to a database).
  async fn respond_to_handler_error(&self, err: Self::HandlerError) -> HttpResponse {
    err.into()
  }

  async fn create_user(
    &self,
    _request_body: Option<crate::models::User>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(CreateUserResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn create_users_with_list_input(
    &self,
    _request_body: Option<Vec<crate::models::User>>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(CreateUsersWithListInputResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn delete_user(
    &self,
    _username: String,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(DeleteUserResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn get_user_by_name(
    &self,
    _username: String,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(GetUserByNameResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn login_user(
    &self,
    _username: Option<String>,
    _password: Option<String>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(LoginUserResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn logout_user(
    &self,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(LogoutUserResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn update_user(
    &self,
    _username: String,
    _request_body: Option<crate::models::User>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(UpdateUserResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }
}
