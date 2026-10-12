//! Form and query parameter parsing.
//!
//! RFC 6749 §3.1 and §3.2: "Request and response parameters MUST NOT be included more than once."
//! RFC 8707 makes `resource` the one exception.

use std::collections::HashMap;

use super::error::{ErrorCode, OAuthError};

/// Maximum number of parameters accepted in one request.
const MAX_PARAMS: usize = 64;

/// Parsed `application/x-www-form-urlencoded` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params {
    single: HashMap<String, String>,
    resources: Vec<String>,
}

impl Params {
    /// Parses a query string or form body.
    ///
    /// # Errors
    ///
    /// `invalid_request` for a repeated parameter (other than `resource`) or too many parameters.
    /// Parameters with empty values are treated as absent (RFC 6749 §3.1).
    pub fn parse(input: &[u8]) -> Result<Self, OAuthError> {
        let mut params = Self::default();
        for (count, (key, value)) in form_urlencoded::parse(input).enumerate() {
            if count >= MAX_PARAMS {
                return Err(OAuthError::new(
                    ErrorCode::InvalidRequest,
                    "too many parameters",
                ));
            }
            if value.is_empty() {
                continue;
            }
            if key == "resource" {
                params.resources.push(value.into_owned());
                continue;
            }
            if params
                .single
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(OAuthError::new(
                    ErrorCode::InvalidRequest,
                    "a parameter was repeated",
                ));
            }
        }
        Ok(params)
    }

    /// A single-valued parameter.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.single.get(key).map(String::as_str)
    }

    /// All `resource` values (RFC 8707).
    #[must_use]
    pub fn resources(&self) -> &[String] {
        &self.resources
    }

    /// Whether any parameter other than those in `allowed` is present.
    #[must_use]
    pub fn has_other_than(&self, allowed: &[&str]) -> bool {
        self.single.keys().any(|k| !allowed.contains(&k.as_str()))
            || (!self.resources.is_empty() && !allowed.contains(&"resource"))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn rejects_repeats_but_allows_resource() {
        assert!(Params::parse(b"a=1&a=2").is_err());
        let p = Params::parse(b"resource=x&resource=y&a=%20b&empty=").unwrap();
        assert_eq!(p.resources(), ["x", "y"]);
        assert_eq!(p.get("a"), Some(" b"));
        assert_eq!(p.get("empty"), None);
    }

    proptest! {
        #[test]
        fn parse_never_panics(input in proptest::collection::vec(any::<u8>(), 0..512)) {
            drop(Params::parse(&input));
        }
    }
}
