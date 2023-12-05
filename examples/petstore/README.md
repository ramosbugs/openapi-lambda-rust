# Petstore API Example

This directory provides a complete example using OpenAPI Lambda for Rust together with
the
[AWS Serverless Application Model](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/index.html)
(SAM) to locally test the API and deploy it to AWS on ARM-based processors. It is based on the
[Swagger Petstore Example](https://github.com/swagger-api/swagger-petstore) OpenAPI definition.

The Petstore API itself is mostly unimplemented, but this example illustrates how to generate Rust
code, implement API handlers and middleware, test APIs locally, and deploy APIs to AWS. Inline
explanatory comments are provided in many files throughout this example.

## Prerequisites

Before using this example, be sure to install the
[AWS SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html).

## Build and deploy

To build this example, run:
```shell
sam build
```

To start the API locally for testing, first build, then run:
```shell
sam local start-api
```

To deploy this example to AWS, first build, then run:
```shell
sam deploy
```
