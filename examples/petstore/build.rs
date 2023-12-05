use env_logger::Env;
use openapi_lambda_codegen::{ApiLambda, CodeGenerator, LambdaArn};

fn main() {
  env_logger::init_from_env(Env::default().filter_or("RUST_LOG", "info"));

  // If using an OpenAPI spec that contains references to other files, be sure to edit the
  // `rerun_glob` (second argument) below so that updates trigger the codegen build script.
  CodeGenerator::new("openapi.yaml", ".openapi-lambda")
    // Divide the API into 3 Lambda functions based on the tag of each endpoint.
    .add_api_lambda(
      ApiLambda::new("pet", LambdaArn::cloud_formation("PetApiFunction.Alias"))
        .with_op_filter(|op| op.tags.iter().any(|tag| tag == "pet")),
    )
    .add_api_lambda(
      ApiLambda::new(
        "store",
        LambdaArn::cloud_formation("StoreApiFunction.Alias"),
      )
      .with_op_filter(|op| op.tags.iter().any(|tag| tag == "store")),
    )
    .add_api_lambda(
      ApiLambda::new("user", LambdaArn::cloud_formation("UserApiFunction.Alias"))
        .with_op_filter(|op| op.tags.iter().any(|tag| tag == "user")),
    )
    .generate();
}
