#![allow(unused_imports)]

use crate::store::{
  Api, DeleteOrderResponse, GetInventoryResponse, GetOrderByIdResponse, PlaceOrderResponse,
};
use crate::{AuthenticatedUser, HandlerError};

use openapi_lambda::__private::anyhow;
use openapi_lambda::__private::aws_lambda_events::encodings::Body;
use openapi_lambda::async_trait::async_trait;
use openapi_lambda::{
  ApiGatewayProxyRequestContext, HeaderMap, HttpResponse, LambdaContext, StatusCode,
};

pub struct StoreApiHandler {
  // Store any handler state (e.g., DB client) here.
  _state: (),
}

impl StoreApiHandler {
  pub fn new(state: ()) -> Self {
    Self { _state: state }
  }
}

#[async_trait]
impl Api for StoreApiHandler {
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

  async fn delete_order(
    &self,
    _order_id: i64,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(DeleteOrderResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn get_inventory(
    &self,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(GetInventoryResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn get_order_by_id(
    &self,
    _order_id: i64,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(GetOrderByIdResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn place_order(
    &self,
    _request_body: Option<crate::models::Order>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(PlaceOrderResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }
}
