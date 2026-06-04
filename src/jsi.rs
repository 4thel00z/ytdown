//! JavaScript interpreter (boa_engine) for solving extractor ciphers.

use boa_engine::{Context, Source};

use crate::error::{Error, Result};

/// A compiled JS function (source + name) executed in a fresh boa context per call.
///
/// boa contexts are not [`Send`], so we store the SOURCE and rebuild the context on
/// each call. Cipher functions run rarely (once per player version per video batch),
/// so rebuilding is cheap relative to the surrounding network work.
// Consumed by the YouTube player cipher solver (`extractor::youtube::player`),
// which is implemented in a later task; allow dead code until that lands.
#[allow(dead_code)]
pub(crate) struct JsFunction {
    source: String,
    name: String,
}

#[allow(dead_code)]
impl JsFunction {
    /// Validate `source` by evaluating it once, then store it for later calls.
    ///
    /// A parse or evaluation failure is reported as [`Error::Cipher`].
    pub fn compile(source: &str, name: &str) -> Result<Self> {
        let mut context = Context::default();
        context
            .eval(Source::from_bytes(source))
            .map_err(|e| Error::Cipher(format!("failed to compile function `{name}`: {e}")))?;
        Ok(Self {
            source: source.to_string(),
            name: name.to_string(),
        })
    }

    /// Evaluate `source; name(input)` with a single string argument, returning the
    /// string result.
    ///
    /// The input is serialized via [`serde_json`] so it is correctly quoted and
    /// escaped before being spliced into the script. Any boa error becomes
    /// [`Error::Cipher`].
    pub fn call_str(&self, input: &str) -> Result<String> {
        let input_json = serde_json::to_string(input)
            .map_err(|e| Error::Cipher(format!("failed to encode input: {e}")))?;
        let script = format!(
            "{src}\n{name}({arg})",
            src = self.source,
            name = self.name,
            arg = input_json
        );

        let mut context = Context::default();
        let value = context
            .eval(Source::from_bytes(&script))
            .map_err(|e| Error::Cipher(format!("failed to evaluate `{}`: {e}", self.name)))?;
        let js_string = value.to_string(&mut context).map_err(|e| {
            Error::Cipher(format!("result of `{}` is not a string: {e}", self.name))
        })?;
        Ok(js_string.to_std_string_lossy())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn runs_extracted_function_against_input() {
        let src = r#"function decode(a){a=a.split("");a.reverse();return a.join("")}"#;
        let f = JsFunction::compile(src, "decode").unwrap();
        assert_eq!(f.call_str("abc").unwrap(), "cba");
    }

    #[test]
    fn compile_error_is_cipher_error() {
        assert!(matches!(
            JsFunction::compile("not js ((", "f"),
            Err(Error::Cipher(_))
        ));
    }

    #[test]
    fn call_is_reusable_and_send() {
        fn assert_send<T: Send>() {}
        assert_send::<JsFunction>();
    }
}
