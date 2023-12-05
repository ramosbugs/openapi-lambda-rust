use crate::api::operation::{ApiOperation, PathOperation};
use crate::CodeGenerator;

use convert_case::{Case, Casing};
use indexmap::IndexMap;
use itertools::Itertools;
use openapiv3::{
  ParameterData, ParameterSchemaOrContent, ReferenceOr, Schema, SchemaKind, StringFormat,
  StringType, Type, VariantOrUnknownOrEmpty,
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use unzip_n::unzip_n;

use std::collections::HashMap;

pub mod body;
pub mod operation;

unzip_n!(6);

/// Generated operations for a single API module.
struct ApiModuleOperations {
  /// Match cases for the API dispatcher from `operation_id` to the corresponding handler wrapper.
  api_dispatcher_cases: TokenStream,

  /// Handler functions the user must implement.
  handler_impls: Vec<String>,

  /// Prototypes for the handler functions the user must implement.
  handler_prototypes: TokenStream,

  /// Definitions for wrapper functions that parse parameters and implement logging, authentication,
  /// etc. for each operation.
  ///
  /// These functions call the corresponding handler implemented by the user.
  handler_wrappers: TokenStream,

  /// Definitions for operation response type enums.
  response_type_enums: TokenStream,

  response_type_idents: Vec<Ident>,
}

impl FromIterator<ApiOperation> for ApiModuleOperations {
  fn from_iter<T: IntoIterator<Item = ApiOperation>>(iter: T) -> Self {
    let (
      api_dispatcher_cases,
      handler_impls,
      handler_prototypes,
      handler_wrappers,
      response_type_enums,
      response_type_idents,
    ) = iter
      .into_iter()
      .map(
        |ApiOperation {
           api_dispatcher_case,
           handler_impl,
           handler_prototype,
           handler_wrapper,
           response_type_enum,
           response_type_ident,
         }| {
          (
            api_dispatcher_case,
            handler_impl.to_string(),
            handler_prototype,
            handler_wrapper,
            response_type_enum,
            response_type_ident,
          )
        },
      )
      .unzip_n();

    Self {
      api_dispatcher_cases,
      handler_impls,
      handler_prototypes,
      handler_wrappers,
      response_type_enums,
      response_type_idents,
    }
  }
}

fn is_array_param(parameter_data: &ParameterData) -> bool {
  matches!(
    parameter_data.format,
    ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
      schema_kind: SchemaKind::Type(Type::Array(_)),
      ..
    }))
  )
}

fn is_plain_string_schema(schema: &Schema) -> bool {
  matches!(
    schema,
    Schema {
      schema_kind: SchemaKind::Type(Type::String(StringType {
        enumeration,
        format: VariantOrUnknownOrEmpty::Item(StringFormat::Byte | StringFormat::Password)
          | VariantOrUnknownOrEmpty::Empty,
        ..
      })),
      ..
    } if enumeration.is_empty()
  )
}

impl CodeGenerator {
  pub(crate) fn gen_api_module(
    &self,
    mod_name: &str,
    operations: &[&PathOperation],
    openapi_inline: &serde_yaml::Mapping,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: &HashMap<Ident, TokenStream>,
  ) -> TokenStream {
    let ApiModuleOperations {
      api_dispatcher_cases,
      handler_impls,
      handler_prototypes,
      handler_wrappers,
      response_type_enums,
      response_type_idents,
    } = operations
      .iter()
      // Ensure deterministic codegen for readability and build caching.
      .sorted_by(|a, b| a.op.operation_id.cmp(&b.op.operation_id))
      .map(|operation| {
        self.gen_api_operation(
          mod_name,
          operation,
          openapi_inline,
          components_schemas,
          generated_models,
        )
      })
      .collect();

    self.gen_api_handler(mod_name, &handler_impls, &response_type_idents);

    let mod_name_ident = Ident::new(mod_name, Span::call_site());

    let crate_import = self.crate_use_name();
    quote! {
      pub mod #mod_name_ident {
        #![allow(clippy::too_many_arguments)]
        #![allow(unused_imports)]

        use #crate_import::{
          ApiGatewayProxyRequestContext, EventError, HeaderMap, HeaderName, http_response_to_apigw,
          HttpResponse, LambdaContext, LambdaEvent, Middleware, Response, StatusCode,
        };
        use #crate_import::async_trait::async_trait;
        use #crate_import::__private::{
          log, panic_string, serde_json, serde_path_to_error, urlencoding,
        };
        use #crate_import::__private::aws_lambda_events::apigw::{
          ApiGatewayProxyRequest,
          ApiGatewayProxyResponse,
        };
        use #crate_import::__private::aws_lambda_events::encodings::Body;
        use #crate_import::__private::backtrace::Backtrace;
        use #crate_import::__private::base64::{self, Engine as _};
        use #crate_import::__private::encoding::to_json;
        use #crate_import::__private::futures::FutureExt;
        use #crate_import::__private::headers::{ContentType, Header};
        use #crate_import::__private::mime::Mime;
        use #crate_import::error::format_error;

        #response_type_enums

        /// API Handler
        ///
        /// **This is an `#[async_trait]`.**
        #[async_trait]
        pub trait Api: Sized {
          /// User-defined authenticated identity type.
          ///
          /// This type is returned when
          /// [`Middleware::authenticate`](openapi_lambda::Middleware::authenticate) successfully
          /// authenticates a request then passed as an argument to the request handler method of
          /// this trait.
          ///
          /// Note that [`Middleware::authenticate`](openapi_lambda::Middleware::authenticate) is
          /// not invoked for unauthenticated endpoints (i.e., those with
          /// [`security: [{}]`](https://swagger.io/specification/#operation-object)),
          /// and no `AuthOk` value is passed as an argument to the corresponding request
          /// handler methods.
          type AuthOk: Send;

          /// User-defined error type (typically an `enum`).
          type HandlerError: Send;

          async fn respond_to_event_error(&self, err: EventError) -> HttpResponse {
            log::error!(
              "{}",
              format_error(&err, Some(&format!("EventError::{}", err.name())), err.backtrace()),
            );

            err.into()
          }

          async fn respond_to_handler_error(&self, err: Self::HandlerError) -> HttpResponse;

          #handler_prototypes

          async fn dispatch_request<M>(
            &self,
            event: LambdaEvent<ApiGatewayProxyRequest>,
            middleware: &M,
          ) -> ApiGatewayProxyResponse
          where
            M: Middleware<AuthOk = <Self as Api>::AuthOk> + Sync
          {
            match std::panic::AssertUnwindSafe(
              dispatch_request_impl(self, event.payload, event.context, middleware)
            )
            .catch_unwind()
            .await {
              Ok(response) => response,
              Err(panic) => {
                http_response_to_apigw(
                  self.respond_to_event_error(
                    EventError::Panic(
                      // If the panic value isn't a String or &str, don't catch it since we can't
                      // print it and it's unclear what we should do instead.
                      panic_string(panic).unwrap_or_else(|err| std::panic::resume_unwind(err)),
                      // Unfortunately, the panic doesn't give us a stack trace unless we set a
                      // panic hook, which might interfere with the user's own error handling.
                      // Instead, we just capture a backtrace indicating where we caught the
                      // panic, for now.
                      Backtrace::new(),
                    )
                  )
                  .await
                )
              }
            }
          }
        }

        #handler_wrappers

        async fn dispatch_request_impl<A, M>(
          api: &A,
          request: ApiGatewayProxyRequest,
          lambda_context: LambdaContext,
          middleware: &M,
        ) -> ApiGatewayProxyResponse
        where
          A: Api<AuthOk = <M as Middleware>::AuthOk> + Sync,
          M: Middleware + Sync,
        {
          log::trace!("Request: {request:#?}");
          log::trace!("Lambda context: {lambda_context:#?}");

          let operation_id = if let Some(ref operation_id) = request.request_context.operation_name {
            operation_id
          } else {
            return http_response_to_apigw(
              api
                .respond_to_event_error(EventError::UnexpectedOperationId(
                  "no operation_name provided in ApiGatewayProxyRequest".into(),
                  Backtrace::new(),
                ))
                .await
            );
          };

          let http_response = match operation_id.as_str() {
            #api_dispatcher_cases
            _ => {
              api
                .respond_to_event_error(
                  EventError::UnexpectedOperationId(operation_id.to_string(), Backtrace::new())
                )
                .await
            }
          };

          http_response_to_apigw(http_response)
        }
      }
    }
  }

  pub(crate) fn gen_api_handler(
    &self,
    mod_name: &str,
    handler_impls: &[String],
    response_types: &[Ident],
  ) {
    let crate_import = self.crate_use_name();
    let mod_name_pascal = format!("{}ApiHandler", mod_name.to_case(Case::Pascal));

    let api_mod_imports = response_types.iter().join(", ");

    let handler_impls_str = handler_impls.join("\n\n");

    let handler = format!(
      r#"#![allow(unused_imports)]

      use crate::{mod_name}::{{Api, {api_mod_imports}}};

      use {crate_import}::{{
        ApiGatewayProxyRequestContext, HeaderMap, HttpResponse, LambdaContext, StatusCode,
      }};
      use {crate_import}::async_trait::async_trait;
      use {crate_import}::__private::anyhow;
      use {crate_import}::__private::aws_lambda_events::encodings::Body;

      pub struct {mod_name_pascal} {{
        // Store any handler state (e.g., DB client) here.
        state: (),
      }}

      impl {mod_name_pascal} {{
        pub fn new(state: ()) -> Self {{
          Self {{ state }}
        }}
      }}

      #[async_trait]
      impl Api for {mod_name_pascal} {{
        // Define a type here to represent a successfully authenticated user.
        type AuthOk = ();

        // Define an error type to capture the errors produced by your API handler methods.
        type HandlerError = ();

        // Return an error response depending on the nature of the error (e.g., 400 Bad Request for
        // errors caused by a client sending an invalid request, or 500 Internal Server Error for
        // internal errors such as failing to connect to a database).
        async fn respond_to_handler_error(&self, _err: Self::HandlerError) -> HttpResponse {{
          todo!()
        }}

        {handler_impls_str}
      }}
      "#
    );

    let handler_path = self.out_dir.join(format!("{mod_name}_handler.rs"));
    log::info!("Writing `{mod_name}` handler to {}", handler_path.display());
    std::fs::write(&handler_path, handler.as_bytes()).unwrap_or_else(|err| {
      panic!(
        "failed to write {mod_name} handler to `{}`: {err}",
        handler_path.display()
      )
    });

    self.rustfmt(&handler_path);
  }
}
