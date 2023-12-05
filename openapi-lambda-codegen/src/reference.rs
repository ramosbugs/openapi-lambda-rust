use crate::DocCache;

use openapiv3::ReferenceOr;
use serde::de::DeserializeOwned;

use std::fs::File;
use std::path::{Path, PathBuf};

pub struct ResolvedReference<'a, T>
where
  T: DeserializeOwned,
{
  // The root-relative reference after the fragment and slash (`#/`) (e.g., components/schemas/Foo).
  pub root_rel_ref: &'a str,
  pub target: T,
  pub target_name: &'a str,
}

pub fn resolve_reference<'a, T>(
  referrer_doc_path: &Path,
  reference: &'a str,
  cached_external_docs: &mut DocCache,
) -> (PathBuf, ResolvedReference<'a, T>)
where
  T: DeserializeOwned,
{
  let ref_parts = reference.split('#').collect::<Vec<_>>();
  if ref_parts.len() != 2 || !ref_parts[1].starts_with('/') {
    panic!(
      "invalid reference: {reference} (referrer: {})",
      referrer_doc_path.display()
    );
  }

  let (rel_path, rel_ref) = (ref_parts[0], &ref_parts[1][1..]);
  let doc_path = if ref_parts[0].is_empty() {
    PathBuf::from(referrer_doc_path)
  } else {
    PathBuf::from(referrer_doc_path)
      .parent()
      .unwrap()
      .join(rel_path)
  };
  let doc: &serde_yaml::Mapping =
    cached_external_docs
      .entry(doc_path.clone())
      .or_insert_with(|| {
        println!("cargo:rerun-if-changed={}", doc_path.display());
        let doc_file = File::open(&doc_path)
          .unwrap_or_else(|err| panic!("failed to open {}: {err}", doc_path.to_string_lossy()));
        serde_path_to_error::deserialize(serde_yaml::Deserializer::from_reader(&doc_file))
          .unwrap_or_else(|err| {
            panic!(
              "failed to parse external OpenAPI doc {}: {err}",
              doc_path.display()
            )
          })
      });

  let (reference_target, reference_target_name) =
    rel_ref
      .split('/')
      .fold((doc, ""), |(doc_context, _), ref_component| {
        let target_doc_context = doc_context.get(ref_component).unwrap_or_else(|| {
          panic!(
            "invalid reference `{reference}`: path component `{ref_component}` not found in \
                 {doc_context:#?}"
          )
        });
        if let serde_yaml::Value::Mapping(next_doc_context) = target_doc_context {
          (next_doc_context, ref_component)
        } else {
          panic!(
            "invalid reference `{reference}`: must be a mapping, but found {target_doc_context:#?}"
          );
        }
      });

  let target_ref_or_item: ReferenceOr<T> =
    serde_path_to_error::deserialize(serde_yaml::Value::Mapping(reference_target.to_owned()))
      .unwrap_or_else(|err| {
        panic!(
          "failed to deserialize value referenced by `{reference}` (relative to {}): {err}",
          referrer_doc_path.display()
        )
      });

  match target_ref_or_item {
    ReferenceOr::Reference {
      reference: inner_reference,
    } => {
      // If we ever decide to support this, we probably want to recurse and return the final
      // destination of the reference chain. We'll also need to add cycle detection to avoid the
      // potential for infinite recursion. We should also make sure that after the inline pass
      // completes, there are no reference chains (in particular, schema reference chains) reachable
      // from any of the operations. That way, when we generate the models, we'll know that any
      // schema references point directly to named schemas.
      unimplemented!(
        "reference chains (references to references): `{reference}` -> `{inner_reference}`"
      );
    }
    ReferenceOr::Item(target) => (
      doc_path,
      ResolvedReference {
        root_rel_ref: rel_ref,
        target,
        target_name: reference_target_name,
      },
    ),
  }
}

pub fn resolve_local_reference<'a, T>(
  reference: &'a str,
  openapi_inline: &serde_yaml::Mapping,
) -> ResolvedReference<'a, T>
where
  T: DeserializeOwned,
{
  let ref_parts = reference.split('#').collect::<Vec<_>>();
  if ref_parts.len() != 2 || !ref_parts[1].starts_with('/') {
    panic!("invalid reference: {reference}");
  }

  let (rel_path, rel_ref) = (ref_parts[0], &ref_parts[1][1..]);
  if !rel_path.is_empty() {
    panic!("unexpected non-local reference: {reference}")
  }

  let (reference_target, reference_target_name) =
    rel_ref
      .split('/')
      .fold((openapi_inline, ""), |(doc_context, _), ref_component| {
        let target_doc_context = doc_context.get(ref_component).unwrap_or_else(|| {
          panic!(
            "invalid reference `{reference}`: path component `{ref_component}` not found in \
                 {doc_context:#?}"
          )
        });
        if let serde_yaml::Value::Mapping(next_doc_context) = target_doc_context {
          (next_doc_context, ref_component)
        } else {
          panic!(
            "invalid reference `{reference}`: must be a mapping, but found {target_doc_context:#?}"
          );
        }
      });

  let target_ref_or_item: ReferenceOr<T> =
    serde_path_to_error::deserialize(serde_yaml::Value::Mapping(reference_target.to_owned()))
      .unwrap_or_else(|err| {
        panic!("failed to deserialize local value referenced by `{reference}`: {err}");
      });

  match target_ref_or_item {
    ReferenceOr::Reference {
      reference: inner_reference,
    } => {
      // See note above.
      unimplemented!(
        "reference chains (references to references): `{reference}` -> `{inner_reference}`"
      );
    }
    ReferenceOr::Item(target) => ResolvedReference {
      root_rel_ref: rel_ref,
      target,
      target_name: reference_target_name,
    },
  }
}
