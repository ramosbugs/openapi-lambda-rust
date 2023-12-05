use crate::api::body::BodySchema;
use crate::api::operation::PathOperation;
use crate::reference::{resolve_local_reference, ResolvedReference};
use crate::{description_to_doc_attr, CodeGenerator};

use indexmap::IndexMap;
use openapiv3::{ReferenceOr, Schema, StatusCode};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

use std::borrow::Cow;
use std::collections::HashMap;

impl CodeGenerator {
  pub(crate) fn gen_operation_response_type_enum(
    &self,
    mod_name: &str,
    func_name_snake: &str,
    response_type_ident: &Ident,
    op: &PathOperation,
    openapi_inline: &serde_yaml::Mapping,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: &HashMap<Ident, TokenStream>,
  ) -> TokenStream {
    let OperationResponses {
      response_variants,
      response_cases,
    } = self.gen_responses(
      op,
      response_type_ident,
      openapi_inline,
      components_schemas,
      generated_models,
    );
    quote! {
      #[allow(clippy::large_enum_variant)]
      #[derive(Clone, Debug)]
      #[doc = concat!(
        "Response to [`Api::", #func_name_snake, "`](crate::", #mod_name, "::Api::",
        #func_name_snake, ").",
      )]
      pub enum #response_type_ident {
        #response_variants
      }
      impl #response_type_ident {
        pub(crate) fn into_http_response(
          self,
          headers: HeaderMap,
        ) -> Result<HttpResponse, EventError> {
          let (status_code, content_type, body) = match self {
            #response_cases
          };

          let response = Response::builder().status(status_code);

          let response_with_content_type = if let Some(content_type) = content_type {
            response.header(ContentType::name(), content_type.to_string())
          } else {
            response
          };

          let response_with_headers = headers
            .iter()
            .fold(response_with_content_type, |response, (header_name, header_value)| {
              response.header(header_name, header_value)
            });

          response_with_headers
            .body(body)
            .map_err(|err| EventError::HttpResponse(Box::new(err), Backtrace::new()))
        }
      }
    }
  }

  fn gen_responses(
    &self,
    op: &PathOperation,
    response_type_ident: &Ident,
    openapi_inline: &serde_yaml::Mapping,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: &HashMap<Ident, TokenStream>,
  ) -> OperationResponses {
    let (response_variants, response_cases) = op
      .op
      .responses
      .responses
      .iter()
      .map(|(status_code, response)| Some((Some(status_code), response)))
      .chain(std::iter::once(
        op.op
          .responses
          .default
          .as_ref()
          .map(|response| (None, response)),
      ))
      .flatten()
      .map(|(status_code_enum, ref_or_response)| {
        let (status_code, variant_name) = if let Some(status_code_enum) = status_code_enum {
          let StatusCodeTokens {
            status_code,
            variant_name,
          } = status_code_tokens(status_code_enum);
          (Some(status_code), variant_name)
        } else {
          (None, quote! { Default })
        };

        let response = match ref_or_response {
          ReferenceOr::Item(response) => Cow::Borrowed(response),
          ReferenceOr::Reference { reference } => {
            let ResolvedReference { target, .. } =
              resolve_local_reference::<openapiv3::Response>(reference, openapi_inline);
            Cow::Owned(target)
          }
        };

        let (response_variant, response_case) = match response.content.len() {
          0 => {
            if let Some(status) = status_code {
              (
                quote! {
                  #variant_name,
                },
                quote! {
                  #response_type_ident::#variant_name =>
                    (#status, Option::<&'static str>::None, Body::Empty),
                },
              )
            } else {
              (
                quote! {
                  #variant_name(StatusCode),
                },
                quote! {
                  #response_type_ident::#variant_name(status_code) =>
                    (status_code, Option::<&'static str>::None, Body::Empty),
                },
              )
            }
          }
          1 => {
            // This should never fail since we filter out empty request bodies above.
            let (mime_type, body_type) = response.content.get_index(0).expect("no mime types");

            let BodySchema {
              required_type: variant_body,
              serialize: serialized_body,
              ..
            } = self.gen_body_schema(
              body_type.schema.as_ref(),
              mime_type,
              &response_type_ident.to_string(),
              openapi_inline,
              components_schemas,
              generated_models,
            );

            if let Some(status) = status_code {
              (
                quote! {
                  #variant_name(#variant_body),
                },
                quote! {
                  #response_type_ident::#variant_name(body) =>
                    (#status, Some(#mime_type), #serialized_body),
                },
              )
            } else {
              (
                quote! {
                  #variant_name(StatusCode, #variant_body),
                },
                quote! {
                  #response_type_ident::#variant_name(status_code, body) =>
                    (status_code, Some(#mime_type), #serialized_body),
                },
              )
            }
          }
          _ => {
            // Shouldn't be too difficult to support this.
            unimplemented!("multiple response body MIME types for {}", op.request_path);
          }
        };

        let doc_attr = description_to_doc_attr(&response.description);

        (
          quote! {
            #doc_attr
            #response_variant
          },
          response_case,
        )
      })
      .unzip::<_, _, TokenStream, TokenStream>();

    OperationResponses {
      response_cases,
      response_variants,
    }
  }
}

struct StatusCodeTokens {
  status_code: TokenStream,
  variant_name: TokenStream,
}

// Ensure at compile time that we only reference valid StatusCode variants.
macro_rules! validated_status_code {
  ($konst: ident) => {{
    let _ = http::StatusCode::$konst;
    quote! { StatusCode::$konst }
  }};
}

fn status_code_tokens(status_code_enum: &StatusCode) -> StatusCodeTokens {
  let (status_code, variant_name) = match status_code_enum {
    StatusCode::Code(100) => (validated_status_code!(CONTINUE), quote! { Continue }),
    StatusCode::Code(101) => (
      validated_status_code!(SWITCHING_PROTOCOLS),
      quote! { SwitchingProtocols },
    ),
    StatusCode::Code(102) => (validated_status_code!(PROCESSING), quote! { Processing }),
    StatusCode::Code(200) => (validated_status_code!(OK), quote! { Ok }),
    StatusCode::Code(201) => (validated_status_code!(CREATED), quote! { Created }),
    StatusCode::Code(202) => (validated_status_code!(ACCEPTED), quote! { Accepted }),
    StatusCode::Code(203) => (
      validated_status_code!(NON_AUTHORITATIVE_INFORMATION),
      quote! { NonAuthoritativeInformation },
    ),
    StatusCode::Code(204) => (validated_status_code!(NO_CONTENT), quote! { NoContent }),
    StatusCode::Code(205) => (
      validated_status_code!(RESET_CONTENT),
      quote! { ResetContent },
    ),
    StatusCode::Code(206) => (
      validated_status_code!(PARTIAL_CONTENT),
      quote! { PartialContent },
    ),
    StatusCode::Code(207) => (validated_status_code!(MULTI_STATUS), quote! { MultiStatus }),
    StatusCode::Code(208) => (
      validated_status_code!(ALREADY_REPORTED),
      quote! { AlreadyReported },
    ),
    StatusCode::Code(226) => (validated_status_code!(IM_USED), quote! { ImUsed }),
    StatusCode::Code(300) => (
      validated_status_code!(MULTIPLE_CHOICES),
      quote! { MultipleChoices },
    ),
    StatusCode::Code(301) => (
      validated_status_code!(MOVED_PERMANENTLY),
      quote! { MovedPermanently },
    ),
    StatusCode::Code(302) => (validated_status_code!(FOUND), quote! { Found }),
    StatusCode::Code(303) => (validated_status_code!(SEE_OTHER), quote! { SeeOther }),
    StatusCode::Code(304) => (validated_status_code!(NOT_MODIFIED), quote! { NotModified }),
    StatusCode::Code(305) => (validated_status_code!(USE_PROXY), quote! { UseProxy }),
    StatusCode::Code(307) => (
      validated_status_code!(TEMPORARY_REDIRECT),
      quote! { TemporaryRedirect },
    ),
    StatusCode::Code(308) => (
      validated_status_code!(PERMANENT_REDIRECT),
      quote! { PermanentRedirect },
    ),
    StatusCode::Code(400) => (validated_status_code!(BAD_REQUEST), quote! { BadRequest }),
    StatusCode::Code(401) => (
      validated_status_code!(UNAUTHORIZED),
      // The naming in the standard is misleading, so we use a more correct variant name instead.
      quote! { Unauthenticated },
    ),
    StatusCode::Code(402) => (
      validated_status_code!(PAYMENT_REQUIRED),
      quote! { PaymentRequired },
    ),
    StatusCode::Code(403) => (validated_status_code!(FORBIDDEN), quote! { Forbidden }),
    StatusCode::Code(404) => (validated_status_code!(NOT_FOUND), quote! { NotFound }),
    StatusCode::Code(405) => (
      validated_status_code!(METHOD_NOT_ALLOWED),
      quote! { MethodNotAllowed },
    ),
    StatusCode::Code(406) => (
      validated_status_code!(NOT_ACCEPTABLE),
      quote! { NotAcceptable },
    ),
    StatusCode::Code(407) => (
      validated_status_code!(PROXY_AUTHENTICATION_REQUIRED),
      quote! { ProxyAuthenticationRequired },
    ),
    StatusCode::Code(408) => (
      validated_status_code!(REQUEST_TIMEOUT),
      quote! { RequestTimeout },
    ),
    StatusCode::Code(409) => (validated_status_code!(CONFLICT), quote! { Conflict }),
    StatusCode::Code(410) => (validated_status_code!(GONE), quote! { Gone}),
    StatusCode::Code(411) => (
      validated_status_code!(LENGTH_REQUIRED),
      quote! { LengthRequired },
    ),
    StatusCode::Code(412) => (
      validated_status_code!(PRECONDITION_FAILED),
      quote! { PreconditionFailed },
    ),
    StatusCode::Code(413) => (
      validated_status_code!(PAYLOAD_TOO_LARGE),
      quote! { PayloadTooLarge },
    ),
    StatusCode::Code(414) => (validated_status_code!(URI_TOO_LONG), quote! { UriTooLong }),
    StatusCode::Code(415) => (
      validated_status_code!(UNSUPPORTED_MEDIA_TYPE),
      quote! { UnsupportedMediaType },
    ),
    StatusCode::Code(416) => (
      validated_status_code!(RANGE_NOT_SATISFIABLE),
      quote! { RangeNotSatisfiable },
    ),
    StatusCode::Code(417) => (
      validated_status_code!(EXPECTATION_FAILED),
      quote! { ExpectationFailed },
    ),
    StatusCode::Code(418) => (validated_status_code!(IM_A_TEAPOT), quote! { ImATeapot }),
    StatusCode::Code(421) => (
      validated_status_code!(MISDIRECTED_REQUEST),
      quote! { MisdirectedRequest },
    ),
    StatusCode::Code(422) => (
      validated_status_code!(UNPROCESSABLE_ENTITY),
      quote! { UnprocessableEntity },
    ),
    StatusCode::Code(423) => (validated_status_code!(LOCKED), quote! { Locked }),
    StatusCode::Code(424) => (
      validated_status_code!(FAILED_DEPENDENCY),
      quote! { FailedDependency },
    ),
    StatusCode::Code(426) => (
      validated_status_code!(UPGRADE_REQUIRED),
      quote! { UpgradeRequired },
    ),
    StatusCode::Code(428) => (
      validated_status_code!(PRECONDITION_REQUIRED),
      quote! { PreconditionRequired },
    ),
    StatusCode::Code(429) => (
      validated_status_code!(TOO_MANY_REQUESTS),
      quote! { TooManyRequests },
    ),
    StatusCode::Code(431) => (
      validated_status_code!(REQUEST_HEADER_FIELDS_TOO_LARGE),
      quote! { RequestHeaderFieldsTooLarge },
    ),
    StatusCode::Code(451) => (
      validated_status_code!(UNAVAILABLE_FOR_LEGAL_REASONS),
      quote! { UnavailableForLegalReasons },
    ),
    StatusCode::Code(500) => (
      validated_status_code!(INTERNAL_SERVER_ERROR),
      quote! { InternalServerError },
    ),
    StatusCode::Code(501) => (
      validated_status_code!(NOT_IMPLEMENTED),
      quote! { NotImplemented },
    ),
    StatusCode::Code(502) => (validated_status_code!(BAD_GATEWAY), quote! { BadGateway }),
    StatusCode::Code(503) => (
      validated_status_code!(SERVICE_UNAVAILABLE),
      quote! { ServiceUnavailable },
    ),
    StatusCode::Code(504) => (
      validated_status_code!(GATEWAY_TIMEOUT),
      quote! { GatewayTimeout },
    ),
    StatusCode::Code(505) => (
      validated_status_code!(HTTP_VERSION_NOT_SUPPORTED),
      quote! { HttpVersionNotSupported },
    ),
    StatusCode::Code(506) => (
      validated_status_code!(VARIANT_ALSO_NEGOTIATES),
      quote! { VariantAlsoNegotiates },
    ),
    StatusCode::Code(507) => (
      validated_status_code!(INSUFFICIENT_STORAGE),
      quote! { InsufficientStorage },
    ),
    StatusCode::Code(508) => (
      validated_status_code!(LOOP_DETECTED),
      quote! { LoopDetected },
    ),
    StatusCode::Code(510) => (validated_status_code!(NOT_EXTENDED), quote! { NotExtended }),
    StatusCode::Code(511) => (
      validated_status_code!(NETWORK_AUTHENTICATION_REQUIRED),
      quote! { NetworkAuthenticationRequired },
    ),
    StatusCode::Code(other) => {
      let variant_ident = Ident::new(&format!("HttpStatus{other}"), Span::call_site());
      // Make sure it's valid at codegen time.
      http::StatusCode::from_u16(*other)
        .unwrap_or_else(|err| panic!("invalid HTTP status code {other}: {err}"));
      (
        quote! {
          StatusCode::from_u16(#other)
            .expect(concat!(stringify!(#other), " should be a valid HTTP status code"))
        },
        quote! { #variant_ident },
      )
    }

    StatusCode::Range(_) => unimplemented!("response status code ranges"),
  };

  StatusCodeTokens {
    status_code,
    variant_name,
  }
}

struct OperationResponses {
  pub response_cases: TokenStream,
  pub response_variants: TokenStream,
}
