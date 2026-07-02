//! Output helpers: render either a human view or a structured JSON/YAML view.

use anyhow::Result;

use crate::cli::OutputFormat;

/// Emit a result. For `Human`, run `human`. For `Json`/`Yaml`, serialize `value`.
pub fn emit(fmt: OutputFormat, value: &serde_json::Value, human: impl FnOnce()) -> Result<()> {
    match fmt {
        OutputFormat::Human => human(),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
        OutputFormat::Yaml => print!("{}", serde_yaml::to_string(value)?),
    }
    Ok(())
}

/// Emit a single streaming record (one line for JSON, a block otherwise).
pub fn emit_stream(
    fmt: OutputFormat,
    value: &serde_json::Value,
    human: impl FnOnce(),
) -> Result<()> {
    match fmt {
        OutputFormat::Human => human(),
        OutputFormat::Json => println!("{}", serde_json::to_string(value)?),
        OutputFormat::Yaml => {
            print!("{}", serde_yaml::to_string(value)?);
            println!("---");
        }
    }
    Ok(())
}
