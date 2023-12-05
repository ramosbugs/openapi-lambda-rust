# Makefile for building Rust Lambdas. Running `sam build` invokes each build-*ApiFunction target
# since the SAM template uses `BuildMethod: makefile`.

# Paths that should trigger recompilation when they change.
# TIP: Be sure to include any other OpenAPI definitions referenced by the root openapi.yaml (e.g.,
# using a wildcard).
DEPS = build.rs ../../Cargo.toml Cargo.toml Makefile openapi.yaml src/

# Path to the Cargo workspace's target directory.
# TIP: Update this to reflect your crate's nesting level within a project (or just `target` for a
# root crate not nested within a workspace).
TARGET_DIR = ../../target

# Change to `musl-cross` to use Homebrew musl-cross package instead.
# NOTE: cargo-lambda's Zig toolchain (tested on MacOS w/ Apple Silicon) does not seem to support
# debug symbols, causing useless backtraces. See
# https://github.com/cargo-lambda/cargo-lambda/issues/577.
# Use the musl-cross toolchain from Homebrew for reliable stack traces when cross-compiling on MacOS
# (https://github.com/FiloSottile/homebrew-musl-cross):
TOOLCHAIN = cargo-lambda

# Build using cargo-lambda .
ifeq ($(TOOLCHAIN),cargo-lambda)

$(TARGET_DIR)/lambda/%/bootstrap: $(DEPS)
# TIP: remove --arm64 to compile for x86-64.
# 2>&1 since sam build hides the stderr output unless the command fails, but it's useful to see.
	cargo lambda build --arm64 --release -p petstore 2>&1

.PHONY: bootstrap_%
bootstrap_%: $(TARGET_DIR)/lambda/bootstrap_%/bootstrap
	cp $(TARGET_DIR)/lambda/$@/bootstrap $(ARTIFACTS_DIR)

else ifeq ($(TOOLCHAIN),musl-cross)

# Build using Homebrew musl-cross package.
# TIP: replace `aarch64` with `x86_64` everywhere below to compile for x86-64.
$(TARGET_DIR)/aarch64-unknown-linux-musl/release/%: $(DEPS)
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc \
	  cargo build --target aarch64-unknown-linux-musl --release -p petstore -vv 2>&1

.PHONY: bootstrap_%
bootstrap_%: $(TARGET_DIR)/aarch64-unknown-linux-musl/release/bootstrap_%
	cp $(TARGET_DIR)/aarch64-unknown-linux-musl/release/$@ $(ARTIFACTS_DIR)/bootstrap

else
$(error unknown toolchain `$(TOOLCHAIN)`)
endif

# Each target below copies the corresponding `bootstrap` binary to the root of the Lambda artifacts
# directory (see
# https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/building-custom-runtimes.html).
# There should be one target for each `AWS::Serverless::Function` in the AWS SAM template.yaml.

# TIP: Replace `PetApiFunction` with the logical ID of the function defined in the AWS SAM
# template.yaml, and replace `bootstrap_pet` with the name of the corresponding module in `src/bin`.
.PHONY: build-PetApiFunction
build-PetApiFunction: bootstrap_pet

.PHONY: build-StoreApiFunction
build-StoreApiFunction: bootstrap_store

.PHONY: build-UserApiFunction
build-UserApiFunction: bootstrap_user
