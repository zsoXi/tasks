use serde_json::{Value, json};
use tasks_core::messages::{self, MessageCode};
#[test]
fn every_code_renders_with_exact_catalogue_parameters() -> Result<(), Box<dyn std::error::Error>> {
    let catalogue = messages::catalogue();
    for code in MessageCode::ALL {
        let entry = catalogue["messages"]
            .as_array()
            .ok_or("catalogue")?
            .iter()
            .find(|m| m["code"] == code.code())
            .ok_or("missing code")?;
        let mut params = json!({});
        for param in entry["params"].as_array().ok_or("params")? {
            params[param.as_str().ok_or("parameter")?] = json!("example");
        }
        let rendered = messages::render(*code, &params, "en");
        assert!(!rendered.contains('{'), "{}", code.code());
        assert!(!rendered.contains('}'));
        assert_eq!(messages::render(*code, &params, "unknown-locale"), rendered);
    }
    assert_eq!(
        catalogue["messages"].as_array().map(Vec::len),
        Some(MessageCode::ALL.len())
    );
    let error = tasks::parse_input("{").err().ok_or("expected error")?;
    use std::error::Error;
    assert!(error.0.source().is_some());
    let response: Value = json!(error.response("en"));
    assert_eq!(response["warnings"], json!([]));
    Ok(())
}
