use crate::reference::{resolve_reference, ResolvedReference};
use crate::{CodeGenerator, DocCache};

use indexmap::IndexMap;
use openapiv3::{
  AdditionalProperties, Callback, Components, Header, MediaType, OpenAPI, Operation, Parameter,
  ParameterSchemaOrContent, PathItem, ReferenceOr, RequestBody, Response, Responses, Schema,
  SchemaKind, Type,
};
use serde::de::DeserializeOwned;

use std::borrow::BorrowMut;
use std::ops::{Deref, DerefMut};
use std::path::Path;

// An OpenAPI definition with only local references (i.e., within the same file).
#[derive(Debug)]
pub(crate) struct InlineApi(OpenAPI);

impl Deref for InlineApi {
  type Target = OpenAPI;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for InlineApi {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

impl CodeGenerator {
  /// Resolve and inline all `$ref` elements that point to objects contained in other files.
  ///
  /// The resulting OpenAPI definition is contained in a single file. Schemas are a special case:
  /// rather than inlining foreign schema references, we add them to `openapi.components.schemas`
  /// and replace the foreign reference with a local reference in order to preserve the schema name.
  /// If there is already a non-identical schema with the same name, we inline it instead.
  pub(crate) fn inline_openapi(
    &self,
    mut openapi: OpenAPI,
    mut cached_external_docs: DocCache,
  ) -> InlineApi {
    let components = if let Some(components) = &mut openapi.components {
      self.inline_components(components, &mut cached_external_docs);
      components
    } else {
      openapi.components.insert(Components::default())
    };

    for (_path, path_item) in &mut openapi.paths.paths {
      self.inline_reference_or_item(
        &self.openapi_path,
        path_item,
        &mut cached_external_docs,
        |parent_doc_path, path_item, cached_external_docs| {
          self.inline_path_item(
            parent_doc_path,
            path_item,
            &mut components.schemas,
            cached_external_docs,
          )
        },
      );
    }

    InlineApi(openapi)
  }

  fn inline_components(&self, components: &mut Components, cached_external_docs: &mut DocCache) {
    for (_, security_scheme) in &mut components.security_schemes {
      self.inline_reference_or_item(
        &self.openapi_path,
        security_scheme,
        cached_external_docs,
        |_, _, _| (),
      );
    }

    for (_, response) in &mut components.responses {
      self.inline_reference_or_item(
        &self.openapi_path,
        response,
        cached_external_docs,
        |parent_doc_path, response, cached_external_docs| {
          self.inline_response(
            parent_doc_path,
            response,
            &mut components.schemas,
            cached_external_docs,
          )
        },
      );
    }

    for (_, parameter) in &mut components.parameters {
      self.inline_reference_or_item(
        &self.openapi_path,
        parameter,
        cached_external_docs,
        |parent_doc_path, parameter, cached_external_docs| {
          self.inline_parameter(
            parent_doc_path,
            parameter,
            &mut components.schemas,
            cached_external_docs,
          )
        },
      );
    }

    for (_, example) in &mut components.examples {
      self.inline_reference_or_item(
        &self.openapi_path,
        example,
        cached_external_docs,
        |_, _, _| (),
      );
    }

    for (_, request_body) in &mut components.request_bodies {
      self.inline_reference_or_item(
        &self.openapi_path,
        request_body,
        cached_external_docs,
        |parent_doc_path, request_body, cached_external_docs| {
          self.inline_request_body(
            parent_doc_path,
            request_body,
            &mut components.schemas,
            cached_external_docs,
          )
        },
      );
    }

    for (_, header) in &mut components.headers {
      self.inline_reference_or_item(
        &self.openapi_path,
        header,
        cached_external_docs,
        |parent_doc_path, header, cached_external_docs| {
          self.inline_header(
            parent_doc_path,
            header,
            &mut components.schemas,
            cached_external_docs,
          )
        },
      );
    }

    // We can't borrow components.schemas mutably twice, so we create a temporary copy for importing
    // foreign schemas, and then we merge those in below.
    let mut inlined_schemas = components.schemas.clone();

    for (_, schema) in &mut components.schemas {
      self.inline_reference_or_schema(
        &self.openapi_path,
        schema,
        &mut inlined_schemas,
        cached_external_docs,
      )
    }

    for (name, schema) in inlined_schemas {
      // The only modification to `inlined_schemas` should be newly inserted schemas that were the
      // targets of foreign references. However, existing entries in components.schemas may have
      // been modified after we cloned it, so we don't overwrite those.
      if !components.schemas.contains_key(&name) {
        components.schemas.insert(name, schema);
      }
    }

    for (_, link) in &mut components.links {
      self.inline_reference_or_item(&self.openapi_path, link, cached_external_docs, |_, _, _| ())
    }

    for (_, callback) in &mut components.callbacks {
      self.inline_reference_or_item(
        &self.openapi_path,
        callback,
        cached_external_docs,
        |parent_doc_path, callback, cached_external_docs| {
          self.inline_callback(
            parent_doc_path,
            callback,
            &mut components.schemas,
            cached_external_docs,
          )
        },
      );
    }

    // We just leave `components.extensions` alone for now.
  }

  // NB: Don't use this for schemas because the inlining loses the name of the reference target.
  // Use `inline_reference_or_schema` for schemas instead, which copies the target to
  // `components.schemas` instead and preserves the name.
  fn inline_reference_or_item<F, T>(
    &self,
    parent_doc_path: &Path,
    reference_or: &mut ReferenceOr<T>,
    cached_external_docs: &mut DocCache,
    mut inline_fn: F,
  ) where
    F: FnMut(&Path, &mut T, &mut DocCache),
    T: DeserializeOwned,
  {
    match reference_or {
      ReferenceOr::Reference { reference } => {
        let (
          target_doc_path,
          ResolvedReference {
            root_rel_ref: rel_ref,
            mut target,
            ..
          },
        ) = resolve_reference::<T>(parent_doc_path, reference, cached_external_docs);

        // If the reference target is in the root OpenAPI spec, don't update it here since we'll
        // process it directly. As much as possible, we try to leave local references in place
        // so that the size of the final OpenAPI spec doesn't due to excessive inlining.
        if target_doc_path != *self.openapi_path {
          inline_fn(&target_doc_path, &mut target, cached_external_docs);

          // NB: This drops the name of the reference target, which seems fine for non-schema
          // references, although it may increase the size of the final OpenAPI spec. If this
          // becomes a problem, we can inline into the appropriate field of `openapi.components`
          // instead.
          *reference_or = ReferenceOr::Item(target);
        } else {
          // Sometimes we have foreign references from a non-root definition back to the root
          // definition file. In that case, we want to replace the reference with a local one since
          // we've inlined the referer into the root definition.
          *reference_or = ReferenceOr::Reference {
            reference: format!("#/{rel_ref}"),
          }
        }
      }
      ReferenceOr::Item(item) => {
        inline_fn(parent_doc_path, item, cached_external_docs);
      }
    }
  }

  fn inline_reference_or_schema<T>(
    &self,
    parent_doc_path: &Path,
    reference_or_schema: &mut ReferenceOr<T>,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) where
    T: BorrowMut<Schema> + std::fmt::Debug + From<Schema>,
  {
    match reference_or_schema {
      ReferenceOr::Reference { reference } => {
        let (
          target_doc_path,
          ResolvedReference {
            root_rel_ref: rel_ref,
            mut target,
            target_name,
          },
        ) = resolve_reference::<Schema>(parent_doc_path, reference, cached_external_docs);

        // If the reference target is in the root OpenAPI spec, don't update it here since we'll
        // process it directly. As much as possible, we try to leave local references in place
        // so that the size of the final OpenAPI spec doesn't due to excessive inlining.
        if target_doc_path != *self.openapi_path {
          self.inline_schema(
            &target_doc_path,
            &mut target,
            components_schemas,
            cached_external_docs,
          );

          // To preserve schema names (which will become Rust type names in the generated code),
          // we insert them into #/components/schemas (if there isn't already a schema with the
          // same name) and replace the foreign reference with a local reference. If there is a
          // conflicting schema with the same name, we just inline it and handle name conflict
          // resolution later, when generating the models.

          match components_schemas.get(target_name) {
            Some(ReferenceOr::Item(existing_schema_with_name))
              if *existing_schema_with_name == target =>
            {
              *reference_or_schema = ReferenceOr::Reference {
                reference: format!("#/components/schemas/{target_name}"),
              };
            }
            Some(_) => {
              *reference_or_schema = ReferenceOr::Item(T::from(target));
            }
            None => {
              components_schemas.insert(target_name.to_string(), ReferenceOr::Item(target));
              *reference_or_schema = ReferenceOr::Reference {
                reference: format!("#/components/schemas/{target_name}"),
              };
            }
          }
        } else {
          // Sometimes we have foreign references from a non-root definition back to the root
          // definition file. In that case, we want to replace the reference with a local one since
          // we've inlined the referer into the root definition.
          *reference_or_schema = ReferenceOr::Reference {
            reference: format!("#/{rel_ref}"),
          }
        }
      }
      ReferenceOr::Item(item) => {
        self.inline_schema(
          parent_doc_path,
          item.borrow_mut(),
          components_schemas,
          cached_external_docs,
        );
      }
    }
  }

  fn inline_callback(
    &self,
    parent_doc_path: &Path,
    callback: &mut Callback,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    for (_, path_item) in callback {
      self.inline_path_item(
        parent_doc_path,
        path_item,
        components_schemas,
        cached_external_docs,
      )
    }
  }

  fn inline_header(
    &self,
    parent_doc_path: &Path,
    header: &mut Header,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    self.inline_parameter_schema_or_content(
      parent_doc_path,
      &mut header.format,
      components_schemas,
      cached_external_docs,
    );

    for (_, example) in &mut header.examples {
      self.inline_reference_or_item(parent_doc_path, example, cached_external_docs, |_, _, _| ())
    }
  }

  fn inline_media_type(
    &self,
    parent_doc_path: &Path,
    media_type: &mut MediaType,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    if let Some(schema) = &mut media_type.schema {
      self.inline_reference_or_schema(
        parent_doc_path,
        schema,
        components_schemas,
        cached_external_docs,
      )
    }

    for (_, example) in &mut media_type.examples {
      self.inline_reference_or_item(parent_doc_path, example, cached_external_docs, |_, _, _| ())
    }
  }

  fn inline_operation(
    &self,
    parent_doc_path: &Path,
    operation: &mut Operation,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    for parameter in &mut operation.parameters {
      self.inline_reference_or_item(
        parent_doc_path,
        parameter,
        cached_external_docs,
        |parent_doc_path, parameter, cached_external_docs| {
          self.inline_parameter(
            parent_doc_path,
            parameter,
            components_schemas,
            cached_external_docs,
          )
        },
      );
    }

    if let Some(request_body) = &mut operation.request_body {
      self.inline_reference_or_item(
        parent_doc_path,
        request_body,
        cached_external_docs,
        |parent_doc_path, request_body, cached_external_docs| {
          self.inline_request_body(
            parent_doc_path,
            request_body,
            components_schemas,
            cached_external_docs,
          )
        },
      );
    }

    self.inline_responses(
      parent_doc_path,
      &mut operation.responses,
      components_schemas,
      cached_external_docs,
    );
  }

  fn inline_parameter(
    &self,
    parent_doc_path: &Path,
    parameter: &mut Parameter,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    let parameter_data = match parameter {
      Parameter::Query { parameter_data, .. }
      | Parameter::Header { parameter_data, .. }
      | Parameter::Path { parameter_data, .. }
      | Parameter::Cookie { parameter_data, .. } => parameter_data,
    };

    self.inline_parameter_schema_or_content(
      parent_doc_path,
      &mut parameter_data.format,
      components_schemas,
      cached_external_docs,
    );

    for (_, example) in &mut parameter_data.examples {
      self.inline_reference_or_item(parent_doc_path, example, cached_external_docs, |_, _, _| ())
    }
  }

  fn inline_parameter_schema_or_content(
    &self,
    parent_doc_path: &Path,
    parameter_schema_or_content: &mut ParameterSchemaOrContent,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    match parameter_schema_or_content {
      ParameterSchemaOrContent::Schema(schema) => self.inline_reference_or_schema(
        parent_doc_path,
        schema,
        components_schemas,
        cached_external_docs,
      ),
      ParameterSchemaOrContent::Content(content) => {
        for (_, media_type) in content {
          self.inline_media_type(
            parent_doc_path,
            media_type,
            components_schemas,
            cached_external_docs,
          )
        }
      }
    }
  }

  fn inline_path_item(
    &self,
    parent_doc_path: &Path,
    path_item: &mut PathItem,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
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
      .for_each(|operation| {
        self.inline_operation(
          parent_doc_path,
          operation,
          components_schemas,
          cached_external_docs,
        )
      });

    for parameter in &mut path_item.parameters {
      self.inline_reference_or_item(
        parent_doc_path,
        parameter,
        cached_external_docs,
        |parent_doc_path, parameter, cached_external_docs| {
          self.inline_parameter(
            parent_doc_path,
            parameter,
            components_schemas,
            cached_external_docs,
          )
        },
      );
    }
  }

  fn inline_request_body(
    &self,
    parent_doc_path: &Path,
    request_body: &mut RequestBody,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    for (_, media_type) in &mut request_body.content {
      self.inline_media_type(
        parent_doc_path,
        media_type,
        components_schemas,
        cached_external_docs,
      )
    }
  }

  fn inline_response(
    &self,
    parent_doc_path: &Path,
    response: &mut Response,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    for (_, header) in &mut response.headers {
      self.inline_reference_or_item(
        parent_doc_path,
        header,
        cached_external_docs,
        |parent_doc_path, header, cached_external_docs| {
          self.inline_header(
            parent_doc_path,
            header,
            components_schemas,
            cached_external_docs,
          )
        },
      )
    }

    for (_, media_type) in &mut response.content {
      self.inline_media_type(
        parent_doc_path,
        media_type,
        components_schemas,
        cached_external_docs,
      )
    }

    for (_, link) in &mut response.links {
      self.inline_reference_or_item(parent_doc_path, link, cached_external_docs, |_, _, _| ())
    }
  }

  fn inline_responses(
    &self,
    parent_doc_path: &Path,
    responses: &mut Responses,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    if let Some(default) = &mut responses.default {
      self.inline_reference_or_item(
        parent_doc_path,
        default,
        cached_external_docs,
        |parent_doc_path, response, cached_external_docs| {
          self.inline_response(
            parent_doc_path,
            response,
            components_schemas,
            cached_external_docs,
          )
        },
      );
    }

    for (_, response) in &mut responses.responses {
      self.inline_reference_or_item(
        parent_doc_path,
        response,
        cached_external_docs,
        |parent_doc_path, response, cached_external_docs| {
          self.inline_response(
            parent_doc_path,
            response,
            components_schemas,
            cached_external_docs,
          )
        },
      );
    }
  }

  fn inline_schema(
    &self,
    parent_doc_path: &Path,
    schema: &mut Schema,
    components_schemas: &mut IndexMap<String, ReferenceOr<Schema>>,
    cached_external_docs: &mut DocCache,
  ) {
    match &mut schema.schema_kind {
      SchemaKind::Type(schema_type) => match schema_type {
        Type::Object(object) => {
          for (_, property) in &mut object.properties {
            self.inline_reference_or_schema(
              parent_doc_path,
              property,
              components_schemas,
              cached_external_docs,
            )
          }

          if let Some(AdditionalProperties::Schema(additional_properties)) =
            &mut object.additional_properties
          {
            self.inline_reference_or_schema(
              parent_doc_path,
              additional_properties,
              components_schemas,
              cached_external_docs,
            )
          }
        }
        Type::Array(array) => {
          if let Some(items) = &mut array.items {
            self.inline_reference_or_schema(
              parent_doc_path,
              items,
              components_schemas,
              cached_external_docs,
            )
          }
        }
        Type::String(_) | Type::Number(_) | Type::Integer(_) | Type::Boolean { .. } => {}
      },
      SchemaKind::OneOf { one_of: inner }
      | SchemaKind::AllOf { all_of: inner }
      | SchemaKind::AnyOf { any_of: inner } => inner.iter_mut().for_each(|inner_schema_or_ref| {
        self.inline_reference_or_schema(
          parent_doc_path,
          inner_schema_or_ref,
          components_schemas,
          cached_external_docs,
        )
      }),
      SchemaKind::Not { not } => self.inline_reference_or_schema(
        parent_doc_path,
        not,
        components_schemas,
        cached_external_docs,
      ),
      SchemaKind::Any(any) => {
        for (_, schema) in &mut any.properties {
          self.inline_reference_or_schema(
            parent_doc_path,
            schema,
            components_schemas,
            cached_external_docs,
          )
        }

        if let Some(AdditionalProperties::Schema(additional_properties)) =
          &mut any.additional_properties
        {
          self.inline_reference_or_schema(
            parent_doc_path,
            additional_properties,
            components_schemas,
            cached_external_docs,
          )
        }

        if let Some(items) = &mut any.items {
          self.inline_reference_or_schema(
            parent_doc_path,
            items,
            components_schemas,
            cached_external_docs,
          )
        }

        any
          .one_of
          .iter_mut()
          .chain(any.all_of.iter_mut())
          .chain(any.any_of.iter_mut())
          .for_each(|inner_schema_or_ref| {
            self.inline_reference_or_schema(
              parent_doc_path,
              inner_schema_or_ref,
              components_schemas,
              cached_external_docs,
            )
          });

        if let Some(not) = &mut any.not {
          self.inline_reference_or_schema(
            parent_doc_path,
            not,
            components_schemas,
            cached_external_docs,
          )
        }
      }
    }

    if let Some(discriminator) = &mut schema.schema_data.discriminator {
      for (_, schema_ref) in &mut discriminator.mapping {
        let mut temp_ref = ReferenceOr::<Schema>::Reference {
          reference: schema_ref.to_owned(),
        };

        self.inline_reference_or_schema(
          parent_doc_path,
          &mut temp_ref,
          components_schemas,
          cached_external_docs,
        );

        match temp_ref {
          ReferenceOr::Reference { reference } => {
            *schema_ref = reference;
          }
          // Since we already inlined all of the oneOf variant schemas above, this should only
          // happen if the mapping points to a schema that isn't listed under oneOf/anyOf (which is
          // an error in the OpenAPI definition).
          ReferenceOr::Item(inlined) => panic!(
            "discriminator-mapped reference {schema_ref} unexpectedly inlined to schema {inlined:#?}",
          ),
        }
      }
    }
  }
}
