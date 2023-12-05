# How to Contribute

This project welcomes GitHub issues and pull requests!

Before submitting a pull request, be sure to:
1. Run `cargo fmt` to resolve any formatting issues.
2. Run `cargo clippy` using the minimum-supported Rust version (MSRV) and resolve any warnings.
   For example, if the MSRV is 1.70, run `cargo +1.70.0 clippy` (after installing that version
   using `rustup toolchain install 1.70.0`).
   Running Clippy using a newer Rust version may emit warnings that depend on Rust features not
   yet stabilized in the MSRV. These will be addressed whenever the MSRV is updated (see the
   [README](README.md#minimum-supported-rust-version-msrv) for more information about the MSRV
   policy).
3. Run `cargo test` to ensure that tests pass. When making changes that affect the generated code,
   the [Insta](https://insta.rs/) snapshots may need to be updated. First, install Insta by
   running `cargo install cargo-insta`. Then, update the test snapshots by running:
   ```shell
   cargo insta test --review
   ```
   Be sure to add the modified snapshot files to your pull request via `git add`.

To run the Petstore example API integration tests locally:
1. Install the
   [AWS SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html)
   if you have not already done so.
2. Run the following from the `examples/petstore` directory to start the local API emulator:
   ```shell
   sam local start-api
   ```
   Please note that this requires the `localhost` TCP port 3000 to be available.
3. In a separate terminal, run the tests as follows:
   ```shell
   cargo test -p petstore -- --ignored
   ```
