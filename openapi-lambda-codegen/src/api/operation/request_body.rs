use crate::api::operation::parameter::RequestParameter;
use crate::CodeGenerator;

use indexmap::IndexMap;
use openapiv3::{ReferenceOr, RequestBody, Schema};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

use std::collections::HashMap;

use crate::api::body::BodySchema;

impl CodeGenerator {
  pub(crate) fn gen_request_body(
    &self,
    request_path: &str,
    request_body: &RequestBody,
    openapi_inline: &serde_yaml::Mapping,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: &HashMap<Ident, TokenStream>,
  ) -> Option<RequestParameter> {
    if request_body.content.is_empty() {
      return None;
    } else if request_body.content.len() > 1 {
      // Shouldn't be too difficult to support this.
      unimplemented!("multiple request body MIME types for `{request_path}`");
    }

    // This should never fail since we filter out empty request bodies above.
    let (mime_type, body_type) = request_body.content.get_index(0).expect("no mime types");

    // NB: this is critical for CSRF prevention since any Content-Type header other than forms
    // or plaintext requires a preflight, and we reject all CORS preflights at the API
    // gateway.
    let check_mime_type = quote! {
      if let Some(content_type_raw) = request.headers.get(ContentType::name().as_str()) {
        let content_type = match content_type_raw.to_str() {
          Ok(content_type) => content_type,
          Err(err) => return api
            .respond_to_event_error(
              EventError::InvalidHeaderUtf8(
                HeaderName::from_static(ContentType::name().as_str()),
                Box::new(err),
                Backtrace::new(),
              )
            ).await,
        };
        if !matches!(
          content_type.parse::<Mime>(),
          Ok(content_type) if content_type.essence_str() == #mime_type
        ) {
          return api.respond_to_event_error(
            EventError::UnexpectedContentType(content_type.to_owned(), Backtrace::new()),
          ).await;
        }
      } else {
        return api.respond_to_event_error(
          EventError::MissingRequestHeader(
            std::borrow::Cow::Borrowed(ContentType::name().as_str()),
            Backtrace::new(),
          )
        ).await;
      }
    };

    // Option<Vec<u8>>
    let decoded_body_opt = quote! {
      if request.is_base64_encoded {
        match request
          .body
          .map(|body| base64::engine::general_purpose::STANDARD.decode(body.as_bytes()))
          .transpose()
          // if this fails, it's an internal error since the base64 encoding is done by the
          // API Gateway.
          .map_err(|err| EventError::InvalidBodyBase64(Box::new(err), Backtrace::new()))
        {
          Ok(body) => body,
          Err(err) => return api.respond_to_event_error(err).await,
        }
      } else {
        request.body.map(String::into_bytes)
      }
    };

    let (signature, wrapper_parse_assignment) = if let Some(body_schema_or_ref) = &body_type.schema
    {
      let BodySchema {
        required_type,
        deserialize,
        ..
      } = self.gen_body_schema(
        Some(body_schema_or_ref),
        mime_type,
        "request_body",
        openapi_inline,
        components_schemas,
        generated_models,
      );

      if request_body.required {
        (
          quote! {
            request_body: #required_type,
          },
          quote! {
            let request_body_opt = match #decoded_body_opt #deserialize {
              Ok(body) => body,
              Err(err) => return api.respond_to_event_error(err).await,
            };
            let request_body = if let Some(request_body) = request_body_opt {
              request_body
            } else {
              return api
                .respond_to_event_error(EventError::MissingRequestBody(Backtrace::new()))
                .await;
            };
          },
        )
      } else {
        (
          quote! {
            request_body: Option<#required_type>,
          },
          quote! {
            let request_body = match #decoded_body_opt #deserialize {
              Ok(body) => body,
              Err(err) => return api.respond_to_event_error(err).await,
            };
          },
        )
      }
      // Body without schema (e.g., uploading binary data).
    } else if request_body.required {
      (
        quote! {
          request_body: Vec<u8>,
        },
        quote! {
          let request_body = if let Some(request_body) = #decoded_body_opt {
            request_body
          } else {
            return api.respond_to_event_error(
              EventError::MissingRequestBody(Backtrace::new())
            ).await;
          };
        },
      )
    } else {
      (
        quote! {
          request_body: Option<Vec<u8>>,
        },
        quote! {
          let request_body = #decoded_body_opt;
        },
      )
    };

    let log_param = quote! { log::trace!("Request body: {request_body:#?}"); };

    let param_desc = request_body
      .description
      .as_deref()
      .unwrap_or("Request body");

    let doc_attr = quote! {
      #[doc = concat!("* `request_body` - ", #param_desc)]
    };

    Some(RequestParameter {
      call_value: quote! { request_body, },
      doc_attr,
      log_param,
      signature,
      wrapper_parse_assignment: quote! {
        #check_mime_type;
        #wrapper_parse_assignment
      },
    })
  }
}
