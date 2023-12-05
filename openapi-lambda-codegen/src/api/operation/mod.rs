use crate::api::operation::parameter::RequestParameter;
use crate::inline::InlineApi;
use crate::reference::resolve_local_reference;
use crate::{description_to_doc_attr, CodeGenerator};

use convert_case::{Case, Casing};
use http::Method;
use indexmap::IndexMap;
use openapiv3::{Operation, PathItem, ReferenceOr, Schema};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use unzip_n::unzip_n;

use std::borrow::Cow;
use std::collections::HashMap;

mod parameter;
mod request_body;
mod response;

unzip_n!(5);

/// A single API operation (e.g., `GET /foo`).
pub(crate) struct PathOperation {
  pub method: Method,
  pub op: Operation,
  /// HTTP request path of the operation (e.g., `/foo`).
  pub request_path: String,
}

/// Collect all API operations into a flattened `Vec`.
pub(crate) fn collect_operations(
  openapi: &InlineApi,
  openapi_inline_mapping: &serde_yaml::Mapping,
) -> Vec<PathOperation> {
  openapi
    .paths
    .iter()
    .flat_map(|(request_path, path_item_or_ref)| {
      let path_item = match path_item_or_ref {
        ReferenceOr::Item(path_item) => Cow::Borrowed(path_item),
        ReferenceOr::Reference { reference } => {
          Cow::Owned(resolve_local_reference::<PathItem>(reference, openapi_inline_mapping).target)
        }
      };

      match path_item {
        Cow::Borrowed(item) => vec![
          item.get.as_ref().map(|op| (Method::GET, op.to_owned())),
          item.put.as_ref().map(|op| (Method::PUT, op.to_owned())),
          item.post.as_ref().map(|op| (Method::POST, op.to_owned())),
          item
            .delete
            .as_ref()
            .map(|op| (Method::DELETE, op.to_owned())),
          item
            .options
            .as_ref()
            .map(|op| (Method::OPTIONS, op.to_owned())),
          item.head.as_ref().map(|op| (Method::HEAD, op.to_owned())),
          item.patch.as_ref().map(|op| (Method::PATCH, op.to_owned())),
          item.trace.as_ref().map(|op| (Method::TRACE, op.to_owned())),
        ],
        Cow::Owned(item) => vec![
          item.get.map(|op| (Method::GET, op)),
          item.put.map(|op| (Method::PUT, op)),
          item.post.map(|op| (Method::POST, op)),
          item.delete.map(|op| (Method::DELETE, op)),
          item.options.map(|op| (Method::OPTIONS, op)),
          item.head.map(|op| (Method::HEAD, op)),
          item.patch.map(|op| (Method::PATCH, op)),
          item.trace.map(|op| (Method::TRACE, op)),
        ],
      }
      .into_iter()
      .flatten()
      .map(move |(method, op)| PathOperation {
        method,
        op,
        request_path: request_path.to_owned(),
      })
    })
    .collect()
}

/// A generated single API operation (e.g., `GET /foo`).
pub struct ApiOperation {
  /// Match case for the API dispatcher from `operation_id` to the handler wrapper.
  pub api_dispatcher_case: TokenStream,

  /// Handler function the user must implement.
  pub handler_impl: TokenStream,

  /// Prototype for the handler function the user must implement.
  pub handler_prototype: TokenStream,

  /// Definition for wrapper function for the handler that parses parameters and implements logging,
  /// authentication, etc.
  ///
  /// This function calls the user's handler.
  pub handler_wrapper: TokenStream,

  /// Definition for operation response type enum with one variant for each HTTP status code.
  pub response_type_enum: TokenStream,

  /// Identifier for the operation response type.
  pub response_type_ident: Ident,
}

impl CodeGenerator {
  pub(crate) fn gen_api_operation(
    &self,
    mod_name: &str,
    operation: &PathOperation,
    openapi_inline: &serde_yaml::Mapping,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: &HashMap<Ident, TokenStream>,
  ) -> ApiOperation {
    let PathOperation {
      method,
      request_path,
      op,
    } = operation;

    let operation_id = op
      .operation_id
      .as_ref()
      .unwrap_or_else(|| panic!("no operation_id for {request_path}"));

    let request_body = op
      .request_body
      .as_ref()
      .map(|request_body| match request_body {
        ReferenceOr::Item(request) => Cow::Borrowed(request),
        ReferenceOr::Reference { reference } => {
          Cow::Owned(resolve_local_reference(reference, openapi_inline).target)
        }
      });

    let body_parameter = request_body.and_then(|request_body| {
      self.gen_request_body(
        request_path,
        request_body.as_ref(),
        openapi_inline,
        components_schemas,
        generated_models,
      )
    });

    let (param_call_values, log_params, param_doc_attrs, param_signatures, param_parse_assignments) =
      op.parameters
        .iter()
        .map(|parameter| match parameter {
          ReferenceOr::Reference { reference } => self.gen_request_parameter(
            &resolve_local_reference(reference, openapi_inline).target,
            components_schemas,
            generated_models,
          ),
          ReferenceOr::Item(parameter) => {
            self.gen_request_parameter(parameter, components_schemas, generated_models)
          }
        })
        .chain(body_parameter)
        .map(
          |RequestParameter {
             call_value,
             doc_attr,
             log_param,
             signature,
             wrapper_parse_assignment,
           }| {
            (
              call_value,
              log_param,
              doc_attr,
              signature,
              wrapper_parse_assignment,
            )
          },
        )
        .unzip_n::<TokenStream, TokenStream, TokenStream, TokenStream, TokenStream>();

    let func_name_snake = operation_id.to_case(Case::Snake);
    let func_name_ident = self.identifier(&func_name_snake);
    let handler_wrapper_name_ident =
      Ident::new(&format!("handle_{func_name_snake}"), Span::call_site());
    let response_type_ident =
      self.identifier(&format!("{}Response", operation_id.to_case(Case::Pascal)));

    let response_type_enum = self.gen_operation_response_type_enum(
      mod_name,
      &func_name_snake,
      &response_type_ident,
      operation,
      openapi_inline,
      components_schemas,
      generated_models,
    );

    let is_unauthenticated = op
      .security
      .as_ref()
      .map(|security| security.iter().any(|sec| sec.is_empty()))
      .unwrap_or(false);
    let (maybe_authenticate, auth_ok_proto_arg, auth_ok_doc_attr, auth_ok_call_arg, wrapper) =
      if is_unauthenticated {
        (
          quote! {
            log::debug!("Request does not require authentication");
          },
          quote! {},
          quote! {},
          quote! {},
          quote! { wrap_handler_unauthed },
        )
      } else {
        (
          quote! {
            log::trace!("Authenticating request");
            let auth_ok = match middleware.authenticate(
              #operation_id,
              &request.headers,
              &request.request_context,
              &lambda_context,
            ).await {
              Ok(auth_ok) => auth_ok,
              Err(err) => return err,
            };
          },
          quote! {
            auth_ok: Self::AuthOk,
          },
          quote! {
            /// * `auth_ok` - Output of [`Middleware::authenticate`] representing the authenticated
            ///   user's identity
          },
          quote! {
            auth_ok,
          },
          quote! { wrap_handler_authed },
        )
      };

    let description_doc_attr = op
      .description
      .as_ref()
      .map(|description| {
        let doc_attr = description_to_doc_attr(description);
        quote! {
          #doc_attr
          ///
        }
      })
      .unwrap_or_default();

    let method_upper = method.as_str();
    let handler_prototype = quote! {
      #description_doc_attr
      #[doc = concat!("Endpoint: `", #method_upper, " ", #request_path, "`")]
      ///
      #[doc = concat!("Operation ID: `", #operation_id, "`")]
      ///
      /// # Arguments
      ///
      #param_doc_attrs
      /// * `headers` - HTTP request headers
      /// * `request_context` - API Gateway request context. Contains information about the AWS
      ///   account/resources that invoked the Lambda function and Cognito identity information
      ///   about the client (if configured for the API Gateway).
      /// * `lambda_context` Lambda function execution context
      #auth_ok_doc_attr
      async fn #func_name_ident(
        &self,
        #param_signatures
        headers: HeaderMap,
        request_context: ApiGatewayProxyRequestContext,
        lambda_context: LambdaContext,
        #auth_ok_proto_arg
      ) -> Result<(#response_type_ident, HeaderMap), Self::HandlerError>;
    };

    let handler_impl = quote! {
      async fn #func_name_ident(
        &self,
        #param_signatures
        headers: HeaderMap,
        request_context: ApiGatewayProxyRequestContext,
        lambda_context: LambdaContext,
        #auth_ok_proto_arg
      ) -> Result<(#response_type_ident, HeaderMap), Self::HandlerError> {
        todo!()
      }
    };

    let handler_wrapper = quote! {
      async fn #handler_wrapper_name_ident<A, M>(
        api: &A,
        request: ApiGatewayProxyRequest,
        lambda_context: LambdaContext,
        middleware: &M,
      )-> HttpResponse
      where
        A: Api<AuthOk = <M as Middleware>::AuthOk> + Sync,
        M: Middleware + Sync,
      {
        log::info!(concat!("Handling HTTP ", #method_upper, " {} ({})"), #request_path, #operation_id);

        #param_parse_assignments
        #log_params

        #maybe_authenticate

        middleware.#wrapper(
          |headers, request_context, lambda_context, #auth_ok_call_arg| async move {
            let (response, response_headers) = match api
              .#func_name_ident(
                #param_call_values
                headers,
                request_context,
                lambda_context,
                #auth_ok_call_arg
              )
              .await
            {
              Ok((response, response_headers)) => (response, response_headers),
              Err(err) => return api.respond_to_handler_error(err).await,
            };

            log::trace!("Response: {response:#?}");
            log::trace!("Returning response headers: {response_headers:#?}");

            match response.into_http_response(response_headers) {
              Ok(response) => response,
              Err(err) => api.respond_to_event_error(err).await,
            }
          },
          #operation_id,
          request.headers,
          request.request_context,
          lambda_context,
          #auth_ok_call_arg
        )
        .await
      }
    };

    let api_dispatcher_case = quote! {
      #operation_id => #handler_wrapper_name_ident(
        api,
        request,
        lambda_context,
        middleware,
      ).await,
    };

    ApiOperation {
      api_dispatcher_case,
      handler_impl,
      handler_prototype,
      handler_wrapper,
      response_type_enum,
      response_type_ident,
    }
  }
}
