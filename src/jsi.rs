//! JavaScript interpreter (boa_engine) for solving extractor ciphers.

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use boa_engine::{Context, Source};

use crate::error::{Error, Result};

/// Maximum loop iterations a single cipher evaluation may run before boa throws.
///
/// The real `sig`/`nsig` transforms iterate over a small character array, so a
/// few hundred thousand iterations is comfortably above legitimate use while
/// bounding a hostile `while(1){}` in attacker-influenced player JS.
const LOOP_ITERATION_LIMIT: u64 = 5_000_000;

/// Wall-clock budget for a single cipher evaluation.
///
/// Even with the loop-iteration limit, a hostile player body could attempt a
/// giant allocation or other non-looping CPU/memory blowup. The blocking eval is
/// run on a dedicated thread under this timeout so a single malicious video
/// cannot wedge an executor worker forever; the runaway thread is abandoned.
#[cfg(not(target_arch = "wasm32"))]
const EVAL_TIMEOUT: Duration = Duration::from_secs(5);

/// Build a boa [`Context`] with execution limits applied.
///
/// Sets a loop-iteration limit (defends against `while(1){}` in
/// attacker-influenced player JS) on top of boa's default recursion/stack
/// limits. Exceeding any limit throws a catchable JS error rather than hanging.
fn limited_context() -> Context {
    let mut context = Context::default();
    context
        .runtime_limits_mut()
        .set_loop_iteration_limit(LOOP_ITERATION_LIMIT);
    context
}

/// A compiled JS function (source + name) executed in a fresh boa context per call.
///
/// boa contexts are not [`Send`], so we store the SOURCE and rebuild the context on
/// each call. Cipher functions run rarely (once per player version per video batch),
/// so rebuilding is cheap relative to the surrounding network work.
#[derive(Debug, Clone)]
pub(crate) struct JsFunction {
    source: String,
    name: String,
}

impl JsFunction {
    /// Validate `source` by evaluating it once, then store it for later calls.
    ///
    /// A parse or evaluation failure is reported as [`Error::Cipher`].
    pub fn compile(source: &str, name: &str) -> Result<Self> {
        let mut context = limited_context();
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
    /// escaped before being spliced into the script. Evaluation runs under a
    /// loop-iteration limit (see [`limited_context`]). Any boa error becomes
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

        let mut context = limited_context();
        let value = context
            .eval(Source::from_bytes(&script))
            .map_err(|e| Error::Cipher(format!("failed to evaluate `{}`: {e}", self.name)))?;
        let js_string = value.to_string(&mut context).map_err(|e| {
            Error::Cipher(format!("result of `{}` is not a string: {e}", self.name))
        })?;
        Ok(js_string.to_std_string_lossy())
    }

    /// Asynchronously evaluate this function against `input`.
    ///
    /// The synchronous, CPU-bound boa evaluation is run on a dedicated blocking
    /// thread (via [`tokio::task::spawn_blocking`]) so it never blocks the async
    /// executor, and is bounded by [`EVAL_TIMEOUT`]: if the player JS hangs (a
    /// hostile `while(1){}` that somehow slips the loop limit, or a giant
    /// allocation), the future returns an [`Error::Cipher`] rather than wedging a
    /// worker thread. The runaway blocking thread is abandoned.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn call_str_async(&self, input: &str) -> Result<String> {
        let this = self.clone();
        let input = input.to_string();
        let join = tokio::task::spawn_blocking(move || this.call_str(&input));
        match tokio::time::timeout(EVAL_TIMEOUT, join).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_err)) => Err(Error::Cipher(format!(
                "cipher evaluation thread failed: {join_err}"
            ))),
            Err(_elapsed) => Err(Error::Cipher(
                "cipher evaluation exceeded time limit".into(),
            )),
        }
    }

    /// Asynchronously evaluate this function against `input` (wasm32).
    ///
    /// On `wasm32-unknown-unknown` there is no blocking-thread pool and no
    /// `tokio::time` wheel, so the CPU-bound boa evaluation runs inline. The
    /// loop-iteration limit in [`limited_context`] still bounds a hostile
    /// `while(1){}`; the wall-clock [`EVAL_TIMEOUT`] is not enforced here, so a
    /// non-looping CPU/memory blowup is not separately bounded on wasm.
    #[cfg(target_arch = "wasm32")]
    pub async fn call_str_async(&self, input: &str) -> Result<String> {
        self.call_str(input)
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

    #[test]
    fn infinite_loop_in_body_is_bounded_by_iteration_limit() {
        // A hostile player body with an unbounded loop must not hang: the
        // loop-iteration limit makes boa throw, surfacing as a Cipher error.
        let src = r#"function evil(a){var i=0;while(true){i=i+1}return a}"#;
        let f = JsFunction::compile(src, "evil").unwrap();
        assert!(matches!(f.call_str("seed"), Err(Error::Cipher(_))));
    }

    #[tokio::test]
    async fn async_call_runs_off_executor_and_returns_result() {
        let src = r#"function decode(a){a=a.split("");a.reverse();return a.join("")}"#;
        let f = JsFunction::compile(src, "decode").unwrap();
        assert_eq!(f.call_str_async("abc").await.unwrap(), "cba");
    }
}
