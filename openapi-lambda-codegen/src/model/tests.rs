use crate::CodeGenerator;

use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
use openapi_lambda::error::format_error;
use openapiv3::{Components, OpenAPI, ReferenceOr, Schema};
use pretty_assertions::assert_eq;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::de::DeserializeOwned;
use syn::parse2;

use std::collections::HashMap;

type Schemas = IndexMap<String, ReferenceOr<Schema>>;

#[test]
fn test_components() {
  let components = parse_yaml::<Components>(
    r##"
schemas:
  foo_bar:
    type: object
    properties:
      foo:
        type: string
      bar:
        type: string
    required:
      - bar

  # Don't generate a model for non-enum strings.
  Baz:
    type: string

  Enum:
    type: string
    enum:
      - option_a
      - option_b
    "##,
  );

  let code_generator = mock_code_generator();
  let models = code_generator.generate_components(&components);

  assert_eq!(
    models.keys().sorted().collect::<Vec<_>>(),
    vec!["Enum", "FooBar"]
  );

  expect_token_stream_eq(
    models
      .get(&Ident::new("Enum", Span::call_site()))
      .unwrap()
      .to_owned(),
    quote! {
      #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
      #[serde(crate = "openapi_lambda::__private::serde")]
      pub enum Enum {
        #[serde(rename = "option_a")]
        OptionA,
        #[serde(rename = "option_b")]
        OptionB,
      }
      impl Enum {
        fn as_str(&self) -> &'static str {
          match self {
            Self::OptionA => "option_a",
            Self::OptionB => "option_b",
          }
        }
      }
      impl std::fmt::Display for Enum {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          write!(f, "{}", self.as_str())
        }
      }
      impl std::str::FromStr for Enum {
        type Err = anyhow::Error;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
          match s {
            "option_a" => Ok(Self::OptionA),
            "option_b" => Ok(Self::OptionB),
            _ => Err(anyhow!("invalid enum variant `{}`", s)),
          }
        }
      }
    },
  );

  expect_token_stream_eq(
    models
      .get(&Ident::new("FooBar", Span::call_site()))
      .unwrap()
      .to_owned(),
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(crate = "openapi_lambda::__private::serde")]
      pub struct FooBar {
          #[serde(skip_serializing_if = "Option::is_none")]
          pub foo: Option<String>,
          pub bar: String,
      }
    },
  );
}

#[test]
fn test_object_properties() {
  expect_model(
    r##"
Foo:
  type: object
  description: Description Foo
  properties:
    string_plain:
      description: Description `string_plain`
      type: string
    string_format:
      type: string
      format: foo::bar
    string_date:
      type: string
      format: date
    string_datetime:
      type: string
      format: date-time
    string_byte:
      type: string
      format: byte
    string_password:
      type: string
      format: password
    string_binary:
      type: string
      format: binary
    string_ref:
      $ref: "#/components/schemas/String"
    integer:
      type: integer
    integer_format:
      type: integer
      format: foo::integer
    integer32:
      type: integer
      format: int32
    integer64:
      type: integer
      format: int64
    number:
      type: number
    number_format:
      type: number
      format: foo::number
    number_float:
      type: number
      format: double
    number_double:
      type: number
      format: double
    boolean:
      type: boolean
    array:
      type: array
      items:
        type: string
    array_any:
      type: array
    array_obj_ref_properties:
      type: array
      items:
        $ref: "#/components/schemas/ObjectProperties"
    array_obj_ref_addl_properties:
      type: array
      items:
        $ref: "#/components/schemas/ObjectAdditionalProperties"
    array_unique:
      type: array
      items:
        type: string
      uniqueItems: true
    obj_addl_properties:
      type: object
      additionalProperties:
        type: integer
    obj_empty:
      type: object
    obj_ref_properties:
      $ref: "#/components/schemas/ObjectProperties"
    obj_ref_addl_properties:
      $ref: "#/components/schemas/ObjectAdditionalProperties"
    # Property names should be converted to snake_case.
    SnakeCase:
      type: string
    # Make sure we don't generate identifiers that are Rust keywords.
    type:
      type: string
  required:
    # Random subset of required fields to make sure we're not unconditionally using `Option`
    # everywhere.
    - string_plain
    - integer32
    - number
    - array
    - obj_ref_addl_properties

String:
  type: string

ObjectProperties:
  type: object
  properties:
    bar:
      type: string

ObjectAdditionalProperties:
  type: object
  additionalProperties:
    type: string
    format: foo::Bar
    "##,
    "Foo",
    quote! {
      #[doc = "Description Foo"]
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(crate = "openapi_lambda::__private::serde")]
      pub struct Foo {
        #[doc = "Description `string_plain`"]
        pub string_plain: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub string_format: Option<foo::bar>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub string_date: Option<chrono::NaiveDate>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub string_datetime: Option<chrono::DateTime<chrono::Utc>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub string_byte: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub string_password: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub string_binary: Option<Vec<u8>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub string_ref: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub integer: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub integer_format: Option<foo::integer>,
        #[serde(rename = "integer32")]
        pub integer_32: i32,
        #[serde(rename = "integer64", skip_serializing_if = "Option::is_none")]
        pub integer_64: Option<i64>,
        pub number: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub number_format: Option<foo::number>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub number_float: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub number_double: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub boolean: Option<bool>,
        pub array: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub array_any: Option<Vec<openapi_lambda::models::serde_json::Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub array_obj_ref_properties: Option<Vec<crate::models::ObjectProperties>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub array_obj_ref_addl_properties: Option<
          Vec<std::collections::HashMap<String, foo::Bar>>,
        >,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub array_unique: Option<openapi_lambda::models::IndexSet<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub obj_addl_properties: Option<std::collections::HashMap<String, i64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub obj_empty: Option<openapi_lambda::models::EmptyModel>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub obj_ref_properties: Option<crate::models::ObjectProperties>,
        pub obj_ref_addl_properties: std::collections::HashMap<String, foo::Bar>,
        #[serde(rename = "SnakeCase", skip_serializing_if = "Option::is_none")]
        pub snake_case: Option<String>,
        #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
        pub r#type: Option<String>,
      }
    },
  );
}

#[test]
fn test_object_properties_with_additional() {
  expect_model(
    r##"
Foo:
  type: object
  properties:
    foo:
      type: string
  additionalProperties:
    $ref: "#/components/schemas/Bar"

Bar:
  type: string
  format: foo::bar
    "##,
    "Foo",
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(crate = "openapi_lambda::__private::serde")]
      pub struct Foo {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub foo: Option<String>,
        #[serde(flatten)]
        pub additional_properties: std::collections::HashMap<String, foo::bar>,
      }
    },
  );

  expect_model(
    r##"
Foo:
  type: object
  properties:
    foo:
      type: string
  additionalProperties:
    $ref: "#/components/schemas/Bar"

Bar:
  type: object
  properties:
    bar:
      type: boolean
    "##,
    "Foo",
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(crate = "openapi_lambda::__private::serde")]
      pub struct Foo {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub foo: Option<String>,
        #[serde(flatten)]
        pub additional_properties: std::collections::HashMap<String, crate::models::Bar>,
      }
    },
  );
}

#[test]
fn test_object_additional_properties() {
  expect_no_model(
    r##"
Baz:
  type: object
  additionalProperties:
    type: string
    format: foo::Bar
    "##,
    "Baz",
  );
}

#[test]
fn test_string() {
  expect_no_model(
    r##"
Foo:
  type: string
    "##,
    "Foo",
  );
}

#[test]
fn test_string_enum() {
  expect_model(
    r##"
Foo:
  type: string
  enum:
    - option_a
    - option_b
    "##,
    "Foo",
    quote! {
      #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
      #[serde(crate = "openapi_lambda::__private::serde")]
      pub enum Foo {
        #[serde(rename = "option_a")]
        OptionA,
        #[serde(rename = "option_b")]
        OptionB,
      }
      impl Foo {
        fn as_str(&self) -> &'static str {
          match self {
            Self::OptionA => "option_a",
            Self::OptionB => "option_b",
          }
        }
      }
      impl std::fmt::Display for Foo {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          write!(f, "{}", self.as_str())
        }
      }
      impl std::str::FromStr for Foo {
        type Err = anyhow::Error;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
          match s {
            "option_a" => Ok(Self::OptionA),
            "option_b" => Ok(Self::OptionB),
            _ => Err(anyhow!("invalid enum variant `{}`", s)),
          }
        }
      }
    },
  );
}

#[test]
fn test_array() {
  expect_no_model(
    r##"
Foo:
  type: array
  items:
    type: string
    "##,
    "Foo",
  );
}

#[test]
fn test_integer() {
  expect_no_model(
    r##"
Foo:
  type: integer
    "##,
    "Foo",
  );
}

#[test]
fn test_number() {
  expect_no_model(
    r##"
Foo:
  type: number
    "##,
    "Foo",
  );
}

#[test]
fn test_bool() {
  expect_no_model(
    r##"
Foo:
  type: boolean
    "##,
    "Foo",
  );
}

#[test]
fn test_oneof_discriminator_mapping() {
  // The OpenAPI spec requires the discriminator to be present in each `oneOf` variant, but we relax
  // this requirement and treat the discriminator as a string whether or not it exists in the
  // variants.
  //   "It is important that all the models mentioned below `anyOf` or `oneOf` contain the property
  //    that the discriminator specifies."
  for discriminator_in_variants in [true, false] {
    let (discriminator_property, discriminator_required) = if discriminator_in_variants {
      (
        r##"
    foo:
      type: string"##,
        "- foo",
      )
    } else {
      ("", "")
    };

    expect_model(
      &format!(
        r##"
Foo:
  oneOf:
    - $ref: "#/components/schemas/Bar"
    - $ref: "#/components/schemas/Baz"
  discriminator:
    propertyName: foo
    mapping:
      bar: "#/components/schemas/Bar"
      baz: "#/components/schemas/Baz"

Bar:
  type: object
  properties:
    {discriminator_property}
    bar:
      type: string
  required:
    {discriminator_required}
    - bar

Baz:
  type: object
  properties:
    {discriminator_property}
    baz:
      type: string
  required:
    {discriminator_required}
    - baz
        "##,
      ),
      "Foo",
      quote! {
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(crate = "openapi_lambda::__private::serde", tag = "foo")]
        pub enum Foo {
          #[serde(rename = "bar")]
          // NB: `foo` should not appear here.
          Bar { bar: String },
          #[serde(rename = "baz")]
          Baz { baz: String },
        }
      },
    );
  }
}

#[test]
fn test_oneof_discriminator_no_mapping() {
  for discriminator_in_variants in [true, false] {
    let (discriminator_property, discriminator_required) = if discriminator_in_variants {
      (
        r##"
    foo:
      type: string"##,
        "- foo",
      )
    } else {
      ("", "")
    };

    expect_model(
      &format!(
        r##"
Foo:
  oneOf:
    - $ref: "#/components/schemas/Bar"
    - $ref: "#/components/schemas/Baz"
  discriminator:
    propertyName: foo

Bar:
  type: object
  properties:
    {discriminator_property}
    bar:
      type: string
  required:
    {discriminator_required}
    - bar

Baz:
  type: object
  properties:
    {discriminator_property}
    baz:
      type: string
  required:
    {discriminator_required}
    - baz
        "##,
      ),
      "Foo",
      quote! {
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(crate = "openapi_lambda::__private::serde", tag = "foo")]
        pub enum Foo {
          Bar { bar: String },
          Baz { baz: String },
        }
      },
    );
  }
}

#[test]
fn test_oneof_no_discriminator() {
  expect_model(
    r##"
Foo:
  oneOf:
    - $ref: "#/components/schemas/Bar"
    - $ref: "#/components/schemas/Baz"

Bar:
  type: object
  properties:
    bar:
      type: string
  required:
    - bar

Baz:
  type: object
  properties:
    baz:
      type: string
  required:
    - baz
        "##,
    "Foo",
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(crate = "openapi_lambda::__private::serde", untagged)]
      pub enum Foo {
        Bar { bar: String },
        Baz { baz: String },
      }
    },
  );
}

#[test]
fn test_allof_discriminator() {
  expect_model(
    r##"
Foo:
  allOf:
    - $ref: "#/components/schemas/Bar"
    - $ref: "#/components/schemas/Baz"

Bar:
  type: object
  properties:
    bar:
      type: string
  required:
    - bar

Baz:
  type: object
  properties:
    baz:
      type: string
  required:
    - baz
  additionalProperties: true
        "##,
    "Foo",
    quote! {
      #[derive(Clone, Debug, Deserialize, Serialize)]
      #[serde(crate = "openapi_lambda::__private::serde")]
      pub struct Foo {
        pub bar: String,
        pub baz: String,
        #[serde(flatten)]
        pub additional_properties: std::collections::HashMap<
          String,
          openapi_lambda::models::serde_json::Value,
        >,
      }
    },
  );
}

#[test]
#[should_panic(expected = "dependency cycle detected between models")]
fn test_circular_reference() {
  let components_schemas = parse_yaml::<Schemas>(
    r##"
Foo:
  type: object
  properties:
    bar:
      $ref: "#/components/schemas/Bar"

Bar:
  type: object
  properties:
    foo:
      $ref: "#/components/schemas/Foo"
  "##,
  );
  let code_generator = mock_code_generator();
  let mut models = HashMap::new();
  let model_ident = Ident::new("Foo", Span::call_site());

  code_generator.generate_model(
    model_ident.clone(),
    unwrap_item(components_schemas.get("Foo").unwrap()),
    &components_schemas,
    &mut models,
    &mut IndexSet::new(),
  );
}

#[test]
fn test_unnamed_schemas() {
  let openapi = parse_yaml::<OpenAPI>(
    r##"
openapi: 3.0.2
info:
  title: Test
  version: 1.0
paths:
  /foo:
    get:
      operationId: listFoo
      parameters:
        - name: color
          in: query
          schema:
            type: string
            enum:
              - red
              - green
              - blue

      responses:
        default:
          description: Default response
          content:
            text/plain:
              schema:
                type: string
                enum:
                  - foo
                  - bar
                  - baz
        200:
          description: Success response
          content:
            application/json:
              schema:
                type: object
                properties:
                  foo_id:
                    type: string
components:
  parameters:
    Color:
      name: color
      in: query
      schema:
        type: string
        enum:
          - red
          - green
          - blue
    ColorBw:
      name: color
      in: query
      schema:
        type: string
        enum:
          - black
          - white
  responses:
    Bar:
      description: Bar response
      content:
        text/plain:
          schema:
            type: string
            enum:
              - foo
              - bar
              - baz
  schemas:
    Fruit:
      type: object
      properties:
        type:
          type: string
          enum:
            - berry
            - stonefruit
  "##,
  );

  let code_generator = mock_code_generator();
  let (_, models) =
    code_generator.generate_models(code_generator.inline_openapi(openapi, HashMap::new()));

  assert_eq!(
    models
      .keys()
      .map(|ident| ident.to_string())
      .sorted()
      .collect::<Vec<_>>(),
    [
      "BarResponsePlainTextResponseBody",
      "ColorParam",
      "ColorParam2",
      "Fruit",
      "FruitType",
      "ListFoo200ResponseJsonResponseBody",
      "ListFooColorParam",
      "ListFooDefaultResponsePlainTextResponseBody"
    ]
  );
}

fn parse_yaml<T>(yaml: &str) -> T
where
  T: DeserializeOwned,
{
  serde_path_to_error::deserialize(serde_yaml::Deserializer::from_str(yaml))
    .unwrap_or_else(|err| panic!("{}", format_error(&err, None, None)))
}

fn mock_code_generator() -> CodeGenerator {
  CodeGenerator::new("openapi.yaml", ".openapi-lambda")
}

fn unwrap_item<T>(reference_or: &ReferenceOr<T>) -> &T {
  match reference_or {
    ReferenceOr::Reference { reference } => {
      panic!("should be an item; found reference {reference}")
    }
    ReferenceOr::Item(item) => item,
  }
}

fn expect_token_stream_eq(a: TokenStream, b: TokenStream) {
  assert_eq!(
    prettyplease::unparse(
      &parse2(a.clone()).unwrap_or_else(|err| panic!("failed to parse token stream {a}: {err}"))
    ),
    prettyplease::unparse(
      &parse2(b.clone()).unwrap_or_else(|err| panic!("failed to parse token stream {b}: {err}"))
    )
  );
}

fn expect_model(components_schemas_str: &str, model_name: &str, expected_model: TokenStream) {
  let components_schemas = parse_yaml::<Schemas>(components_schemas_str);
  let code_generator = mock_code_generator();
  let mut models = HashMap::new();
  let model_ident = Ident::new(model_name, Span::call_site());

  assert_eq!(
    code_generator.generate_model(
      model_ident.clone(),
      unwrap_item(components_schemas.get(model_name).unwrap()),
      &components_schemas,
      &mut models,
      &mut IndexSet::new(),
    ),
    true
  );

  expect_token_stream_eq(models.get(&model_ident).unwrap().to_owned(), expected_model);
}

fn expect_no_model(components_schemas_str: &str, model_name: &str) {
  let components_schemas = parse_yaml::<Schemas>(components_schemas_str);
  let code_generator = mock_code_generator();
  let mut models = HashMap::new();
  let model_ident = Ident::new(model_name, Span::call_site());

  assert_eq!(
    code_generator.generate_model(
      model_ident.clone(),
      unwrap_item(components_schemas.get(model_name).unwrap()),
      &components_schemas,
      &mut models,
      &mut IndexSet::new(),
    ),
    false
  );
  assert!(models.is_empty());
}
