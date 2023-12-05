#![allow(unused_imports)]

use crate::pet::{
  AddPetResponse, Api, DeletePetResponse, FindPetsByStatusResponse, FindPetsByTagsResponse,
  GetPetByIdResponse, UpdatePetResponse, UpdatePetWithFormResponse, UploadFileResponse,
};
use crate::{AuthenticatedUser, HandlerError};

use openapi_lambda::__private::anyhow;
use openapi_lambda::__private::aws_lambda_events::encodings::Body;
use openapi_lambda::async_trait::async_trait;
use openapi_lambda::{
  ApiGatewayProxyRequestContext, HeaderMap, HttpResponse, LambdaContext, StatusCode,
};

pub struct PetApiHandler {
  // Store any handler state (e.g., DB client) here.
  _state: (),
}

impl PetApiHandler {
  pub fn new(state: ()) -> Self {
    Self { _state: state }
  }
}

#[async_trait]
impl Api for PetApiHandler {
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

  async fn add_pet(
    &self,
    request_body: crate::models::Pet,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(AddPetResponse, HeaderMap), Self::HandlerError> {
    Ok((
      AddPetResponse::Ok(crate::models::Pet {
        id: request_body.id,
        name: request_body.name,
        category: request_body.category,
        photo_urls: request_body.photo_urls,
        tags: request_body.tags,
        status: request_body.status,
      }),
      HeaderMap::default(),
    ))
  }

  async fn delete_pet(
    &self,
    _api_key: Option<String>,
    _pet_id: i64,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(DeletePetResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn find_pets_by_status(
    &self,
    _status: Option<crate::models::FindPetsByStatusStatusParam>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(FindPetsByStatusResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn find_pets_by_tags(
    &self,
    _tags: Option<Vec<String>>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(FindPetsByTagsResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn get_pet_by_id(
    &self,
    _pet_id: i64,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(GetPetByIdResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn update_pet(
    &self,
    _request_body: crate::models::Pet,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(UpdatePetResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn update_pet_with_form(
    &self,
    _pet_id: i64,
    _name: Option<String>,
    _status: Option<String>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(UpdatePetWithFormResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }

  async fn upload_file(
    &self,
    _pet_id: i64,
    _additional_metadata: Option<String>,
    _request_body: Option<Vec<u8>>,
    _headers: HeaderMap,
    _request_context: ApiGatewayProxyRequestContext,
    _lambda_context: LambdaContext,
    _auth_ok: Self::AuthOk,
  ) -> Result<(UploadFileResponse, HeaderMap), Self::HandlerError> {
    todo!()
  }
}
