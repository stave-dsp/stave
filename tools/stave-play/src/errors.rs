use std::error::Error;

/// An error and its causes on one line, outermost first: how the UI
/// shows a typed error.
pub fn with_causes(error: &dyn Error) -> String {
    let mut causes = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }
    causes.join(": ")
}
