use crate::model::GeneratedModels;
use crate::reference::resolve_local_reference;
use crate::CodeGenerator;

use indexmap::IndexMap;
use openapiv3::{
  ReferenceOr, Schema, SchemaKind, StringFormat, StringType, Type, VariantOrUnknownOrEmpty,
};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

use std::borrow::Cow;
use std::collections::HashMap;

/// Represents a request or response body type.
pub struct BodySchema {
  /// Type of the request or response body as passed into the request handler (for request bodies)
  /// or stored in the response body enum variant.
  pub required_type: TokenStream,

  /// Code appended to the decoded request body to deserialize from `Option<Vec<u8>>` to
  /// `Result<Option<#required_type>, EventError>`.
  pub deserialize: TokenStream,

  /// Code that takes a `body` variable of `required_type` and converts it to a
  /// `aws_lambda_events::encodings::Body`.
  pub serialize: TokenStream,
}

impl CodeGenerator {
  /// Generates both request and response body schemas to ensure symmetry.
  pub(crate) fn gen_body_schema(
    &self,
    schema_or_ref_opt: Option<&ReferenceOr<Schema>>,
    mime_type: &str,
    response_type: &str,
    openapi_inline: &serde_yaml::Mapping,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: &HashMap<Ident, TokenStream>,
  ) -> BodySchema {
    match (mime_type, schema_or_ref_opt) {
      ("application/json", None) => BodySchema {
        required_type: quote! { serde_json::Value },
        deserialize: quote! {
          .map(|decoded_body|
            serde_path_to_error::deserialize::<_, serde_json::Value>(
              &mut serde_json::Deserializer::from_slice(&decoded_body)
            )
          )
          .transpose()
          .map_err(|err| EventError::InvalidBodyJson(Box::new(err), Backtrace::new()))
        },
        serialize: quote! { Body::Text(body.to_string()) },
      },
      ("application/json", Some(schema_or_ref)) => {
        let schema = match schema_or_ref {
          ReferenceOr::Reference { reference } => {
            Cow::Owned(resolve_local_reference::<Schema>(reference, openapi_inline).target)
          }
          ReferenceOr::Item(schema) => Cow::Borrowed(schema),
        };

        // If the body schema is a string instead of an object or reference (e.g., for
        // webhook handlers that require the raw request body for HMAC verification), don't
        // deserialize it. We also don't use any newtypes here, even if the user defined a named
        // schema for this type.
        if let SchemaKind::Type(Type::String(StringType {
          enumeration,
          format,
          ..
        })) = &schema.as_ref().schema_kind
        {
          if !enumeration.is_empty() {
            panic!("unexpected inline enum JSON request or response body: {schema:#?}");
          }
          match format {
            // We assume that a binary type for a JSON request body wants the raw JSON as a byte
            // string, since there's no well-defined JSON representation for binary data (e.g.,
            // it could be an escaped string, a base64-encoded string, an array of byte integers,
            // etc.).
            VariantOrUnknownOrEmpty::Item(StringFormat::Binary) => BodySchema {
              required_type: quote! { Vec<u8> },
              deserialize: quote! { .map(Ok).transpose() },
              serialize: quote! { Body::Binary(body) },
            },
            // We assume that a string type for a JSON request body wants the raw JSON as a
            // string, rather than expecting a JSON payload containing a quoted and escaped
            // string. We just ignore any formats like date-time here, which wouldn't make much
            // sense for an application/json MIME type. It's unclear what to do with newtypes
            // (`VariantOrUnknownOrEmpty::Unknown`) here (i.e., should we use the FromStr or
            // Deserialize trait for parsing?), so we just pass the raw string and let the user do
            // any necessary deserialization.
            VariantOrUnknownOrEmpty::Empty
            | VariantOrUnknownOrEmpty::Item(_)
            | VariantOrUnknownOrEmpty::Unknown(_) => BodySchema {
              required_type: quote! { String },
              deserialize: quote! {
                .map(String::from_utf8)
                .transpose()
                .map_err(|err| EventError::InvalidBodyUtf8(Box::new(err), Backtrace::new()))
              },
              serialize: quote! { Body::Text(body) },
            },
          }
        } else {
          let (required_type, _) = self.inline_ref_or_schema(
            schema_or_ref,
            components_schemas,
            GeneratedModels::Done(generated_models),
          );
          let deserialize = quote! {
            .map(|decoded_body|
              serde_path_to_error::deserialize::<_, #required_type>(
                &mut serde_json::Deserializer::from_slice(&decoded_body)
              )
            )
            .transpose()
            .map_err(|err| EventError::InvalidBodyJson(Box::new(err), Backtrace::new()))
          };
          let serialize = quote! {
            Body::Text(
              to_json(&body)
                .map_err(|err| {
                  EventError::ToJsonResponse {
                    type_name: std::borrow::Cow::Borrowed(#response_type),
                    source: Box::new(err),
                    backtrace: Backtrace::new()
                  }
                })?
            )
          };

          BodySchema {
            required_type,
            deserialize,
            serialize,
          }
        }
      }
      // If there's a schema defined for these flat types, we just ignore it since there's
      // no well-defined way to (de)serialize to them. It becomes the user's responsibility to
      // do the (de)serialization.
      ("application/octet-stream", _) => BodySchema {
        required_type: quote! { Vec<u8> },
        deserialize: quote! { .map(Ok).transpose() },
        serialize: quote! { Body::Binary(body) },
      },
      // Treat all text types as UTF-8 strings.
      (mime, _) if mime.starts_with("text/") => BodySchema {
        required_type: quote! { String },
        deserialize: quote! {
          .map(String::from_utf8)
          .transpose()
          .map_err(|err| EventError::InvalidBodyUtf8(Box::new(err), Backtrace::new()))
        },
        serialize: quote! { Body::Text(body) },
      },
      // Any types we don't explicitly support we just leave as raw byte strings.
      _ => BodySchema {
        required_type: quote! { Vec<u8> },
        deserialize: quote! { .map(Ok).transpose() },
        serialize: quote! { Body::Binary(body) },
      },
    }
  }
}
