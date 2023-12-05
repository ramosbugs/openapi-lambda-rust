#![allow(unused_variables)]

include!(concat!(env!("OUT_DIR"), "/out.rs"));

mod types {
  use std::num::ParseIntError;
  use std::str::FromStr;

  #[derive(Clone, Copy, Debug)]
  pub struct BarId(i64);

  impl FromStr for BarId {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
      i64::from_str(s).map(Self)
    }
  }
}

// Make sure the auto-generated handler templates compile.
#[allow(dead_code)]
#[path = "../.openapi-lambda/foo_handler.rs"]
pub mod foo_handler;

#[path = "../.openapi-lambda/bar_handler.rs"]
#[allow(dead_code)]
pub mod bar_handler;

// TO UPDATE THE OUTPUT SNAPSHOTS BELOW, RUN:
//   cargo insta test --review
// This requires having previously run `cargo install cargo-insta`.
// See: https://insta.rs/docs/quickstart/
#[cfg(test)]
mod tests {
  use insta::{assert_display_snapshot, assert_yaml_snapshot};
  use openapi_lambda::error::format_error;
  use openapiv3::OpenAPI;
  use proc_macro2::TokenStream;

  use std::fs::File;
  use std::path::Path;

  #[test]
  fn test_openapi_apigw() {
    let openapi_apigw_path = Path::new(".openapi-lambda/openapi-apigw.yaml");
    let openapi_apigw_contents =
      serde_path_to_error::deserialize::<_, OpenAPI>(serde_yaml::Deserializer::from_reader(
        &File::open(openapi_apigw_path)
          .unwrap_or_else(|err| panic!("failed to read {}: {err}", openapi_apigw_path.display())),
      ))
      .unwrap_or_else(|err| {
        panic!(
          "failed to parse {}: {}",
          openapi_apigw_path.display(),
          format_error(&err, None, None)
        )
      });
    assert_yaml_snapshot!("openapi-apigw.yaml", openapi_apigw_contents);
  }

  #[test]
  fn test_out_rs() {
    let out_rs_path = Path::new(concat!(env!("OUT_DIR"), "/out.rs"));
    let out_rs_contents = std::fs::read_to_string(out_rs_path)
      .unwrap_or_else(|err| panic!("failed to read {}: {err}", out_rs_path.display()));
    out_rs_contents
      .parse::<TokenStream>()
      .unwrap_or_else(|err| {
        panic!(
          "failed to parse {} into token stream: {err}",
          out_rs_path.display()
        )
      });
    assert_display_snapshot!("out.rs", out_rs_contents);
  }

  #[test]
  fn test_foo_handler() {
    let foo_handler_path = Path::new(".openapi-lambda/foo_handler.rs");
    let foo_handler_contents = std::fs::read_to_string(foo_handler_path)
      .unwrap_or_else(|err| panic!("failed to read {}: {err}", foo_handler_path.display()));
    foo_handler_contents
      .parse::<TokenStream>()
      .unwrap_or_else(|err| {
        panic!(
          "failed to parse {} into token stream: {err}",
          foo_handler_path.display()
        )
      });
    assert_display_snapshot!("foo_handler.rs", foo_handler_contents);
  }

  #[test]
  fn test_bar_handler() {
    let bar_handler_path = Path::new(".openapi-lambda/bar_handler.rs");
    let bar_handler_contents = std::fs::read_to_string(bar_handler_path)
      .unwrap_or_else(|err| panic!("failed to read {}: {err}", bar_handler_path.display()));
    bar_handler_contents
      .parse::<TokenStream>()
      .unwrap_or_else(|err| {
        panic!(
          "failed to parse {} into token stream: {err}",
          bar_handler_path.display()
        )
      });
    assert_display_snapshot!("bar_handler.rs", bar_handler_contents);
  }
}
