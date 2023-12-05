use env_logger::Env;
use openapi_lambda_codegen::{ApiLambda, CodeGenerator, LambdaArn};

fn main() {
  env_logger::init_from_env(Env::default().filter_or("RUST_LOG", "info"));

  // If using an OpenAPI spec that contains references to other files, be sure to edit the
  // `rerun_glob` (second argument) below so that updates trigger the codegen build script.
  CodeGenerator::new("spec/openapi.yaml", ".openapi-lambda")
    // Divide the API into 3 Lambda functions based on the tag of each endpoint.
    .add_api_lambda(
      ApiLambda::new("foo", LambdaArn::cloud_formation("FooApiFunction.Alias"))
        .with_op_filter(|op| op.tags.iter().any(|tag| tag == "foo")),
    )
    .add_api_lambda(
      ApiLambda::new("bar", LambdaArn::cloud_formation("BarApiFunction.Alias"))
        .with_op_filter(|op| op.tags.iter().any(|tag| tag == "bar")),
    )
    .generate();
}
