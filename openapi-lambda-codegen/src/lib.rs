#![allow(clippy::too_many_arguments)]
#![warn(missing_docs)]

//! Code generator for [`openapi-lambda`](https://docs.rs/openapi-lambda) crate.
//!
//! Please refer to the [`openapi-lambda`](https://docs.rs/openapi-lambda) crate documentation for
//! usage information.

use crate::api::operation::collect_operations;

use indexmap::IndexMap;
use itertools::Itertools;
use openapiv3::{OpenAPI, Operation};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde_json::json;
use syn::parse2;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

mod api;
mod apigw;
mod inline;
mod model;
mod reference;

// Re-export since `Operation` is part of the public API (for filters), and that includes references
// to other `openapiv3` types.
pub use openapiv3;

/// Cache of parsed OpenAPI documents.
type DocCache = HashMap<PathBuf, serde_yaml::Mapping>;

#[derive(Debug)]
enum LambdaArnImpl {
  /// Use a `!Sub` AWS CloudFormation intrinsic to resolve the Lambda ARN at deploy time.
  ///
  /// This should be used only if the OpenAPI spec will be embedded in the `DefinitionBody` of an
  /// `AWS::Serverless::Api` resource or the `Body` of an `AWS::ApiGateway::RestApi` resource. Note
  /// that in both cases, an `Fn::Transform` intrinsic with the `AWS::Include` transform is needed
  /// to resolve `!Sub` intrinsics in the OpenAPI template. Otherwise, the template will be deployed
  /// verbatim without substituting the Lambda ARN, which the API Gateway service will reject.
  CloudFormation {
    /// Logical ID of the Lambda function within the CloudFormation/SAM template (e.g.,
    /// `PetstoreFunction.Arn`, `PetstoreFunction.Alias`, or `PetstoreFunctionAliasLive`).
    ///
    /// This logical ID is used for resolving the Lambda function's ARN at deploy time using
    /// CloudFormation intrinsics (e.g., `Sub`). This way, the generated OpenAPI spec passed to
    /// the `AWS::Serverless::Api` resource's `DefinitionBody` can be generic and support multiple
    /// deployments in distinct environments.
    logical_id: String,
  },
  /// Use a known ARN that can be provided directly to API Gateway or an infrastructure-as-code
  /// (IaC) solution other than AWS CloudFormation/SAM.
  Known {
    api_gateway_region: String,
    account_id: String,
    function_region: String,
    function_name: String,
    alias_or_version: Option<String>,
  },
}

impl LambdaArnImpl {
  pub fn apigw_invocation_arn(&self) -> serde_json::Value {
    match self {
      LambdaArnImpl::CloudFormation { logical_id } => {
        json!({
          "Fn::Sub": format!(
            "arn:aws:apigateway:${{AWS::Region}}:lambda:path/2015-03-31/functions/${{{logical_id}}}\
             /invocations",
          )
        })
      }
      LambdaArnImpl::Known {
        api_gateway_region,
        account_id,
        function_region,
        function_name,
        alias_or_version,
      } => serde_json::Value::String(format!(
        "arn:aws:apigateway:{api_gateway_region}:lambda:path/2015-03-31/functions/arn:aws\
         :lambda:{function_region}:{account_id}:function:{function_name}{}/invocations",
        alias_or_version
          .as_ref()
          .map(|alias| Cow::Owned(format!(":{alias}")))
          .unwrap_or(Cow::Borrowed(""))
      )),
    }
  }
}

/// Amazon Resource Name (ARN) for an AWS Lambda function.
///
/// This type represents an ARN either using variables (e.g., an AWS CloudFormation logical ID
/// constructed via the [`cloud_formation`](LambdaArn::cloud_formation) method) or as a
/// fully-resolved ARN via the [`known`](LambdaArn::known) method. It is used to populate the
/// [`x-amazon-apigateway-integration`](https://docs.aws.amazon.com/apigateway/latest/developerguide/api-gateway-swagger-extensions-integration.html)
/// OpenAPI extensions that Amazon API Gateway uses to determine which Lambda function should handle
/// each API endpoint.
#[derive(Debug)]
pub struct LambdaArn(LambdaArnImpl);

impl LambdaArn {
  /// Construct a variable ARN that references an AWS CloudFormation or Serverless Application Model
  /// (SAM) logical ID.
  ///
  /// The logical ID should reference one of the following resource types defined in your
  /// CloudFormation/SAM template:
  ///  * [`AWS::Serverless::Function`](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-resource-function.html)
  ///  * [`AWS::Lambda::Function`](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/aws-resource-lambda-function.html)
  ///  * [`AWS::Lambda::Alias`](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/aws-resource-lambda-alias.html)
  ///    (e.g., by appending `.Alias` to the logical ID when specifying an
  ///    [`AutoPublishAlias`](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-resource-function.html#sam-function-autopublishalias)
  ///    on the `AWS::Serverless::Function` resource)
  ///  * [`AWS::Lambda::Version`](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/aws-resource-lambda-version.html)
  ///    (e.g., by appending `.Version` to the logical ID when specifying an
  ///    [`AutoPublishAlias`](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-resource-function.html#sam-function-autopublishalias)
  ///    on the `AWS::Serverless::Function` resource)
  ///
  /// When using this method, be sure to include the `openapi-apigw.yaml` file in your
  /// CloudFormation/SAM template with the `AWS::Include` transform. Otherwise, the variables will
  /// not be substituted during deployment, and deployment will fail. For example (where
  /// `.openapi-lambda` is the `out_dir` passed to [`CodeGenerator::new`]):
  /// ```yaml
  /// Resources:
  ///   MyApi:
  ///     Type: AWS::Serverless::Api
  ///     Properties:
  ///       Name: my-api
  ///       StageName: prod
  ///       DefinitionBody:
  ///         Fn::Transform:
  ///           Name: AWS::Include
  ///           Parameters:
  ///             Location: .openapi-lambda/openapi-apigw.yaml
  /// ```
  ///
  /// # Example
  ///
  /// ```rust
  /// # use openapi_lambda_codegen::LambdaArn;
  /// # let _ =
  /// LambdaArn::cloud_formation("MyApiFunction.Alias")
  /// # ;
  /// ```
  pub fn cloud_formation<L>(logical_id: L) -> Self
  where
    L: Into<String>,
  {
    Self(LambdaArnImpl::CloudFormation {
      logical_id: logical_id.into(),
    })
  }

  /// Construct a fully-resolved AWS Lambda function ARN.
  ///
  /// The resulting ARN does not depend on any CloudFormation variables and is compatible with any
  /// deployment method.
  ///
  /// # Arguments
  ///
  /// * `api_gateway_region` - Region containing the Amazon API Gateway (e.g., `us-east-1`)
  /// * `account_id` - AWS account containing the AWS Lambda function
  /// * `function_region` - Region containing the AWS Lambda function (e.g., `us-east-1`)
  /// * `function_name` - Name of the AWS Lambda function
  /// * `alias_or_version` - Optional Lambda function
  ///   [version](https://docs.aws.amazon.com/lambda/latest/dg/configuration-versions.html) or
  ///   [alias](https://docs.aws.amazon.com/lambda/latest/dg/configuration-aliases.html)
  ///
  /// # Example
  ///
  /// ```rust
  /// # use openapi_lambda_codegen::LambdaArn;
  /// # let _ =
  /// LambdaArn::known(
  ///   "us-east-1",
  ///   "1234567890",
  ///   "us-east-1",
  ///   "my-api-function",
  ///   Some("live".to_string()),
  /// )
  /// # ;
  /// ```
  pub fn known<A, F, G, R>(
    api_gateway_region: G,
    account_id: A,
    function_region: R,
    function_name: F,
    alias_or_version: Option<String>,
  ) -> Self
  where
    A: Into<String>,
    F: Into<String>,
    G: Into<String>,
    R: Into<String>,
  {
    Self(LambdaArnImpl::Known {
      api_gateway_region: api_gateway_region.into(),
      account_id: account_id.into(),
      function_region: function_region.into(),
      function_name: function_name.into(),
      alias_or_version,
    })
  }
}

type OpFilter = Box<dyn Fn(&Operation) -> bool + 'static>;

/// Builder for generating code for a single API Lambda function.
///
/// An `ApiLambda` instance represents a collection of API endpoints handled by a single
/// Lambda function. This could include all endpoints defined in an OpenAPI spec (i.e., a
/// "mono-Lambda"), a single API endpoint, or a subset of the API. Larger Lambda binaries incur a
/// greater
/// [cold start](https://docs.aws.amazon.com/lambda/latest/operatorguide/execution-environments.html#cold-start-latency)
/// cost than smaller binaries, so the granularity of API Lambda functions presents a tradeoff
/// between performance and implementation/deployment complexity (i.e., more Lambda functions to
/// manage).
///
/// Use the [`with_op_filter`](ApiLambda::with_op_filter) method to specify a closure that
/// associates API endpoints with the corresponding Lambda function.
///
/// # Example
///
/// ```rust
/// # use openapi_lambda_codegen::{ApiLambda, LambdaArn};
/// # let _ =
/// ApiLambda::new("backend", LambdaArn::cloud_formation("BackendApiFunction.Alias"))
/// # ;
/// ```
pub struct ApiLambda {
  mod_name: String,
  lambda_arn: LambdaArnImpl,
  op_filter: Option<OpFilter>,
}

impl ApiLambda {
  /// Construct a new `ApiLambda`.
  ///
  /// # Arguments
  ///
  /// * `mod_name` - Name of the Rust module to generate (must be a valid Rust identifier)
  /// * `lambda_arn` - Amazon Resource Name (ARN) of the AWS Lambda function that will handle
  ///   requests to the corresponding API endpoints via Amazon API Gateway (see [`LambdaArn`])
  pub fn new<M>(mod_name: M, lambda_arn: LambdaArn) -> Self
  where
    M: Into<String>,
  {
    Self {
      lambda_arn: lambda_arn.0,
      mod_name: mod_name.into(),
      op_filter: None,
    }
  }

  /// Define a filter to associate a subset of API endpoints with this Lambda function.
  ///
  /// Use this method when *not* implementing a "mono-Lambda" that handles all API endpoints. By
  /// default, all API endpoints will be included unless this method is called.
  ///
  /// # Arguments
  ///
  /// * `op_filter` - Closure that returns `true` or `false` to indicate whether the given OpenAPI
  ///   [`Operation`] (endpoint) will be handled by the corresponding Lambda function
  ///
  /// # Example
  ///
  /// ```rust
  /// # use openapi_lambda_codegen::{ApiLambda, LambdaArn};
  /// # let _ =
  /// ApiLambda::new("backend", LambdaArn::cloud_formation("BackendApiFunction.Alias"))
  ///   // Only include API endpoints with the `pet` tag.
  ///   .with_op_filter(|op| op.tags.iter().any(|tag| tag == "pet"))
  /// # ;
  /// ```
  pub fn with_op_filter<F>(mut self, op_filter: F) -> Self
  where
    F: Fn(&Operation) -> bool + 'static,
  {
    self.op_filter = Some(Box::new(op_filter));
    self
  }
}

/// OpenAPI Lambda code generator.
///
/// This code generator is intended to be called from a `build.rs` Rust
/// [build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html). It emits an
/// `out.rs` file to the directory referenced by the `OUT_DIR` environment variable set by Cargo.
/// This file defines a module named `models` containing Rust types for the input parameters and
/// request/response bodies defined in the OpenAPI definition. It also defines one
/// module for each call to [`add_api_lambda`](CodeGenerator::add_api_lambda), which defines an
/// `Api` trait with one method for each operation (path + HTTP method) defined in the OpenAPI
/// definition.
///
/// In addition, the generator writes the following files to the `out_dir` directory specified in
/// the call to [`new`](CodeGenerator::new):
///  * `openapi-apigw.yaml` - OpenAPI definition annotated with
///    [`x-amazon-apigateway-integration`](https://docs.aws.amazon.com/apigateway/latest/developerguide/api-gateway-swagger-extensions-integration.html)
///    extensions to be used by Amazon API Gateway. This file is also modified from the input
///    OpenAPI definition to help adhere to the
///    [subset of OpenAPI features](https://docs.aws.amazon.com/apigateway/latest/developerguide/api-gateway-known-issues.html#api-gateway-known-issues-rest-apis)
///    supported by Amazon API Gateway. In particular, all references are merged into a single file,
///    and `discriminator` properties are removed.
///  * One file for each call to [`add_api_lambda`](CodeGenerator::add_api_lambda) named
///    `<MODULE_NAME>_handler.rs`, where `<MODULE_NAME>` is the `mod_name` in the [`ApiLambda`]
///    passed to `add_api_lambda`. This file contains a placeholder implementation of the
///    corresponding `Api` trait. To get started, copy this file into `src/`, define a corresponding
///    module (`<MODULE_NAME>_handler`) in `src/lib.rs`, and replace each instance of `todo!()` in
///    the trait implementation.
///
/// # Examples
///
/// ## Mono-Lambda
///
/// The following invocation in `build.rs` uses a single Lambda function to handle all API endpoints:
/// ```rust,no_run
/// # use openapi_lambda_codegen::{ApiLambda, CodeGenerator, LambdaArn};
/// CodeGenerator::new("openapi.yaml", ".openapi-lambda")
///   .add_api_lambda(
///     ApiLambda::new("backend", LambdaArn::cloud_formation("BackendApiFunction.Alias"))
///   )
///   .generate();
/// ```
///
/// ## Multiple Lambda functions
///
/// The following invocation in `build.rs` uses multiple Lambda functions, each handling a subset of
/// API endpoints:
/// ```rust,no_run
/// # use openapi_lambda_codegen::{ApiLambda, CodeGenerator, LambdaArn};
/// CodeGenerator::new("openapi.yaml", ".openapi-lambda")
///   .add_api_lambda(
///     ApiLambda::new("pet", LambdaArn::cloud_formation("PetApiFunction.Alias"))
///     // Only include API endpoints with the `pet` tag.
///     .with_op_filter(|op| op.tags.iter().any(|tag| tag == "pet"))
///   )
///   .add_api_lambda(
///     ApiLambda::new("store", LambdaArn::cloud_formation("StoreApiFunction.Alias"))
///     // Only include API endpoints with the `store` tag.
///     .with_op_filter(|op| op.tags.iter().any(|tag| tag == "store"))
///   )
///   .generate();
/// ```
pub struct CodeGenerator {
  api_lambdas: IndexMap<String, ApiLambda>,
  openapi_path: PathBuf,
  out_dir: PathBuf,
}

impl CodeGenerator {
  /// Construct a new `CodeGenerator`.
  ///
  /// # Arguments
  ///
  /// * `openapi_path` - Input path to OpenAPI definition in YAML format
  /// * `out_dir` - Output directory path in which `openapi-apigw.yaml` and one
  ///   `<MODULE_NAME>_handler.rs` file for each call to
  ///    [`add_api_lambda`](CodeGenerator::add_api_lambda) will be written
  pub fn new<P, O>(openapi_path: P, out_dir: O) -> Self
  where
    P: Into<PathBuf>,
    O: Into<PathBuf>,
  {
    Self {
      api_lambdas: IndexMap::new(),
      openapi_path: openapi_path.into(),
      out_dir: out_dir.into(),
    }
  }

  /// Register an API Lambda function for code generation.
  ///
  /// Each call to this method will result in a module being generated that contains an `Api` trait
  /// with methods for the corresponding API endpoints. See [`ApiLambda`] for further details.
  pub fn add_api_lambda(mut self, builder: ApiLambda) -> Self {
    if self.api_lambdas.contains_key(&builder.mod_name) {
      panic!(
        "API Lambda module names must be unique: found duplicate `{}`",
        builder.mod_name
      )
    }

    self.api_lambdas.insert(builder.mod_name.clone(), builder);
    self
  }

  /// Emit generated code.
  pub fn generate(self) {
    let cargo_out_dir = std::env::var("OUT_DIR").expect("OUT_DIR env not set");
    log::info!("writing Rust codegen to {cargo_out_dir}");
    log::info!("writing OpenAPI codegen to {}", self.out_dir.display());

    if !self.out_dir.exists() {
      std::fs::create_dir_all(&self.out_dir).unwrap_or_else(|err| {
        panic!(
          "failed to create directory `{}`: {err}",
          self.out_dir.display()
        )
      });
    }

    let openapi_file = File::open(&self.openapi_path)
      .unwrap_or_else(|err| panic!("failed to open {}: {err}", self.openapi_path.display()));

    let openapi_yaml: serde_yaml::Mapping =
      serde_path_to_error::deserialize(serde_yaml::Deserializer::from_reader(&openapi_file))
        .unwrap_or_else(|err| panic!("Failed to parse OpenAPI spec as YAML: {err}"));

    let mut cached_external_docs = DocCache::new();

    // Clippy in 1.70.0 raises a false positive here.
    #[allow(clippy::redundant_clone)]
    cached_external_docs.insert(self.openapi_path.to_path_buf(), openapi_yaml.clone());

    println!("cargo:rerun-if-changed={}", self.openapi_path.display());

    let openapi: OpenAPI =
      serde_path_to_error::deserialize(serde_yaml::Value::Mapping(openapi_yaml))
        .unwrap_or_else(|err| panic!("Failed to parse OpenAPI spec: {err}"));

    let crate_import = self.crate_use_name();

    // Merge any references to other OpenAPI files into the root OpenAPI definition, and replace
    // any unnamed schemas that require named models to represent in Rust (e.g., enums) with named
    // schemas in components.schemas. This simplifies the rest of the code generation process since
    // we don't have to visit other files or worry about conflicting schema names.
    let (openapi_inline, models) =
      self.generate_models(self.inline_openapi(openapi, cached_external_docs));

    let openapi_inline_mapping =
      serde_path_to_error::serialize(&*openapi_inline, serde_yaml::value::Serializer)
        .expect("failed to serialize OpenAPI spec");
    let serde_yaml::Value::Mapping(openapi_inline_mapping) = openapi_inline_mapping else {
      panic!("OpenAPI spec should be a mapping: {:#?}", &*openapi_inline);
    };

    let operations = collect_operations(&openapi_inline, &openapi_inline_mapping);
    let operations_by_api_lambda = self
      .api_lambdas
      .values()
      .flat_map(|api_lambda| {
        operations
          .iter()
          .filter(|op| {
            api_lambda
              .op_filter
              .as_ref()
              .map(|op_filter| (*op_filter)(&op.op))
              .unwrap_or(true)
          })
          .map(|op| (&api_lambda.mod_name, op))
      })
      .into_group_map();

    operations_by_api_lambda
      .iter()
      .flat_map(|(mod_name, ops)| {
        ops
          .iter()
          .map(|op| ((&op.method, &op.request_path), *mod_name))
      })
      .into_group_map()
      .into_iter()
      .for_each(|((method, request_path), mod_names)| {
        if mod_names.len() > 1 {
          panic!(
            "endpoint {method} {request_path} is mapped to multiple API Lambdas: {mod_names:?}"
          );
        }
      });

    let operation_id_to_api_lambda = operations_by_api_lambda
      .iter()
      .flat_map(|(mod_name, ops)| {
        ops.iter().map(|op| {
          (
            op.op
              .operation_id
              .as_ref()
              .unwrap_or_else(|| panic!("no operation_id for {} {}", op.method, op.request_path))
              .as_str(),
            self
              .api_lambdas
              .get(*mod_name)
              .expect("mod name should exist in api_lambdas"),
          )
        })
      })
      .collect::<HashMap<_, _>>();

    let components_schemas = openapi_inline
      .components
      .as_ref()
      .map(|components| Cow::Borrowed(&components.schemas))
      .unwrap_or_else(|| Cow::Owned(IndexMap::new()));
    let apis_out = operations_by_api_lambda
      .iter()
      .sorted_by_key(|(mod_name, _)| **mod_name)
      .map(|(mod_name, ops)| {
        self.gen_api_module(
          mod_name,
          ops,
          &openapi_inline_mapping,
          &components_schemas,
          &models,
        )
      })
      .collect::<TokenStream>();

    self.gen_openapi_apigw(openapi_inline, &operation_id_to_api_lambda);

    let models_out = models
      .into_iter()
      .sorted_by(|(ident_a, _), (ident_b, _)| ident_a.cmp(ident_b))
      .map(|(_, model)| model)
      .collect::<TokenStream>();

    let out_rs_path = Path::new(&cargo_out_dir).join("out.rs");
    let out_tok = quote! {
      pub mod models {
        #![allow(unused_imports)]
        #![allow(clippy::large_enum_variant)]

        use #crate_import::__private::anyhow::{self, anyhow};
        use #crate_import::__private::serde::{Deserialize, Serialize};
        use #crate_import::models::chrono;

        #models_out
      }

      #apis_out
    };
    File::create(&out_rs_path)
      .unwrap_or_else(|err| panic!("failed to create {}: {err}", out_rs_path.to_string_lossy()))
      .write_all(
        prettyplease::unparse(
          &parse2(out_tok.clone())
            .unwrap_or_else(|err| panic!("failed to parse generated code: {err}\n{out_tok}")),
        )
        .as_bytes(),
      )
      .unwrap_or_else(|err| {
        panic!(
          "failed to write to {}: {err}",
          out_rs_path.to_string_lossy()
        )
      });
  }

  /// Name of this crate to use for `use` imports.
  fn crate_use_name(&self) -> Ident {
    // TODO: support import customization similar to serde's `crate` attribute:
    // https://serde.rs/container-attrs.html#crate. This also requires a custom model.mustache
    // since that file embeds the #[serde(crate = "...")] attributes.
    Ident::new("openapi_lambda", Span::call_site())
  }

  fn rustfmt(&self, path: &Path) {
    let rustfmt_result = Command::new("rustfmt")
      .args(["--edition".as_ref(), "2021".as_ref(), path.as_os_str()])
      .output()
      .unwrap_or_else(|err| panic!("failed to run rustfmt: {err}"));

    if !rustfmt_result.status.success() {
      panic!(
        "rustfmt failed with status {}:\n{}",
        rustfmt_result.status,
        String::from_utf8_lossy(rustfmt_result.stdout.as_slice())
          + String::from_utf8_lossy(rustfmt_result.stderr.as_slice())
      );
    }
  }
}

fn description_to_doc_attr<S>(description: &S) -> TokenStream
where
  S: AsRef<str>,
{
  description
    .as_ref()
    .lines()
    .map(|line| {
      quote! {
        #[doc = #line]
      }
    })
    .collect()
}
