use crate::api::{is_array_param, is_plain_string_schema};
use crate::model::GeneratedModels;
use crate::CodeGenerator;

use convert_case::{Case, Casing};
use indexmap::IndexMap;
use openapiv3::{
  ArrayType, Parameter, ParameterSchemaOrContent, ReferenceOr, Schema, SchemaKind, Type,
};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

use std::collections::HashMap;

/// A generated request query/header/path parameter for an API operation.
pub struct RequestParameter {
  /// Value passed from handler wrapper to user handler implementation.
  pub call_value: TokenStream,

  /// #[doc = "..."] describing the parameter.
  pub doc_attr: TokenStream,

  /// Parameter name as snake_case.
  pub log_param: TokenStream,

  /// `#param_name_ident: #param_type` for handler signature.
  pub signature: TokenStream,

  /// Local variable `let`-assignment for parsing the parameter in the handler wrapper.
  pub wrapper_parse_assignment: TokenStream,
}

impl CodeGenerator {
  pub(crate) fn gen_request_parameter(
    &self,
    param: &Parameter,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: &HashMap<Ident, TokenStream>,
  ) -> RequestParameter {
    let param_data = match param {
      Parameter::Query { parameter_data, .. } => parameter_data,
      Parameter::Header { parameter_data, .. } => parameter_data,
      Parameter::Path { parameter_data, .. } => parameter_data,
      Parameter::Cookie { parameter_data, .. } => parameter_data,
    };

    let param_name = param_data.name.as_str();
    let param_name_ident = self.identifier(&param_name.to_case(Case::Snake));
    let (required_type, parse_type) = match &param_data.format {
      ParameterSchemaOrContent::Schema(ref_or_schema) => {
        let (required_type, _) = self.inline_ref_or_schema(
          ref_or_schema,
          components_schemas,
          GeneratedModels::Done(generated_models),
        );

        // If it's anything other than a string or array of strings, we need to parse it.
        let parse_type = match ref_or_schema {
          // References to named types must be parsed.
          ReferenceOr::Reference { .. } => Some(required_type.clone()),
          ReferenceOr::Item(schema) if is_plain_string_schema(schema) => None,
          ReferenceOr::Item(Schema {
            schema_kind:
              SchemaKind::Type(Type::Array(ArrayType {
                items: Some(item_ref_or_schema),
                ..
              })),
            ..
          }) => match item_ref_or_schema {
            ReferenceOr::Reference { .. } => Some(required_type.clone()),
            ReferenceOr::Item(item_schema) if !is_plain_string_schema(item_schema) => Some(
              self
                .inline_ref_or_schema(
                  item_ref_or_schema,
                  components_schemas,
                  GeneratedModels::Done(generated_models),
                )
                .0,
            ),
            ReferenceOr::Item(_) => None,
          },
          ReferenceOr::Item(_) => Some(required_type.clone()),
        };

        (required_type, parse_type)
      }
      ParameterSchemaOrContent::Content(_) => unimplemented!("content parameter `{param_name}`"),
    };

    let param_type = if param_data.required {
      required_type
    } else {
      quote! { Option<#required_type> }
    };

    let signature = quote! {
      #param_name_ident: #param_type,
    };

    let parse = if let Some(ref parse_type) = parse_type {
      let parse_error_variant = match param {
        Parameter::Query { .. } => quote! { InvalidRequestQueryParam },
        Parameter::Header { .. } => unimplemented!("header newtypes"),
        Parameter::Path { .. } => quote! { InvalidRequestPathParam },
        Parameter::Cookie { .. } => unimplemented!("cookie newtypes"),
      };
      quote! {
        |p| {
          // We use FromStr instead of Deserialize for parameters since parameters
          // are always strings (vs. body parameters that can be structured data), and it
          // simplifies error handling since there are fewer error cases than using
          // something like serde_plain(), which could result in runtime errors from
          // trying to deserialize to a complex type (vs. FromStr which imposes no
          // requirements on the types for which it's implemented).
          p.parse::<#parse_type>()
            .map_err(|err| {
              EventError::#parse_error_variant {
                param_name: std::borrow::Cow::Borrowed(#param_name),
                source: Some(err.into()),
                backtrace: Backtrace::new(),
              }
            })
        }
      }
    } else {
      match param {
        Parameter::Header { .. } => quote! { Ok },
        Parameter::Path { .. } | Parameter::Query { .. } => quote! { |p| Ok(p.to_string()) },
        Parameter::Cookie { .. } => unimplemented!("cookie parameters"),
      }
    };

    let param_parse = match param {
      Parameter::Header { .. } => {
        // Option<Result<String, _>>
        quote! {
          request
            .headers
            .get(#param_name)
            .map(|header_value| {
              header_value.to_str()
                .map(String::from)
                .map_err(|err| {
                  EventError::InvalidHeaderUtf8(
                    HeaderName::from_static(#param_name),
                    Box::new(err),
                    Backtrace::new(),
                  )
                })
                .and_then(#parse)
            })
        }
      }
      Parameter::Path { .. } => {
        // Option<Result<String, _>>
        //
        // The API Gateway REST API Lambda proxy integration doesn't automatically URL-decode path
        // params, so we need to. See https://github.com/aws/aws-sam-cli/issues/771.
        quote! {
          if let Some(param_value) = request.path_parameters.get(#param_name) {
            match urlencoding::decode(param_value) {
              Ok(decoded_param_value) => {
                Some(decoded_param_value)
                  .map(#parse)
              },
              Err(err) => return api.respond_to_event_error(
                EventError::InvalidRequestPathParam {
                  param_name: std::borrow::Cow::Borrowed(#param_name),
                  source: Some(err.into()),
                  backtrace: Backtrace::new(),
                }
              ).await,
            }
          } else {
            None
          }
        }
      }
      Parameter::Query { parameter_data, .. } => {
        // Unlike path parameters (see above), we don't need to URL-deoode query params.
        // "In general, REST APIs decode URL-encoded request parameters before passing them to backend
        // integrations." See:
        // https://docs.aws.amazon.com/apigateway/latest/developerguide/api-gateway-known-issues.html.
        if is_array_param(parameter_data) {
          // Option<Result<Vec<String>, _>>
          quote! {
            request
              .multi_value_query_string_parameters
              .all(#param_name)
              .map(|param_values| {
                param_values
                  .iter()
                  .copied()
                  .map(#parse)
                  .collect::<Result<Vec<_>, _>>()
              })
          }
        } else {
          // Option<Result<String, _>>
          quote! {
            request
              .query_string_parameters
              .first(#param_name)
              .map(#parse)
          }
        }
      }
      Parameter::Cookie { .. } => unimplemented!("cookie parameters"),
    };

    let wrapper_parse_assignment = if param_data.required {
      quote! {
        #[allow(clippy::bind_instead_of_map)]
        let #param_name_ident = match #param_parse {
          Some(Ok(param_value)) => param_value,
          Some(Err(err)) => return api.respond_to_event_error(err).await,
          None => return api.respond_to_event_error(
            EventError::MissingRequestParam(std::borrow::Cow::Borrowed(#param_name), Backtrace::new())
          ).await,
        };
      }
    } else {
      quote! {
        #[allow(clippy::bind_instead_of_map)]
        let #param_name_ident = match #param_parse.transpose() {
          Ok(param_value) => param_value,
          Err(err) => return api.respond_to_event_error(err).await,
        };
      }
    };

    let log_param = quote! {
      log::trace!(concat!("Request parameter `", #param_name, "`: {:#?}"), #param_name_ident);
    };

    let param_desc = param_data.description.as_deref().unwrap_or("");

    let doc_attr = quote! {
      #[doc = concat!("* `", stringify!(#param_name_ident), "` - ", #param_desc)]
    };

    RequestParameter {
      call_value: quote! { #param_name_ident, },
      doc_attr,
      log_param,
      signature,
      wrapper_parse_assignment,
    }
  }
}
