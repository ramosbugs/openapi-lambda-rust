use serde::Serialize;

pub fn to_json<T>(value: &T) -> Result<String, serde_path_to_error::Error<serde_json::Error>>
where
  T: Serialize,
{
  let mut json_bytes = Vec::new();
  let mut serializer = serde_json::Serializer::new(&mut json_bytes);
  serde_path_to_error::serialize(value, &mut serializer)?;
  Ok(String::from_utf8(json_bytes).expect("JSON must be UTF-8"))
}
