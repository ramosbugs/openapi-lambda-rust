use std::any::Any;

pub use anyhow;
pub use aws_lambda_events;
pub use backtrace;
pub use base64;
pub use futures;
pub use headers;
pub use log;
pub use mime;
pub use serde;
pub use serde_json;
pub use serde_path_to_error;
pub use urlencoding;

pub mod encoding;

/// Extract the panic string or error after catching a panic.
pub fn panic_string(panic: Box<dyn Any + Send>) -> Result<String, Box<dyn Any + Send>> {
  panic
    .downcast::<String>()
    .map(|panic| panic.to_string())
    .or_else(|panic| panic.downcast::<&str>().map(|err| err.to_string()))
}
