use crate::inline::InlineApi;

use convert_case::{Case, Casing};
use indexmap::IndexMap;
use log::warn;
use mime::Mime;
use openapiv3::{
  AdditionalProperties, AnySchema, BooleanType, Components, Header, IntegerType, MediaType,
  NumberType, ObjectType, OpenAPI, Operation, Parameter, ParameterSchemaOrContent, PathItem,
  ReferenceOr, RequestBody, Response, Responses, Schema, SchemaKind, StringType, Type,
};

use std::borrow::BorrowMut;

pub(in crate::model) fn visit_openapi(openapi: &mut InlineApi) {
  let OpenAPI {
    components: components_opt,
    paths,
    ..
  } = &mut **openapi;

  let components = if let Some(components) = components_opt {
    visit_components(components);
    components
  } else {
    components_opt.insert(Components::default())
  };

  for (_path, path_item) in &mut paths.paths {
    // Don't follow and update references here since we should reach the reference target directly
    // (e.g., when visiting components above).
    let ReferenceOr::Item(path_item) = path_item else {
      continue;
    };
    visit_path_item(path_item, &mut components.schemas)
  }
}

fn visit_components(components: &mut Components) {
  for (response_name, response) in &mut components.responses {
    let ReferenceOr::Item(response) = response else {
      continue;
    };
    visit_response(
      response,
      &format!("{}Response", response_name.to_case(Case::Pascal)),
      &mut components.schemas,
    );
  }

  for (_, parameter) in &mut components.parameters {
    let ReferenceOr::Item(parameter) = parameter else {
      continue;
    };
    // We just use the parameter `name` field to name parameters. Otherwise, we would end up with
    // redundant names like `ColorParamColorParam`.
    visit_parameter(parameter, "", &mut components.schemas);
  }

  for (request_body_name, request_body) in &mut components.request_bodies {
    let ReferenceOr::Item(request_body) = request_body else {
      continue;
    };
    visit_request_body(
      request_body,
      &request_body_name.to_case(Case::Pascal),
      &mut components.schemas,
    );
  }

  for (header_name, header) in &mut components.headers {
    let ReferenceOr::Item(header) = header else {
      continue;
    };
    visit_header(
      header,
      &format!("{}Header", header_name.to_case(Case::Pascal)),
      &mut components.schemas,
    );
  }

  // We can't borrow components.schemas mutably twice, so we create a temporary copy for inserting
  // new named schemas, and then we merge those in below.
  let mut named_schemas = components.schemas.clone();

  for (schema_name, schema) in &mut components.schemas {
    let ReferenceOr::Item(schema) = schema else {
      continue;
    };
    visit_schema(
      schema,
      &schema_name.to_case(Case::Pascal),
      &mut named_schemas,
    );
  }

  for (name, schema) in named_schemas {
    // The only modification to `named_schemas` should be newly inserted named schemas that were
    // previously unnamed. However, existing entries in components.schemas may have
    // been modified after we cloned it, so we don't overwrite those.
    if !components.schemas.contains_key(&name) {
      components.schemas.insert(name, schema);
    }
  }

  // We don't bother visiting callbacks here because we don't generate any code for them or their
  // models

  // We just leave `components.extensions` alone for now.
}

fn visit_header(
  header: &mut Header,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  visit_parameter_schema_or_content(
    &mut header.format,
    schema_naming_context,
    components_schemas,
  );
}

fn visit_media_type(
  media_type: &mut MediaType,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  if let Some(ref_or_schema) = &mut media_type.schema {
    visit_unnamed_schema(
      ref_or_schema,
      schema_naming_context,
      components_schemas,
      std::convert::identity,
    );
  }
}

fn visit_operation(
  operation: &mut Operation,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  // We require an operation ID for any operation handled by an API Lambda, so just ignore any
  // operations without one. We'll error out later if the user mapped it to an API Lambda.
  let Some(operation_id) = &operation.operation_id else {
    return;
  };
  let schema_naming_context = operation_id.to_case(Case::Pascal);

  for parameter in &mut operation.parameters {
    let ReferenceOr::Item(parameter) = parameter else {
      continue;
    };

    // This function adds a "Param" suffix to the naming context, so we just pass the operation ID
    // here.
    visit_parameter(parameter, &schema_naming_context, components_schemas)
  }

  if let Some(ReferenceOr::Item(request_body)) = &mut operation.request_body {
    // This function adds a "RequestBody" suffix to the naming context, so we just pass the
    // operation ID here.
    visit_request_body(request_body, &schema_naming_context, components_schemas);
  }

  // This function adds a "Response" suffix to the naming context, so we just pass the operation
  // ID here.
  visit_responses(
    &mut operation.responses,
    &schema_naming_context,
    components_schemas,
  );
}

fn visit_parameter(
  parameter: &mut Parameter,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  let parameter_data = match parameter {
    Parameter::Query { parameter_data, .. }
    | Parameter::Header { parameter_data, .. }
    | Parameter::Path { parameter_data, .. }
    | Parameter::Cookie { parameter_data, .. } => parameter_data,
  };

  visit_parameter_schema_or_content(
    &mut parameter_data.format,
    &format!(
      "{schema_naming_context}{}Param",
      parameter_data.name.to_case(Case::Pascal)
    ),
    components_schemas,
  );
}

fn visit_parameter_schema_or_content(
  parameter_schema_or_content: &mut ParameterSchemaOrContent,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  match parameter_schema_or_content {
    ParameterSchemaOrContent::Schema(ref_or_schema) => {
      visit_unnamed_schema(
        ref_or_schema,
        schema_naming_context,
        components_schemas,
        std::convert::identity,
      );
    }
    ParameterSchemaOrContent::Content(content) => {
      // The OpenAPI spec states that "The map MUST only contain one entry," so we don't bother
      // including the MIME type in the schema naming context.
      for (_, media_type) in content {
        visit_media_type(media_type, schema_naming_context, components_schemas)
      }
    }
  }
}

fn visit_path_item(
  path_item: &mut PathItem,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
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
    .for_each(|operation| visit_operation(operation, components_schemas));

  for parameter in &mut path_item.parameters {
    let ReferenceOr::Item(parameter) = parameter else {
      continue;
    };
    // For parameters that are shared between multiple HTTP methods with the same request path,
    // we just use an empty naming context, leading to param schemas like "UserIdParam" that
    // don't specify which endpoint they correspond to. If this leads to unsatisfactory naming,
    // users can create their own named schemas in components.schemas rather than relying on the
    // auto-naming behavior here.
    visit_parameter(parameter, "", components_schemas);
  }
}

fn visit_request_body(
  request_body: &mut RequestBody,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  for (media_type_or_range, media_type) in &mut request_body.content {
    visit_media_type(
      media_type,
      &format!(
        "{schema_naming_context}{}RequestBody",
        media_type_or_range_name_pascal_case(media_type_or_range)
      ),
      components_schemas,
    );
  }
}

fn visit_responses(
  responses: &mut Responses,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  if let Some(ReferenceOr::Item(default)) = &mut responses.default {
    visit_response(
      default,
      &format!("{schema_naming_context}DefaultResponse"),
      components_schemas,
    );
  }

  for (status_code, response) in &mut responses.responses {
    let ReferenceOr::Item(response) = response else {
      continue;
    };

    visit_response(
      response,
      &format!(
        "{schema_naming_context}{}Response",
        status_code.to_string().to_case(Case::Pascal)
      ),
      components_schemas,
    );
  }
}

fn visit_response(
  response: &mut Response,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) {
  for (header_name, header) in &mut response.headers {
    let ReferenceOr::Item(header) = header else {
      continue;
    };
    visit_header(
      header,
      &format!(
        "{schema_naming_context}{}Header",
        header_name.to_case(Case::Pascal)
      ),
      components_schemas,
    );
  }

  for (media_type_or_range, media_type) in &mut response.content {
    visit_media_type(
      media_type,
      &format!(
        "{schema_naming_context}{}ResponseBody",
        media_type_or_range_name_pascal_case(media_type_or_range)
      ),
      components_schemas,
    )
  }
}

/// Returns true if the schema will result in a named Rust model being generated.
///
/// Simple schemas like non-enum strings or numbers don't need Rust types generated for them,
/// while complex schemas like objects and string enums do.
fn visit_schema(
  schema: &mut Schema,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
) -> bool {
  match &mut schema.schema_kind {
    SchemaKind::Type(schema_type) => match schema_type {
      Type::Object(ObjectType {
        ref mut properties,
        ref mut additional_properties,
        ..
      }) => {
        for (property_name, property) in properties.iter_mut() {
          visit_unnamed_schema(
            property,
            &format!(
              "{schema_naming_context}{}",
              property_name.to_case(Case::Pascal)
            ),
            components_schemas,
            |b| *b,
          );
        }

        if let Some(AdditionalProperties::Schema(property_type)) = additional_properties {
          visit_unnamed_schema(
            property_type,
            // The keys are always strings; this represents the mapping value for any additional
            // properties.
            &format!("{schema_naming_context}Value"),
            components_schemas,
            std::convert::identity,
          );
        }

        // Any object schema with named properties needs a named model (Rust struct). We also need
        // a named model for empty objects (i.e., those with neither named nor additional
        // properties) since Rust doesn't support anonymous empty object types.
        !properties.is_empty()
      }
      Type::Array(array) => {
        if let Some(items) = &mut array.items {
          visit_unnamed_schema(
            items,
            &format!("{schema_naming_context}Item"),
            components_schemas,
            |b| *b,
          );
        }

        // We never generate models for array schemas (but we might for its item type).
        false
      }
      Type::String(StringType { enumeration, .. }) => {
        // We generate Rust enums for string enum schemas.
        !enumeration.is_empty()
      }
      Type::Number(NumberType { enumeration, .. }) => {
        // We generate Rust enums for number enum schemas.
        !enumeration.is_empty()
      }
      Type::Integer(IntegerType { enumeration, .. }) => {
        // We generate Rust enums for integer enum schemas.
        !enumeration.is_empty()
      }
      Type::Boolean(BooleanType { enumeration }) => {
        // We generate Rust enums for boolean enum schemas.
        !enumeration.is_empty()
      }
    },
    kind @ SchemaKind::OneOf { .. } | kind @ SchemaKind::AnyOf { .. } => {
      let (naming_context_suffix, inner) = match kind {
        SchemaKind::OneOf { one_of: inner } => ("OneOf", inner),
        SchemaKind::AnyOf { any_of: inner } => ("AnyOf", inner),
        _ => unreachable!(),
      };

      let inner_schema_naming_context = format!("{schema_naming_context}{naming_context_suffix}");
      inner.iter_mut().for_each(|inner_schema_or_ref| {
        visit_unnamed_schema(
          inner_schema_or_ref,
          &inner_schema_naming_context,
          components_schemas,
          std::convert::identity,
        );
      });

      // Always generate Rust structs or enums for compound schemas.
      true
    }
    SchemaKind::AllOf { all_of } => {
      for inner in all_of {
        let ReferenceOr::Item(inner) = inner else {
          continue;
        };
        // Don't inline allOf components because we'll generate a model that combines all of the
        // constituent fields.
        visit_schema(inner, schema_naming_context, components_schemas);
      }

      // Always generate Rust structs or enums for compound schemas.
      true
    }
    SchemaKind::Not { .. } => {
      unimplemented!("`not` schema {schema:#?}");
    }
    SchemaKind::Any(any) => {
      if *any != AnySchema::default() {
        unimplemented!("`any` schema in context {schema_naming_context}: {any:#?}");
      }

      false
    }
  }
}

fn visit_unnamed_schema<F, T>(
  ref_or_schema: &mut ReferenceOr<T>,
  schema_naming_context: &str,
  components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
  unbox: F,
) where
  F: Fn(T) -> Schema,
  T: BorrowMut<Schema>,
{
  if let ReferenceOr::Item(unnamed_schema) = ref_or_schema {
    if visit_schema(
      unnamed_schema.borrow_mut(),
      schema_naming_context,
      components_schemas,
    ) {
      let schema_name = if components_schemas.contains_key(schema_naming_context) {
        // Append an incrementing number until we find an unused schema name.
        let mut i = 2;
        loop {
          let schema_name = format!("{schema_naming_context}{i}");
          if !components_schemas.contains_key(&schema_name) {
            break schema_name;
          }
          i += 1;
        }
      } else {
        schema_naming_context.to_string()
      };

      let ReferenceOr::Item(unnamed_schema) = std::mem::replace(
        ref_or_schema,
        ReferenceOr::Reference {
          reference: format!("#/components/schemas/{schema_name}"),
        },
      ) else {
        unreachable!();
      };
      components_schemas.insert(schema_name, ReferenceOr::Item(unbox(unnamed_schema)));
    }
  }
}

/// Generates a string suitable for usage within a schema name that describes the provided
/// `media-type` (content type) or
/// [`media-range`](https://datatracker.ietf.org/doc/html/rfc7231#section-5.3.2).
fn media_type_or_range_name_pascal_case(media_type_or_range: &str) -> &'static str {
  // We don't currently support media ranges, but the as-yet-unreleased 0.4 version of `mime` should
  // add support for parsing those.
  let mime_type = match media_type_or_range.parse::<Mime>() {
    Ok(mime) => mime,
    Err(err) => {
      warn!("invalid or unsupported MIME type `{media_type_or_range}`: {err}");
      return "";
    }
  };

  // NB: Adding new mime types or changing their identifier names below is a SemVer BREAKING CHANGE.
  match mime_type.essence_str() {
    "application/json" => "Json",
    "application/octet-stream" => "Binary",
    "application/xml" | "text/xml" => "Xml",
    "image/gif" => "Gif",
    "image/jpeg" => "Jpeg",
    "image/png" => "Png",
    "image/svg+xml" => "Svg",
    "image/webp" => "Webp",
    "text/csv" => "Csv",
    "text/html" => "Html",
    "text/plain" => "PlainText",
    _ => {
      warn!("ignoring unrecognized MIME type `{media_type_or_range}`");
      ""
    }
  }
}
