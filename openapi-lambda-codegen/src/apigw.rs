use crate::inline::InlineApi;
use crate::{ApiLambda, CodeGenerator};

use log::warn;
use openapiv3::{
  AdditionalProperties, Callback, Components, Header, MediaType, ObjectType, Operation, Parameter,
  ParameterSchemaOrContent, PathItem, ReferenceOr, RequestBody, Response, Responses, Schema,
  SchemaKind, Type,
};
use serde_json::json;

use std::collections::{HashMap, HashSet};

const API_GATEWAY_INTEGRATION_EXTENTION: &str = "x-amazon-apigateway-integration";
const OPENAPI_GW_FILENAME: &str = "openapi-apigw.yaml";

impl CodeGenerator {
  pub(crate) fn gen_openapi_apigw(
    &self,
    openapi: InlineApi,
    operation_id_to_api_lambda: &HashMap<&str, &ApiLambda>,
  ) {
    let openapi_for_apigw = transform_openapi(openapi, operation_id_to_api_lambda);

    let mut yaml_bytes = Vec::new();
    serde_path_to_error::serialize(
      &*openapi_for_apigw,
      &mut serde_yaml::Serializer::new(&mut yaml_bytes),
    )
    .expect("failed to serialize processed OpenAPI spec");

    let openapi_apigw_path = self.out_dir.join(OPENAPI_GW_FILENAME);
    std::fs::write(&openapi_apigw_path, &yaml_bytes).unwrap_or_else(|err| {
      panic!(
        "failed to write OpenAPI spec to `{}`: {err}",
        openapi_apigw_path.display()
      )
    });
  }
}

/// Process an OpenAPI definition and perform the following transformations:
///  * Insert `x-amazon-apigateway-integration` extensions into each path item whose
///    `operation_id` is mapped to an [`ApiLambda`].
///  * Remove operations whose `operation_id` is not mapped to an [`ApiLambda`], and path items
///    that are empty after removing unmapped operations.
///  * Removes `discriminator` values and makes sure the corresponding fields are required. See
///    <https://docs.aws.amazon.com/apigateway/latest/developerguide/api-gateway-known-issues.html#api-gateway-known-issues-rest-apis>.
///    The serde deserializer will still follow the original schema and reject any invalid request
///    schemas. Response schemas serialized by serde will likewise follow the original schema.
fn transform_openapi(
  mut openapi: InlineApi,
  operation_id_to_api_lambda: &HashMap<&str, &ApiLambda>,
) -> InlineApi {
  if let Some(components) = &mut openapi.components {
    transform_components(components);
  }

  let mut paths_to_remove = Vec::new();
  let mut visited_operation_ids = HashSet::new();
  for (path, path_item) in &mut openapi.paths.paths {
    // Don't follow and update references here since we should reach the reference target directly
    // (e.g., when visiting components above).
    let ReferenceOr::Item(path_item) = path_item else {
      continue;
    };
    transform_path_item(path_item);

    for (method, operation) in [
      ("GET", &mut path_item.get),
      ("PUT", &mut path_item.put),
      ("POST", &mut path_item.post),
      ("DELETE", &mut path_item.delete),
      ("OPTIONS", &mut path_item.options),
      ("HEAD", &mut path_item.head),
      ("PATCH", &mut path_item.patch),
      ("TRACE", &mut path_item.trace),
    ]
    .into_iter()
    {
      if let Some(op) = operation {
        if let Some(operation_id) = &op.operation_id {
          if !visited_operation_ids.insert(operation_id.to_owned()) {
            panic!("duplicate operation_id `{operation_id}`");
          }
          if let Some(api_lambda) = operation_id_to_api_lambda.get(operation_id.as_str()) {
            op.extensions.insert(
              API_GATEWAY_INTEGRATION_EXTENTION.to_string(),
              json!({
                "httpMethod": "POST",
                "type": "aws_proxy",
                "uri": api_lambda.lambda_arn.apigw_invocation_arn()
              }),
            );
          } else {
            warn!("removing endpoint not mapped to any API: {method} {path} ({operation_id})");
            *operation = None;
          }
        } else {
          warn!("removing endpoint without operation_id: {method} {path}");
          *operation = None;
        }
      }
    }

    // If we remove all of the methods, we should remove the path altogether.
    if path_item.iter().next().is_none() {
      paths_to_remove.push(path.to_owned());
    }
  }

  for path in &paths_to_remove {
    openapi.paths.paths.remove(path);
  }

  openapi
}

fn transform_components(components: &mut Components) {
  for (_, response) in &mut components.responses {
    let ReferenceOr::Item(response) = response else {
      continue;
    };
    transform_response(response);
  }

  for (_, parameter) in &mut components.parameters {
    let ReferenceOr::Item(parameter) = parameter else {
      continue;
    };
    transform_parameter(parameter);
  }

  for (_, request_body) in &mut components.request_bodies {
    let ReferenceOr::Item(request_body) = request_body else {
      continue;
    };
    transform_request_body(request_body);
  }

  for (_, header) in &mut components.headers {
    let ReferenceOr::Item(header) = header else {
      continue;
    };
    transform_header(header);
  }

  for (_, schema) in &mut components.schemas {
    let ReferenceOr::Item(schema) = schema else {
      continue;
    };
    transform_schema(schema);
  }

  for (_, callback) in &mut components.callbacks {
    let ReferenceOr::Item(callback) = callback else {
      continue;
    };
    transform_callback(callback);
  }

  // We just leave `components.extensions` alone for now.
}

fn transform_callback(callback: &mut Callback) {
  for (_, path_item) in callback {
    transform_path_item(path_item)
  }
}

fn transform_header(header: &mut Header) {
  transform_parameter_schema_or_content(&mut header.format);
}

fn transform_media_type(media_type: &mut MediaType) {
  if let Some(ReferenceOr::Item(schema)) = &mut media_type.schema {
    transform_schema(schema);
  }
}

fn transform_operation(operation: &mut Operation) {
  for parameter in &mut operation.parameters {
    let ReferenceOr::Item(parameter) = parameter else {
      continue;
    };
    transform_parameter(parameter)
  }

  if let Some(ReferenceOr::Item(request_body)) = &mut operation.request_body {
    transform_request_body(request_body);
  }

  transform_responses(&mut operation.responses);
}

fn transform_parameter(parameter: &mut Parameter) {
  let parameter_data = match parameter {
    Parameter::Query { parameter_data, .. }
    | Parameter::Header { parameter_data, .. }
    | Parameter::Path { parameter_data, .. }
    | Parameter::Cookie { parameter_data, .. } => parameter_data,
  };

  transform_parameter_schema_or_content(&mut parameter_data.format);
}

fn transform_parameter_schema_or_content(
  parameter_schema_or_content: &mut ParameterSchemaOrContent,
) {
  match parameter_schema_or_content {
    ParameterSchemaOrContent::Schema(ReferenceOr::Item(schema)) => transform_schema(schema),
    ParameterSchemaOrContent::Schema(ReferenceOr::Reference { .. }) => {}
    ParameterSchemaOrContent::Content(content) => {
      for (_, media_type) in content {
        transform_media_type(media_type)
      }
    }
  }
}

fn transform_path_item(path_item: &mut PathItem) {
  path_item
    .get
    .iter_mut()
    .chain(path_item.put.iter_mut())
    .chain(path_item.post.iter_mut())
    .chain(path_item.delete.iter_mut())
    .chain(path_item.options.iter_mut())
    .chain(path_item.head.iter_mut())
    .chain(path_item.patch.iter_mut())
    .chain(path_item.trace.iter_mut())
    .for_each(transform_operation);

  for parameter in &mut path_item.parameters {
    let ReferenceOr::Item(parameter) = parameter else {
      continue;
    };
    transform_parameter(parameter);
  }
}

fn transform_request_body(request_body: &mut RequestBody) {
  for (_, media_type) in &mut request_body.content {
    transform_media_type(media_type);
  }
}

fn transform_response(response: &mut Response) {
  for (_, header) in &mut response.headers {
    let ReferenceOr::Item(header) = header else {
      continue;
    };
    transform_header(header);
  }

  for (_, media_type) in &mut response.content {
    transform_media_type(media_type)
  }
}

fn transform_responses(responses: &mut Responses) {
  if let Some(ReferenceOr::Item(default)) = &mut responses.default {
    transform_response(default);
  }

  for (_, response) in &mut responses.responses {
    let ReferenceOr::Item(response) = response else {
      continue;
    };

    transform_response(response);
  }
}

fn transform_schema(schema: &mut Schema) {
  match &mut schema.schema_kind {
    SchemaKind::Type(schema_type) => {
      match schema_type {
        Type::Object(object) => {
          for (_, property) in &mut object.properties {
            let ReferenceOr::Item(property) = property else {
              continue;
            };
            transform_schema(property);
          }

          if let Some(AdditionalProperties::Schema(additional_properties)) =
            &mut object.additional_properties
          {
            if let ReferenceOr::Item(additional_properties) = additional_properties.as_mut() {
              transform_schema(additional_properties);
            }
          }
        }
        Type::Array(array) => {
          if let Some(ReferenceOr::Item(items)) = &mut array.items {
            transform_schema(items.as_mut());
          }
        }
        Type::String(_) | Type::Number(_) | Type::Integer(_) | Type::Boolean { .. } => {}
      }

      if let Some(ref discriminator) = schema.schema_data.discriminator {
        if let Type::Object(ObjectType {
          properties,
          required,
          ..
        }) = schema_type
        {
          if !properties.contains_key(&discriminator.property_name) {
            panic!(
              "discriminator property `{}` does not exist in object type {schema_type:#?}",
              discriminator.property_name
            )
          }

          // Make the discriminator field required (since it's the serde tag)
          if !required.contains(&discriminator.property_name) {
            required.push(discriminator.property_name.clone());
          }
        } else {
          panic!("discriminators are only allowed on object types for schema {schema:#?}");
        }
      }
    }
    SchemaKind::OneOf { one_of: inner }
    | SchemaKind::AllOf { all_of: inner }
    | SchemaKind::AnyOf { any_of: inner } => inner.iter_mut().for_each(|inner_schema_or_ref| {
      if let ReferenceOr::Item(inner_schema) = inner_schema_or_ref {
        transform_schema(inner_schema);
      }
    }),
    SchemaKind::Not { not } => {
      if let ReferenceOr::Item(not) = not.as_mut() {
        transform_schema(not)
      }
    }
    SchemaKind::Any(any) => {
      for (_, property) in &mut any.properties {
        let ReferenceOr::Item(property) = property else {
          continue;
        };
        transform_schema(property);
      }

      if let Some(AdditionalProperties::Schema(additional_properties)) =
        &mut any.additional_properties
      {
        if let ReferenceOr::Item(additional_properties) = additional_properties.as_mut() {
          transform_schema(additional_properties);
        }
      }

      if let Some(ReferenceOr::Item(items)) = &mut any.items {
        transform_schema(items.as_mut());
      }

      any
        .one_of
        .iter_mut()
        .chain(any.all_of.iter_mut())
        .chain(any.any_of.iter_mut())
        .for_each(|inner_schema_or_ref| {
          if let ReferenceOr::Item(inner_schema) = inner_schema_or_ref {
            transform_schema(inner_schema);
          }
        });

      if let Some(not) = &mut any.not {
        if let ReferenceOr::Item(not) = not.as_mut() {
          transform_schema(not);
        }
      }
    }
  }

  schema.schema_data.discriminator = None;
}
