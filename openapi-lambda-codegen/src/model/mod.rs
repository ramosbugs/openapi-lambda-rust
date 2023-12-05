#![allow(clippy::too_many_arguments)]

use crate::inline::InlineApi;
use crate::{description_to_doc_attr, CodeGenerator};

use convert_case::{Case, Casing};
use indexmap::{IndexMap, IndexSet};
use itertools::Either;
use openapiv3::{
  AdditionalProperties, AnySchema, ArrayType, BooleanType, Components, Discriminator,
  IntegerFormat, IntegerType, NumberFormat, NumberType, ObjectType, ReferenceOr, Schema,
  SchemaKind, StringFormat, StringType, Type, VariantOrUnknownOrEmpty,
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use unzip_n::unzip_n;

use std::borrow::{Borrow, Cow};
use std::collections::HashMap;

mod name_model_schemas;

#[cfg(test)]
mod tests;

unzip_n!(3);

/// Used by [`CodeGenerator::inline_ref_or_schema`] to determine whether to inline schema references
/// or generate code that points to a separate generated model. During model generation, the
/// `InProgress` variant is used, since the referenced schema may not have been processed yet, and
/// we'll need to process it immediately in order to determine whether to inline the schema. After
/// model generation, the `Done` variant is used, in which case we can just consult the list of
/// generated models to see whether or not to inline the schema.
pub(crate) enum GeneratedModels<'a> {
  InProgress {
    models: &'a mut HashMap<Ident, TokenStream>,
    models_in_progress: &'a mut IndexSet<Ident>,
  },
  Done(&'a HashMap<Ident, TokenStream>),
}

impl CodeGenerator {
  /// Generate models and update OpenAPI with unnamed models replaced by references to new, named
  /// models inserted into `components/schemas/`.
  pub(crate) fn generate_models(
    &self,
    mut openapi: InlineApi,
  ) -> (InlineApi, HashMap<Ident, TokenStream>) {
    // Moves all schemas for which we need to generate Rust models into openapi.components.schemas.
    name_model_schemas::visit_openapi(&mut openapi);

    // If there are still no components, then there are no models to generate.
    let Some(components) = &openapi.components else {
      return (openapi, HashMap::new());
    };

    let models = self.generate_components(components);
    (openapi, models)
  }

  fn generate_components(&self, components: &Components) -> HashMap<Ident, TokenStream> {
    let mut models = HashMap::new();
    // We use an IndexSet here so that the panic output is in the same order as the dependency
    // cycle.
    let mut models_in_progress = IndexSet::new();
    components.schemas.iter().for_each(|(model_name, schema)| {
      let ReferenceOr::Item(schema) = schema else {
        // If there are any references within `components.schemas`, we know they're unused since
        // we would have panicked on the reference chain (which we don't currently support).
        return;
      };

      let model_ident = self.identifier(&model_name.to_case(Case::Pascal));
      self.generate_model(
        model_ident,
        schema,
        &components.schemas,
        &mut models,
        &mut models_in_progress,
      );
      assert!(models_in_progress.is_empty());
    });

    models
  }

  /// Recursively generate the specified model and any models that it depends on that have not yet
  /// been generated.
  ///
  /// Returns true iff a model was generated. This function (including its callees) is the
  /// single source of truth for whether a model is generated for a given type. When we encounter
  /// schema references during model generation, the only way to know whether to inline that schema
  /// or generate code pointing to a named model is to call this function. Once model generation is
  /// complete, code elsewhere in the crate can check the `HashMap` of generated models to determine
  /// how to handle references.
  fn generate_model(
    &self,
    model_ident: Ident,
    schema: &Schema,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> bool {
    if models.contains_key(&model_ident) {
      return true;
    }

    // Prevent infinite recursion.
    if models_in_progress.contains(&model_ident) {
      panic!("dependency cycle detected between models: {models_in_progress:#?}");
    }
    models_in_progress.insert(model_ident.clone());

    let model = match &schema.schema_kind {
      SchemaKind::Type(schema_type) => match schema_type {
        Type::Object(object) => self.generate_object_model(
          &model_ident,
          object,
          components_schemas,
          models,
          models_in_progress,
        ),
        Type::Array(_) => None,
        Type::String(string) => self.generate_string_model(&model_ident, string),
        Type::Integer(integer) => self.generate_integer_model(&model_ident, integer),
        Type::Number(number) => self.generate_number_model(&model_ident, number),
        Type::Boolean(boolean) => self.generate_boolean_model(&model_ident, boolean),
      },
      SchemaKind::OneOf { one_of } => {
        if let Some(discriminator) = &schema.schema_data.discriminator {
          Some(self.generate_tagged_enum_model(
            &model_ident,
            one_of,
            discriminator,
            components_schemas,
            models,
            models_in_progress,
          ))
        } else {
          Some(self.generate_untagged_enum_model(
            &model_ident,
            one_of,
            components_schemas,
            models,
            models_in_progress,
          ))
        }
      }
      SchemaKind::AnyOf { .. } => {
        unimplemented!("`anyOf` schema {schema:#?}");
      }
      SchemaKind::AllOf { all_of } => Some(self.generate_composed_object_model(
        &model_ident,
        all_of,
        components_schemas,
        models,
        models_in_progress,
      )),
      SchemaKind::Not { .. } => {
        unimplemented!("`not` schema {schema:#?}");
      }
      SchemaKind::Any(any) => {
        if *any != AnySchema::default() {
          unimplemented!("`any` schema: {any:#?}");
        }

        // Don't generate models for types we can represent inline,
        None
      }
    };

    models_in_progress.remove(&model_ident);

    if let Some(model) = model {
      let model_with_docs = if let Some(description) = &schema.schema_data.description {
        let doc_attr = description_to_doc_attr(description);
        quote! {
          #doc_attr
          #model
        }
      } else {
        model
      };

      models.insert(model_ident, model_with_docs);
      true
    } else {
      false
    }
  }

  fn generate_object_struct_properties(
    &self,
    properties: &IndexMap<String, ReferenceOr<Box<Schema>>>,
    required: &[String],
    is_enum_variant: bool,
    tag_field_to_exclude: Option<&str>,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> Vec<TokenStream> {
    properties
      .iter()
      // Don't include the discriminator field of a tagged enum, since serde consumes it to
      // determine the enum variant, it's not available to the variant's body. The OpenAPI spec
      // explicitly requires the discriminator field to be present in each `oneOf` component,
      // although we assume it's a string and don't enforce that it's explicitly defined.
      .filter(|(property_name, _)| {
        !tag_field_to_exclude
          .is_some_and(|tag_field_to_exclude| tag_field_to_exclude == property_name.as_str())
      })
      .map(|(property_name, ref_or_schema)| {
        let property_ident = self.identifier(&property_name.to_case(Case::Snake));
        let (property_type_inner, property_description) = self.inline_ref_or_schema(
          ref_or_schema,
          components_schemas,
          GeneratedModels::InProgress {
            models,
            models_in_progress,
          },
        );

        let serde_rename = if property_ident != property_name {
          Some(quote! { rename = #property_name })
        } else {
          None
        };

        let r#pub = if is_enum_variant {
          quote! {}
        } else {
          quote! { pub }
        };

        let doc_attr = if let Some(description) = property_description {
          description_to_doc_attr(&description)
        } else {
          quote! {}
        };
        if required.contains(property_name) {
          let serde_attrs = serde_rename
            .map(|rename| quote! { #[serde(#rename)] })
            .unwrap_or_default();
          quote! {
            #doc_attr
            #serde_attrs
            #r#pub #property_ident: #property_type_inner,
          }
        } else {
          let serde_attrs = serde_rename
            .map(|rename| quote! { #rename, skip_serializing_if = "Option::is_none" })
            .unwrap_or_else(|| quote! { skip_serializing_if = "Option::is_none" });
          quote! {
            #doc_attr
            #[serde(#serde_attrs)]
            #r#pub #property_ident: Option<#property_type_inner>,
          }
        }
      })
      .collect()
  }

  fn generate_object_struct_additional_properties_type(
    &self,
    additional_properties: Option<&AdditionalProperties>,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> Option<TokenStream> {
    let additional_property_type = match additional_properties.as_ref() {
      None | Some(AdditionalProperties::Any(false)) => None,
      Some(AdditionalProperties::Any(true)) => Some(self.inline_any_type()),
      Some(AdditionalProperties::Schema(ref_or_schema)) => Some(
        self
          .inline_ref_or_schema(
            ref_or_schema,
            components_schemas,
            GeneratedModels::InProgress {
              models,
              models_in_progress,
            },
          )
          .0,
      ),
    };

    additional_property_type.map(|additional_property_type| {
      quote! { std::collections::HashMap<String, #additional_property_type> }
    })
  }

  fn generate_object_struct_body(
    &self,
    object: &ObjectType,
    is_enum_variant: bool,
    tag_field_to_exclude: Option<&str>,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> TokenStream {
    let ObjectType {
      properties,
      required,
      additional_properties,
      ..
    } = object;

    let fields = self.generate_object_struct_properties(
      properties,
      required,
      is_enum_variant,
      tag_field_to_exclude,
      components_schemas,
      models,
      models_in_progress,
    );

    let additional_properties_type = self.generate_object_struct_additional_properties_type(
      additional_properties.as_ref(),
      components_schemas,
      models,
      models_in_progress,
    );

    if fields.is_empty() {
      if let Some(additional_properties_type) = additional_properties_type {
        quote! { (#additional_properties_type) }
      } else {
        quote! { {} }
      }
    } else {
      let fields_tok = fields.into_iter().collect::<TokenStream>();
      let additional_properties_tok =
        if let Some(additional_properties_type) = additional_properties_type {
          let r#pub = if is_enum_variant {
            quote! {}
          } else {
            quote! { pub }
          };

          quote! {
            #[serde(flatten)]
            #r#pub additional_properties: #additional_properties_type,
          }
        } else {
          quote! {}
        };

      quote! {
        {
          #fields_tok
          #additional_properties_tok
        }
      }
    }
  }

  fn generate_object_model(
    &self,
    model_ident: &Ident,
    object: &ObjectType,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> Option<TokenStream> {
    if object.properties.is_empty() {
      return None;
    }

    let struct_body = self.generate_object_struct_body(
      object,
      false,
      None,
      components_schemas,
      models,
      models_in_progress,
    );
    let serde_crate_attr = self.serde_crate_attr();
    Some(quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(#serde_crate_attr)]
      pub struct #model_ident #struct_body
    })
  }

  fn flatten_composed_object_components<'a, I>(
    &'a self,
    model_ident: &'a Ident,
    components: I,
    components_schemas: &'a IndexMap<String, ReferenceOr<Schema>>,
  ) -> Box<dyn Iterator<Item = &'a ObjectType> + 'a>
  where
    I: IntoIterator<Item = &'a ReferenceOr<Schema>> + 'a,
  {
    Box::new(components.into_iter().flat_map(
      move |component: &ReferenceOr<Schema>| match component {
        ReferenceOr::Item(schema) => match &schema.schema_kind {
          SchemaKind::Type(Type::Object(object)) => Box::new(std::iter::once(object)),
          SchemaKind::AllOf { all_of } => {
            self.flatten_composed_object_components(model_ident, all_of, components_schemas)
          }
          SchemaKind::Type(_)
          | SchemaKind::OneOf { .. }
          | SchemaKind::AnyOf { .. }
          | SchemaKind::Not { .. }
          | SchemaKind::Any(_) => {
            panic!(
              "unexpected `allOf` component type (must be object or nested `allOf`): {schema:#?}",
            )
          }
        },
        ReferenceOr::Reference { reference } => {
          let target_schema_name = self.reference_schema_name(reference);
          let Some(target) = components_schemas.get(target_schema_name) else {
            panic!("invalid schema reference `{reference}` from model `{model_ident}`");
          };
          self.flatten_composed_object_components(model_ident, [target], components_schemas)
        }
      },
    ))
  }

  fn generate_composed_object_struct_body(
    &self,
    model_ident: &Ident,
    components: &[ReferenceOr<Schema>],
    is_enum_variant: bool,
    tag_field_to_exclude: Option<&str>,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> TokenStream {
    let (properties, additional_properties) = self
      .flatten_composed_object_components(model_ident, components, components_schemas)
      .fold(
        (TokenStream::default(), None),
        |(mut properties_acc, model_additional_properties), component| {
          let ObjectType {
            properties,
            required,
            additional_properties,
            ..
          } = component;

          let fields = self.generate_object_struct_properties(
            properties,
            required,
            is_enum_variant,
            tag_field_to_exclude,
            components_schemas,
            models,
            models_in_progress,
          );

          if model_additional_properties.is_some() && additional_properties.is_some() {
            panic!(
              "only one `additionalProperties` value is allowed in `allOf` schema {model_ident}: \
             {components:#?}",
            );
          }

          properties_acc.extend(fields);
          (
            properties_acc,
            additional_properties
              .as_ref()
              .or(model_additional_properties),
          )
        },
      );

    let additional_properties_type = self.generate_object_struct_additional_properties_type(
      additional_properties,
      components_schemas,
      models,
      models_in_progress,
    );

    let additional_properties_tok =
      if let Some(additional_properties_type) = additional_properties_type {
        let r#pub = if is_enum_variant {
          quote! {}
        } else {
          quote! { pub }
        };

        quote! {
          #[serde(flatten)]
          #r#pub additional_properties: #additional_properties_type,
        }
      } else {
        quote! {}
      };

    quote!({
      #properties
      #additional_properties_tok
    })
  }

  fn generate_composed_object_model(
    &self,
    model_ident: &Ident,
    components: &[ReferenceOr<Schema>],
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> TokenStream {
    let struct_body = self.generate_composed_object_struct_body(
      model_ident,
      components,
      false,
      None,
      components_schemas,
      models,
      models_in_progress,
    );
    let serde_crate_attr = self.serde_crate_attr();
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(#serde_crate_attr)]
      pub struct #model_ident #struct_body
    }
  }

  fn generate_tagged_enum_model(
    &self,
    model_ident: &Ident,
    variants: &[ReferenceOr<Schema>],
    discriminator: &Discriminator,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> TokenStream {
    if discriminator.property_name.is_empty() {
      panic!("unexpected empty discriminator in `oneOf` model `{model_ident}`");
    };

    let tag_field = &discriminator.property_name;

    let variants_by_name = variants
      .iter()
      .map(|variant| {
        let ReferenceOr::Reference { reference } = variant else {
          panic!(
            "unexpected inline schema in `oneOf` schema `{model_ident}`: enum variants must be \
             references to named schemas: {variant:#?}",
          )
        };

        let target_schema_name = self.reference_schema_name(reference);
        let Some(ReferenceOr::Item(target)) = components_schemas.get(target_schema_name) else {
          panic!(
            "invalid schema reference `{reference}` from model `{model_ident}`: target schema does \
             not exist",
          );
        };

        (target_schema_name.to_string(), target)
      })
      .collect::<IndexMap<_, _>>();

    let variants_tok = if !discriminator.mapping.is_empty() {
      Either::Left(
        discriminator
          .mapping
          .iter()
          .map(|(tag_value, variant_ref)| {
            let variant_name = self.reference_schema_name(variant_ref);

            let Some(variant_schema) = variants_by_name.get(variant_name) else {
              panic!(
                "`oneOf` type `{model_ident}` maps discriminator value `{tag_value}` to unknown \
                 type `{variant_name}`"
              );
            };

            (tag_value, variant_name, variant_schema)
          }),
      )
    } else {
      Either::Right(
        variants_by_name
          .iter()
          .map(|(variant_name, variant_schema)| {
            (variant_name, variant_name.as_str(), variant_schema)
          }),
      )
    }
    .map(|(tag_value, variant_name, variant_schema)| {
      let variant_ident = self.identifier(&variant_name.to_case(Case::Pascal));

      let variant_tok = self.generate_enum_variant(
        model_ident,
        &variant_ident,
        variant_schema,
        Some(tag_field),
        components_schemas,
        models,
        models_in_progress,
      );

      let serde_rename = if variant_ident != tag_value {
        quote! { #[serde(rename = #tag_value)] }
      } else {
        quote! {}
      };

      quote! {
        #serde_rename
        #variant_tok
      }
    })
    .collect::<TokenStream>();

    let serde_crate_attr = self.serde_crate_attr();
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(#serde_crate_attr, tag = #tag_field)]
      pub enum #model_ident {
        #variants_tok
      }
    }
  }

  fn generate_enum_variant(
    &self,
    model_ident: &Ident,
    variant_ident: &Ident,
    variant_schema: &Schema,
    tag_field_to_exclude: Option<&str>,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> TokenStream {
    let struct_body = match &variant_schema.schema_kind {
      SchemaKind::Type(Type::Object(object)) => {
        if object.properties.is_empty()
          && matches!(
            object.additional_properties,
            None | Some(AdditionalProperties::Any(false))
          )
        {
          quote! {}
        } else {
          self.generate_object_struct_body(
            object,
            true,
            tag_field_to_exclude,
            components_schemas,
            models,
            models_in_progress,
          )
        }
      }
      SchemaKind::AllOf { all_of } => self.generate_composed_object_struct_body(
        model_ident,
        all_of,
        true,
        tag_field_to_exclude,
        components_schemas,
        models,
        models_in_progress,
      ),
      _ => panic!(
        "variant of `oneOf` type `{model_ident}` with discriminator must be an object type: \
         {variant_schema:#?}",
      ),
    };

    let doc_attr = if let Some(description) = &variant_schema.schema_data.description {
      description_to_doc_attr(&description)
    } else {
      quote! {}
    };

    quote! {
      #doc_attr
      #variant_ident #struct_body,
    }
  }

  fn generate_untagged_enum_model(
    &self,
    model_ident: &Ident,
    variants: &[ReferenceOr<Schema>],
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    models: &mut HashMap<Ident, TokenStream>,
    models_in_progress: &mut IndexSet<Ident>,
  ) -> TokenStream {
    let variants_tok = variants
      .iter()
      .map(|variant| {
        let ReferenceOr::Reference { reference } = variant else {
          panic!(
            "unexpected inline schema in `oneOf` schema `{model_ident}`: enum variants must be \
             references to named schemas: {variant:#?}",
          )
        };

        let variant_name = self.reference_schema_name(reference);
        let Some(ReferenceOr::Item(variant_schema)) = components_schemas.get(variant_name) else {
          panic!(
            "invalid schema reference `{reference}` from model `{model_ident}`: target schema does \
             not exist",
          );
        };

        let variant_ident = self.identifier(&variant_name.to_case(Case::Pascal));

        self.generate_enum_variant(
          model_ident,
          &variant_ident,
          variant_schema,
          None,
          components_schemas,
          models,
          models_in_progress,
        )
      })
      .collect::<TokenStream>();

    let serde_crate_attr = self.serde_crate_attr();
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(#serde_crate_attr, untagged)]
      pub enum #model_ident {
        #variants_tok
      }
    }
  }

  fn generate_boolean_model(
    &self,
    model_ident: &Ident,
    boolean: &BooleanType,
  ) -> Option<TokenStream> {
    let BooleanType { enumeration, .. } = boolean;

    // Don't generate models for types we can represent inline,
    if enumeration.is_empty() {
      return None;
    }

    if enumeration.contains(&None) {
      unimplemented!("nullable enum {model_ident}: {enumeration:#?}");
    }

    unimplemented!("boolean enum {model_ident}: {enumeration:#?}");
  }

  fn generate_integer_model(
    &self,
    model_ident: &Ident,
    integer: &IntegerType,
  ) -> Option<TokenStream> {
    let IntegerType { enumeration, .. } = integer;

    // Don't generate models for types we can represent inline,
    if enumeration.is_empty() {
      return None;
    }

    if enumeration.contains(&None) {
      unimplemented!("nullable enum {model_ident}: {enumeration:#?}");
    }

    // See https://serde.rs/enum-number.html.
    unimplemented!("integer enum {model_ident}: {enumeration:#?}");
  }

  fn generate_number_model(&self, model_ident: &Ident, number: &NumberType) -> Option<TokenStream> {
    let NumberType { enumeration, .. } = number;

    // Don't generate models for types we can represent inline,
    if enumeration.is_empty() {
      return None;
    }

    if enumeration.contains(&None) {
      unimplemented!("nullable enum {model_ident}: {enumeration:#?}");
    }

    // See https://serde.rs/enum-number.html.
    unimplemented!("number enum {model_ident}: {enumeration:#?}");
  }

  fn generate_string_model(&self, model_ident: &Ident, string: &StringType) -> Option<TokenStream> {
    let StringType {
      enumeration,
      // TODO: Support patterned strings with regex validation during deserialization.
      ..
    } = string;

    let serde_crate_attr = self.serde_crate_attr();

    // Don't generate models for types we can represent inline,
    if enumeration.is_empty() {
      return None;
    }
    if enumeration.contains(&None) {
      unimplemented!("nullable enum {model_ident}: {enumeration:#?}");
    }

    let (variants, parse_cases, as_str_cases) = enumeration
      .iter()
      .map(|variant| {
        let variant = variant.as_ref().expect("enum should not be nullable");
        let variant_pascal = variant.to_case(Case::Pascal);
        let variant_ident = self.identifier(&match variant.as_str().chars().next() {
          // Hopefully users won't have both empty string and literal `empty_string` (or any other
          // version that collides as PascalCase) as variants.
          None => Cow::Borrowed("EmptyString"),
          // If the variant doesn't start with a valid starting character for a Rust identifier,
          // prefix it with `__`. Hopefully this won't collide with other variant names.
          Some(c) if c != '_' && !unicode_ident::is_xid_start(c) => {
            Cow::Owned(format!("__{variant_pascal}"))
          }
          _ => Cow::Borrowed(variant_pascal.as_str()),
        });
        let variant_tok = if variant_ident != variant {
          quote! {
            #[serde(rename = #variant)]
            #variant_ident,
          }
        } else {
          quote! { #variant_ident, }
        };

        (
          variant_tok,
          quote! { #variant => Ok(Self::#variant_ident), },
          quote! { Self::#variant_ident => #variant, },
        )
      })
      .unzip_n::<TokenStream, TokenStream, TokenStream>();

    Some(quote! {
      #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
      #[serde(#serde_crate_attr)]
      pub enum #model_ident {
        #variants
      }
      impl #model_ident {
        fn as_str(&self) -> &'static str {
          match self {
            #as_str_cases
          }
        }
      }
      impl std::fmt::Display for #model_ident {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          write!(f, "{}", self.as_str())
        }
      }
      impl std::str::FromStr for #model_ident {
        type Err = anyhow::Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
          match s {
            #parse_cases
            _ => Err(anyhow!("invalid enum variant `{}`", s))
          }
        }
      }
    })
  }

  pub(crate) fn identifier(&self, name: &str) -> Ident {
    if let Ok(ident) = syn::parse_str::<Ident>(name) {
      ident
    } else {
      // These particular keywords can't even be used in raw identifiers (see
      // https://doc.rust-lang.org/reference/identifiers.html).
      if name == "crate" || name == "self" || name == "super" || name == "Self" {
        Ident::new(&format!("{name}_"), Span::call_site())
      } else {
        Ident::new_raw(name, Span::call_site())
      }
    }
  }

  fn reference_schema_name<'a>(&self, reference: &'a str) -> &'a str {
    const EXPECTED_PREFIX: &str = "#/components/schemas/";
    if !reference.starts_with(EXPECTED_PREFIX) {
      panic!("unexpected reference `{reference}` does not start with `{EXPECTED_PREFIX}`");
    }

    &reference[EXPECTED_PREFIX.len()..]
  }

  pub(crate) fn inline_ref_or_schema<T>(
    &self,
    ref_or_schema: &ReferenceOr<T>,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    mut generated_models: GeneratedModels,
  ) -> (TokenStream, Option<String>)
  where
    T: Borrow<Schema>,
  {
    match ref_or_schema {
      ReferenceOr::Reference { reference } => {
        let target_schema_name = self.reference_schema_name(reference);
        let Some(target) = components_schemas.get(target_schema_name) else {
          panic!("invalid schema reference `{reference}`: target schema does not exist");
        };
        let ReferenceOr::Item(target_schema) = target else {
          unimplemented!(
            "reference chains (references to references): `{reference}` -> `{target:?}`"
          );
        };

        let model_ident = self.identifier(&target_schema_name.to_case(Case::Pascal));
        let reference_points_to_model = match &mut generated_models {
          GeneratedModels::InProgress {
            models,
            models_in_progress,
          } => self.generate_model(
            model_ident.clone(),
            target_schema,
            components_schemas,
            models,
            models_in_progress,
          ),
          GeneratedModels::Done(models) => models.contains_key(&model_ident),
        };

        let schema_tok = if reference_points_to_model {
          quote! { crate::models::#model_ident }
        } else {
          self.inline_type(target_schema, components_schemas, generated_models)
        };

        (schema_tok, target_schema.schema_data.description.clone())
      }
      ReferenceOr::Item(schema) => (
        self.inline_type(schema.borrow(), components_schemas, generated_models),
        schema.borrow().schema_data.description.clone(),
      ),
    }
  }

  fn inline_type(
    &self,
    schema: &Schema,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: GeneratedModels,
  ) -> TokenStream {
    let crate_import = self.crate_use_name();
    match &schema.schema_kind {
      SchemaKind::Type(schema_type) => match schema_type {
        Type::String(string) => self.inline_string(string),
        Type::Number(number) => self.inline_number(number),
        Type::Integer(integer) => self.inline_integer(integer),
        Type::Object(ObjectType {
          properties,
          additional_properties,
          ..
        }) => {
          // Any object schema with named properties needs a named model (Rust struct).
          if !properties.is_empty() {
            panic!(
              "unexpected inline object schema must use a reference to a named schema: {schema:#?}",
            );
          }
          match additional_properties {
            None | Some(AdditionalProperties::Any(false)) => {
              quote! {
                #crate_import::models::EmptyModel
              }
            }
            Some(AdditionalProperties::Any(true)) => {
              let any = self.inline_any_type();
              quote! { std::collections::HashMap<String, #any> }
            }
            Some(AdditionalProperties::Schema(ref_or_schema)) => {
              let (additional_property_tok, _) =
                self.inline_ref_or_schema(ref_or_schema, components_schemas, generated_models);
              quote! { std::collections::HashMap<String, #additional_property_tok> }
            }
          }
        }
        Type::Array(array) => self.inline_array(array, components_schemas, generated_models),
        Type::Boolean(boolean @ BooleanType { ref enumeration }) => {
          if !enumeration.is_empty() {
            panic!("unexpected inline enum must use a reference to a named schema {boolean:#?}");
          }

          quote! { bool }
        }
      },
      SchemaKind::OneOf { .. }
      | SchemaKind::AllOf { .. }
      | SchemaKind::AnyOf { .. }
      | SchemaKind::Not { .. } => {
        panic!("unexpected inline schema must use a reference to a named schema: {schema:#?}");
      }
      SchemaKind::Any(any) => {
        if *any != AnySchema::default() {
          panic!("unexpected inline `any` schema: {any:#?}");
        }

        self.inline_any_type()
      }
    }
  }

  fn inline_any_type(&self) -> TokenStream {
    let crate_import = self.crate_use_name();
    // Should this always be a JSON value? Worst case, the user can specify their own string format
    // to be some other any-type.
    quote! {
      #crate_import::models::serde_json::Value
    }
  }

  fn inline_array(
    &self,
    array: &ArrayType,
    components_schemas: &IndexMap<String, ReferenceOr<Schema>>,
    generated_models: GeneratedModels,
  ) -> TokenStream {
    let crate_import = self.crate_use_name();
    let ArrayType {
      items,
      unique_items,
      ..
    } = array;

    let item_type = if let Some(items) = items {
      self
        .inline_ref_or_schema(items, components_schemas, generated_models)
        .0
    } else {
      self.inline_any_type()
    };

    if *unique_items {
      quote! { #crate_import::models::IndexSet<#item_type> }
    } else {
      quote! { Vec<#item_type> }
    }
  }

  fn inline_integer(&self, integer: &IntegerType) -> TokenStream {
    let IntegerType {
      format,
      enumeration,
      ..
    } = integer;

    if !enumeration.is_empty() {
      panic!("unexpected inline enum must use a reference to a named schema {integer:#?}");
    }

    match format {
      VariantOrUnknownOrEmpty::Item(integer_format) => match integer_format {
        IntegerFormat::Int32 => quote! { i32 },
        IntegerFormat::Int64 => quote! { i64 },
      },
      VariantOrUnknownOrEmpty::Unknown(integer_format) => integer_format
        .parse::<TokenStream>()
        .unwrap_or_else(|err| panic!("invalid integer type {integer_format:#?}: {err}")),
      VariantOrUnknownOrEmpty::Empty => quote! { i64 },
    }
  }

  fn inline_number(&self, number: &NumberType) -> TokenStream {
    let NumberType {
      format,
      enumeration,
      ..
    } = number;

    if !enumeration.is_empty() {
      panic!("unexpected inline enum must use a reference to a named schema {number:#?}");
    }

    match format {
      VariantOrUnknownOrEmpty::Item(number_format) => match number_format {
        NumberFormat::Float => quote! { f32 },
        NumberFormat::Double => quote! { f64 },
      },
      VariantOrUnknownOrEmpty::Unknown(number_format) => number_format
        .parse::<TokenStream>()
        .unwrap_or_else(|err| panic!("invalid number type {number_format:#?}: {err}")),
      VariantOrUnknownOrEmpty::Empty => quote! { f64 },
    }
  }

  fn inline_string(&self, string: &StringType) -> TokenStream {
    let StringType {
      format,
      enumeration,
      ..
    } = string;

    if !enumeration.is_empty() {
      panic!("unexpected inline enum must use a reference to a named schema {string:#?}");
    }

    match format {
      VariantOrUnknownOrEmpty::Item(string_format) => match string_format {
        StringFormat::Date => quote! { chrono::NaiveDate },
        StringFormat::DateTime => quote! { chrono::DateTime<chrono::Utc> },
        // `byte` represents a base64-encoded file. We just pass it as a string and let the user
        // base64-decode it for now.
        StringFormat::Byte | StringFormat::Password => quote! { String },
        StringFormat::Binary => quote! { Vec<u8> },
      },
      VariantOrUnknownOrEmpty::Unknown(string_format) => string_format
        .parse::<TokenStream>()
        .unwrap_or_else(|err| panic!("unsupported string type {string_format:#?}: {err}")),
      VariantOrUnknownOrEmpty::Empty => quote! { String },
    }
  }

  fn serde_crate_attr(&self) -> TokenStream {
    let serde_import = format!("{}::__private::serde", self.crate_use_name());
    quote! { crate = #serde_import }
  }
}
