use std::io::{Read, Write};
use tasks::{Error, Options};
use tasks_core::{Error as CoreError, messages::MessageCode as C};

fn execute() -> Result<serde_json::Value, Error> {
    let args = std::env::args_os()
        .skip(1)
        .map(|a| {
            a.into_string()
                .map_err(|_| CoreError::new(C::Utf8, serde_json::json!({})))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if args.len() > 1 {
        return Err(CoreError::new(C::Arguments, serde_json::json!({})).into());
    }
    let first = args.into_iter().next();
    match first.as_deref() {
        Some("--schema") => return Ok(tasks::schema()),
        Some("--messages") => return Ok(tasks_core::messages::catalogue()),
        _ => (),
    }
    let opts = Options {
        base_dir: std::env::current_dir().map_err(|e| CoreError::caused(C::Io, e))?,
        ..Options::default()
    };
    let text = if let Some(text) = first {
        text
    } else {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(opts.max_input_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| CoreError::caused(C::Io, e))?;
        String::from_utf8(bytes).map_err(|e| CoreError::caused(C::Utf8, e))?
    };
    if text.len() as u64 > opts.max_input_bytes {
        return Err(
            CoreError::new(C::Size, serde_json::json!({"limit":opts.max_input_bytes})).into(),
        );
    }
    let input = tasks::parse_input(&text)?;
    Ok(serde_json::json!(tasks::run(&input, &opts)?))
}
fn main() -> std::process::ExitCode {
    let (value, success) = match execute() {
        Ok(v) => (v, true),
        Err(e) => (serde_json::json!(e.response("en")), false),
    };
    if writeln!(std::io::stdout().lock(), "{value}").is_err() {
        return std::process::ExitCode::FAILURE;
    }
    if success {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
