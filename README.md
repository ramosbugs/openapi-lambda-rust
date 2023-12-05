# OpenAPI Lambda for Rust 🦀

[![crates.io](https://img.shields.io/crates/v/openapi-lambda.svg)](https://crates.io/crates/openapi-lambda)
[![docs.rs](https://docs.rs/openapi-lambda/badge.svg)](https://docs.rs/openapi-lambda)

OpenAPI Lambda for Rust takes an [OpenAPI definition](https://swagger.io/docs/specification)
and generates Rust boilerplate code for running
the API "serverlessly" on [AWS Lambda](https://aws.amazon.com/lambda/) behind an
[Amazon API Gateway](https://aws.amazon.com/api-gateway/) REST API. The generated code automatically routes
requests, parses parameters, marshals responses, invokes middleware to authenticate requests, and
handles related errors. This project's goal is to enable developers to focus on business logic, not
boilerplate.

**This project is not affiliated with the OpenAPI Initiative or Amazon Web Services (AWS).**

## Usage

### 1. Add dependencies

Add `openapi-lambda` as a dependency and `openapi-lambda-codegen` as a build dependency to your
crate's `Cargo.toml`:
 ```toml
 [dependencies]
 openapi-lambda = "0.1"
 
 [build-dependencies]
 openapi-lambda-codegen = "0.1"
 ```
Both crates must have identical version numbers in `Cargo.lock`.

### 2. Generate code
Add a `build.rs` Rust [build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html) to your crate's  root directory (see comments below):
```rust,no_run
use openapi_lambda_codegen::{ApiLambda, CodeGenerator, LambdaArn};

fn main() {
  CodeGenerator::new(
    // Path to OpenAPI definition (relative to build.rs).
    "openapi.yaml",
    // Output path to a directory for generating artifacts. This directory should be added to
    // `.gitignore`.
    ".openapi-lambda",
  )
  // Define one or more Lambda functions for implementing the API. A single "mono-Lambda" may
  // be used to handle all API endpoints, or endpoints may be grouped into multiple Lambda
  // functions using filters (see docs). Note that Lambda cold start time is roughly
  // proportional to the size of each Lambda binary, so consider splitting APIs into smaller
  // Lambda functions to reduce cold start times.
  .add_api_lambda(ApiLambda::new(
    // Name of the generated Rust module that will contain the API types.
    "backend",
    // AWS CloudFormation logical ID or Amazon Resource Name (ARN) that the Lambda function
    // will have when deployed to AWS. This value is used for adding
    // `x-amazon-apigateway-integration` extensions to the OpenAPI definition, which tells
    // API Gateway which Lambda function to use for handling each API request. If using
    // CloudFormation/SAM with a logical ID, the ARN will be populated automatically during
    // deployment.
    LambdaArn::cloud_formation("BackendApiFunction.Alias")
  ))
  .generate();
}
```

Include the generated code in your crate's `src/lib.rs`:
```rust,ignore
include!(concat!(env!("OUT_DIR"), "/out.rs"));
```
The generated file `out.rs` defines a module named `models` containing Rust types for the input
parameters and request/response bodies defined in the OpenAPI definition. It also defines one
module for each call to `add_api_lambda()`, which defines an `Api` trait with one
method for each operation (path + HTTP method) defined in the OpenAPI definition.

#### Generate documentation

It is often helpful to refer to 
[rustdoc](https://doc.rust-lang.org/rustdoc/what-is-rustdoc.html)
documentation to understand the generated models and API types. To generate documentation, run:
```shell
cargo doc --open
```

### 3. Implement API handlers

To implement the API, implement the generated `Api` trait(s). To help you get started,
the code generator creates files named `<MODULE_NAME>_handler.rs`
in the configured output directory (e.g., `.openapi-lambda/backend_handler.rs`) with a placeholder
implementation of each `Api` trait. Copy these files into `src/`, define corresponding modules in
`src/lib.rs` (e.g., `mod backend_handler`),
and replace each `todo!()` to implement the API.

Each `Api` trait declares two associated types that you must define in your implementation:
 * `AuthOk`: the outcome of successful request authentication returned by your middleware (see
   below). This might represent a user, authentication session, or other abstraction relevant to
   your API. If none of the API endpoints require authentication, simply use the unit type (`()`).
 * `HandlerError`: the error type returned by each API handler method. A typical API
   will define an `enum` type for errors and have the `Api::respond_to_handler_error()` method
   return appropriate HTTP responses depending on the nature of the error (e.g., status code 403 for
   access denied errors).

### 4. Implement middleware

The `openapi_lambda::Middleware` trait defines the interface for authenticating requests and
optionally wrapping each API handler to add functionality such as logging and telemetry. 
A convenience
`UnauthenticatedMiddleware` implementation is provided for APIs with no endpoints
that require authentication.

#### Authenticating requests

The `Middleware::AuthOk` associated type represents the outcome of a successful call to the
`Middleware::authenticate()` trait method. This is a type you define that might represent a user,
authentication session, or
other abstraction relevant to your API. If none of the API endpoints require authentication, simply
use the unit type (`()`). The `Middleware::AuthOk` associated type must match the `Api::AuthOk`
associated type in your `Api` trait implementation(s).

The `Middleware::authenticate()` method provides a `headers` argument with access to all request
headers, allowing you to authenticate requests using headers such as
[`Authorization`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization) or
[`Cookie`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Cookie).
It also provides a `lambda_context` argument with access to Amazon Cognito identity information
if using an API Gateway
[Cognito user pool authorizer](https://docs.aws.amazon.com/apigateway/latest/developerguide/apigateway-integrate-with-cognito.html).

If the request fails
to authenticate, be sure to return an `HttpResponse` with the appropriate HTTP status code
(i.e., 401).

### 5. Add binary target(s)

Define a binary target for each Lambda function (e.g., `bin/bootstrap_backend.rs`) to bootstrap the
Lambda runtime. The `openapi_lambda::run_lambda()` function is the recommended entry point to start
the Lambda runtime and begin handling API requests:

```rust,ignore
// Replace `my_api` with the name of your crate and `backend` with the name of the module
// passed to `ApiLambda::new()`.
use my_api::backend::Api;
use my_api::backend_handler::BackendApiHandler;
use openapi_lambda::run_lambda;

#[tokio::main]
pub async fn main() {
  let api = BackendApiHandler::new(...);
  let middleware = ...; // Instantiate your middleware here.

  run_lambda(|event| api.dispatch_request(event, &middleware)).await
}
```

### 6. Compile binaries

#### Cargo Lambda

The easiest way to compile Lambda functions written in Rust is with
[Cargo Lambda](https://www.cargo-lambda.info/), which handles any necessary cross-compilation from
your development environment to AWS Lambda (either x86-64 or ARM-based).

In addition to installing Cargo Lambda, be sure to install the relevant target
(`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu` depending on the targeted Lambda
function architecture) for your Rust toolchain (e.g., via
`rustup target add`).

After installing Cargo
Lambda, run the following command to build Lambda `bootstrap` binaries in the `target/lambda/`
directory:
```shell
cargo lambda build --release
```
If targeting ARM-based Lambda functions, be sure to add the `--arm64` flag.

#### `musl-cross`

An alternative to Cargo Lambda is [`musl-cross`](https://github.com/richfelker/musl-cross-make),
which provides [better backtrace support](https://github.com/ziglang/zig/issues/18280) when
compiling on certain environments such as macOS with Apple Silicon. A
[Homebrew package](https://github.com/FiloSottile/homebrew-musl-cross) is available for easy
installation on macOS.

In addition to installing `musl-cross`, be sure to install the relevant target
(`x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl` depending on the targeted Lambda
function architecture) for your Rust toolchain (e.g., via
`rustup target add`).

To compile binaries for x86-64 Lambda functions, run:
```shell
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc \
  cargo build --target x86_64-unknown-linux-musl --release
```

To compile binaries for ARM Lambda functions, run:
```shell
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc \
  cargo build --target aarch64-unknown-linux-musl --release
```

The final binaries are written to the `target/x86_64-unknown-linux-musl/release/` or
`target/aarch64-unknown-linux-musl/release/` directory, depending on the target architecture.

### 7. Test and deploy

Deploying to AWS involves creating one or more
[Lambda functions](https://docs.aws.amazon.com/lambda/latest/dg/getting-started.html) and an
[API Gateway REST API](https://docs.aws.amazon.com/apigateway/latest/developerguide/apigateway-rest-api.html).

Lambda functions written in Rust should use one of the `provided`
[Lambda runtimes](https://docs.aws.amazon.com/lambda/latest/dg/lambda-runtimes.html).
The `provided` runtimes require each Lambda function to include a binary named `bootstrap`, which 
is produced by the compilation step above.

An API Gateway REST API uses an OpenAPI definition annotated with
[`x-amazon-apigateway-integration`](https://docs.aws.amazon.com/apigateway/latest/developerguide/api-gateway-swagger-extensions-integration.html)
extensions that determine which Lambda function is used for
handling each API endpoint. The `openapi-lambda-codegen` crate writes an annotated
OpenAPI definition suitable for this purpose to a file named `openapi-apigw.yaml` in the output
directory specified in `build.rs` (e.g., `.openapi-lambda/openapi-apigw.yaml`). This OpenAPI
definition is modified from the input to help adhere to the
[subset of OpenAPI features](https://docs.aws.amazon.com/apigateway/latest/developerguide/api-gateway-known-issues.html#api-gateway-known-issues-rest-apis)
supported by Amazon API Gateway. In particular, all references are merged into a single file, and
`discriminator` properties are removed.

As a best practice, consider using an infrastructure-as-code (IaC) solution such as
[AWS CloudFormation](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide),
[AWS Serverless Application Model](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/index.html)
(SAM), or [Terraform](https://www.terraform.io/).

####  AWS Serverless Application Model (SAM)

The
[Petstore](https://github.com/ramosbugs/openapi-lambda-rust/tree/main/examples/petstore) example
provides a working AWS SAM template (`template.yaml`) and accompanying `Makefile`.

AWS SAM provides both a streamlined version of CloudFormation tailored to serverless use cases and
a command-line interface (CLI) for deploying to AWS and locally testing APIs.

When defining a SAM CloudFormation stack template, define an
[`AWS::Serverless::Function`](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-resource-function.html)
resource for each Lambda function. Be sure to specify the same logical ID (i.e., YAML key) in your
`build.rs` Rust build script using the `LambdaArn::cloud_formation()` function. If
specifying an
[`AutoPublishAlias`](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-resource-function.html#sam-function-autopublishalias)
property (recommended), append the `.Alias` suffix to the logical ID passed to
`LambdaArn::cloud_formation()`. This ensures that API Gateway always executes the version of your
function associated with the specified alias. Aliases help support quick rollbacks in production
by simply updating the alias to point to a previous version of the Lambda function, without waiting
for a full stack deploy.

Each `AWS::Serverless::Function` resource should specify
`BuildMethod: makefile` in the `Metadata` attribute (see
[Building custom runtimes](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/building-custom-runtimes.html)). The resource should also specify a `CodeUri` attribute that points
to a directory containing your crate. A `Makefile` must exist in the specified directory. The
`Makefile` must define a target named `build-LOGICAL_ID`, where `LOGICAL_ID` is the logical ID (YAML
key) of  the resource in the SAM template. The `build-LOGICAL_ID` target must copy a binary named
`bootstrap` to the directory referenced by the `ARTIFACTS_DIR` environment variable (set at build
time by the AWS SAM CLI). See the
[Petstore](https://github.com/ramosbugs/openapi-lambda-rust/tree/main/examples/petstore) example
for details.

The SAM template must also include an
[`AWS::Serverless::Api`](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-resource-api.html)
resource that defines the API Gateway REST API. Use the
[`AWS::Include`](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/create-reusable-transform-function-snippets-and-add-to-your-template-with-aws-include-transform.html)
transform along with the annotated OpenAPI definition `openapi-apigw.yaml`, which automatically
resolves the logical IDs of each Lambda function to the corresponding
[Amazon Resource Name](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference-arns.html) (ARN)
during deployment:

```yaml
Resources:
  MyApi:
    Type: AWS::Serverless::Api
    Properties:
      Name: my-api
      StageName: prod
      DefinitionBody:
        Fn::Transform:
          Name: AWS::Include
          Parameters:
            Location: .openapi-lambda/openapi-apigw.yaml
```

Before testing or deploying an AWS SAM template, build it by running:
```shell
sam build
```

To start the API locally for testing, run:
```shell
sam local start-api
```

To deploy the template to AWS, run:
```shell
sam deploy
```

## Example

The [Petstore](https://github.com/ramosbugs/openapi-lambda-rust/tree/main/examples/petstore) example
illustrates how to use this crate together with
[AWS SAM](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/index.html)
to build, test, and deploy an API to AWS Lambda behind an Amazon API Gateway REST API.

## Minimum supported Rust version (MSRV)

The minimum supported Rust version (MSRV) of this crate is **1.70**.

This crate maintains a policy of supporting Rust releases going back at least 6 months. Changes that
break compatibility with Rust releases older than 6 months will not be considered SemVer
breaking changes and will not result in a new major version number for this crate. MSRV changes will
coincide with minor version updates and will not happen in patch releases.

## Logging

The generated code uses the [`log`](https://crates.io/crates/log) crate to log requests. Consider
using the [`log4rs`](https://crates.io/crates/log4rs) or
[`env_logger`](https://crates.io/crates/env_logger) crates to enable logging in each Lambda
function's `main()` entry point.

Enabling `TRACE` level logs will log the raw contents of each request and response. This can be
useful for debugging, but **`TRACE` logs should never be enabled in production**. In addition to
being verbose (incurring
[Amazon CloudWatch Logs](https://docs.aws.amazon.com/AmazonCloudWatch/latest/logs/LogsBillingDetails.html)
charges), enabling `TRACE` logs in production could log sensitive secrets such as passwords and API
keys.

## OpenAPI support

The code generator supports a large portion of the
[OpenAPI 3.0 specification](https://github.com/OAI/OpenAPI-Specification/blob/ecc4e50cf60620c44e1e8f2bee31395f95685e75/versions/3.0.3.md),
but gaps remain. If you encounter an `unimplemented!` error when generating code, please
[submit a GitHub issue](https://github.com/ramosbugs/openapi-lambda-rust/issues/new) or open a
pull request (see
[`CONTRIBUTING.md`](https://github.com/ramosbugs/openapi-lambda-rust/tree/main/CONTRIBUTING.md)).

References (`$ref`) found in OpenAPI definitions are supported, including references to objects in
other files. However, references that resolve to other references are currently not supported.

Every endpoint must have an `operationId` property, which must be unique across all endpoints. The
`operationId` property is used for routing requests and naming the handler method and related types
in the generated code.

### Authenticated vs. unauthenticated API endpoints

By default, all API endpoints are assumed to require authentication. This means that
`Middleware::authenticate()` is invoked, and the `AuthOk` result is passed to the handler
method.

To denote an endpoint as *unauthenticated*, add an empty object (`{}`) to the
[`security`](https://github.com/OAI/OpenAPI-Specification/blob/ecc4e50cf60620c44e1e8f2bee31395f95685e75/versions/3.0.3.md#security-requirement-object)
property for the endpoint. For example:
```yaml
security:
  - {}
```
Unauthenticated endpoints will have their handlers invoked without calling
`Middleware::authenticate()`, and the handler method will not receive an `AuthOk` parameter. 

Note that "unauthenticated" in this context simply means that the middleware will not be used to
authenticate requests. The handler method you implement may still perform its own authentication.
This is often useful for login endpoints (for which no authentication session exists yet), or for
webhook endpoints that require access to the raw request body in order to authenticate the request
(e.g., using an HMAC). In the latter case, a request body schema with `type: string` (optionally
with `format: binary`) should be used. The handler method can deserialize the body after verifying
the HMAC.

### Request parameters

Request parameters must define a single `schema` property. The `content` property is currently not
supported.

Cookie parameters (`in: cookie`) are currently not supported. Header parameters (`in: header`) must
be plain string schemas.

Where supported, non-string parameter types must implement the `FromStr` trait for parsing. Object
types are not supported in request parameters.

### Request/response bodies

Request and response bodies that define more than one media type are currently not supported.

The code generator represents request and response bodies as Rust types according to the following
table. [GitHub issues](https://github.com/ramosbugs/openapi-lambda-rust/issues/new) and
pull requests that add support for other widely-used data formats are encouraged.

| Media type                 | Schema `type` | Rust type                                                    | (De)serialization |
|----------------------------|---------------|--------------------------------------------------------------|-------------------|
| `application/json`         | `string`      | `Vec<u8>` for `format: binary` or `String` (UTF-8) otherwise | None              |
| `application/json`         | Non-`string`  | See below                                                    | `serde_json`      |
| `application/octet-stream` | Any           | `Vec<u8>`                                                    | None              |
| `text/*`                   | Any           | `String` (UTF-8)                                             | None              |
| Others (fallback)          | Any           | `Vec<u8>`                                                    | None              |

#### Strings (`type: string`)

String schemas that specify at least one `enum` variant will result in a named Rust `enum`
being generated. Please note that `null` variants are currently not supported.

Non-`enum` string types are determined by the `format` property, as indicated in the table
below:

| `format`              | Rust type                                                                               |
|-----------------------|-----------------------------------------------------------------------------------------|
| Unspecified (default) | `String`                                                                                |
| `date`                | [`chrono::NaiveDate`](https://docs.rs/chrono/latest/chrono/naive/struct.NaiveDate.html) |
| `date-time`           | [`chrono::DateTime<Utc>`](https://docs.rs/chrono/latest/chrono/struct.DateTime.html)    |
| `byte`                | `String` (without base64 decoding)                                                      |
| `password`            | `String`                                                                                |
| `binary`              | `Vec<u8>`                                                                               |
| Other                 | Treated as a verbatim Rust type                                                         |

#### Integers (`type: integer`)

Integer `enum`s are currently not supported. Non-`enum` integer types are determined by the `format`
property, as indicated in the table below:

| `format`              | Rust type                       |
|-----------------------|---------------------------------|
| Unspecified (default) | `i64`                           |
| `int32`               | `i32`                           |
| `int64`               | `i64`                           |
| Other                 | Treated as a verbatim Rust type |

#### Floating-point numbers (`type: number`)

Number `enum`s are currently not supported. Non-`enum` number types are determined by the `format`
property, as indicated in the table below:

| `format`              | Rust type                       |
|-----------------------|---------------------------------|
| Unspecified (default) | `f64`                           |
| `float`               | `f32`                           |
| `double`              | `f64`                           |
| Other                 | Treated as a verbatim Rust type |

#### Booleans (`type: boolean`)

Boolean `enum`s are currently not supported. Booleans are always represented as `bool`.

#### Objects (`type: object`)

The table below specifies the generated Rust types depending on an object schema's
`properties` and `additionalProperties` fields. Please note that `properties` entries with schemas
that are objects or `enum`s must use references (`$ref`) to named schemas. Other property types
may use inline schemas or references.

| `properties` | `additionalProperties` | Rust type                                                                      |
|--------------|------------------------|--------------------------------------------------------------------------------|
| At least one | `false` or unspecified | Named `struct`                                                                 |
| At least one | `true`                 | Named `struct` + `HashMap<String, serde_json::Value>` with `#[serde(flatten)]` |
| At least one | Schema                 | Named `struct` + `HashMap<String, _>` with `#[serde(flatten)]`                 |
| None         | `false` or unspecified | `openapi_lambda::models::EmptyModel`                                           |
| None         | `true`                 | `HashMap<String, serde_json::Value>`                                           |
| None         | Schema                 | `HashMap<String, _>`                                                           |

#### Arrays (`type: array`)

Array schemas with `uniqueItems: true` are represented as
[`indexmap::IndexSet<_>`](https://docs.rs/indexmap/latest/indexmap/set/struct.IndexSet.html). All
other arrays are represented as `Vec<_>`.

#### Polymorphism (`oneOf`)

A named Rust `enum` is generated for schemas utilizing `oneOf`, with one variant for each
entry contained in the `oneOf` array. If a `discriminator` is
specified, a Serde [internally-tagged](https://serde.rs/enum-representations.html#internally-tagged)
`enum` is generated, with that field as the tag. Otherwise, a Serde
[untagged](https://serde.rs/enum-representations.html#untagged) enum is generated.

Please note that each `oneOf` variant must be a named reference (`$ref`), which determines the name
of the Rust `enum` variant. Each referenced schema must be either an object schema (`type: object`)
or utilize `allOf`. Inline variant schemas are not supported.

#### Composed objects (`allOf`)

Schemas utilizing `allOf` are treated as objects (see above) after merging all of the component
schemas into a single schema of `type: object`. Each component of an `allOf` schema must be an
object or a nested `allOf` schema. At most one component may define `additionalProperties`.

#### Other schema types

Schemas utilizing `anyOf` or `not` are currently not supported.

### Responses

Responses must specify individual HTTP status codes. Status code ranges are currently not supported.

## Sponsorship

This project is sponsored by [Unflakable](https://unflakable.com).
