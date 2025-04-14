use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::{Serialize, Deserialize};
use serde_json::Value as JsonValue;
use ron::ser::to_string_pretty;
use ron::ser::PrettyConfig;

// === TYPES ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Value {
    Str(String),
    Json(JsonValue),
}

#[derive(Debug)]
struct Expr {
    path: Vec<String>, // e.g. ["fetch", "url"]
    eval: fn(&HashMap<String, Value>) -> Value,
}

// === HOST FUNCTIONS ===

fn http_get(url: &str) -> String {
    let body = reqwest::blocking::get(url)
        .expect("Failed GET")
        .text()
        .expect("Failed to read response body");
    body
}

fn json_parse(s: &str) -> JsonValue {
    serde_json::from_str(s).expect("Failed to parse JSON")
}

// === HARDCODED PROGRAM ===

fn garden_program() -> Vec<Expr> {
    vec![
        Expr {
            path: vec!["fetch".into(), "url".into()],
            eval: |_| Value::Str("https://catfact.ninja/fact".into()),
        },
        Expr {
            path: vec!["fetch".into(), "res".into()],
            eval: |ctx| {
                if let Value::Str(url) = ctx["fetch/url"].clone() {
                    Value::Str(http_get(&url))
                } else {
                    panic!("Expected string for url")
                }
            },
        },
        Expr {
            path: vec!["fetch".into(), "fact".into()],
            eval: |ctx| {
                if let Value::Str(json_str) = &ctx["fetch/res"] {
                    let parsed = json_parse(json_str);
                    let fact = parsed["fact"].as_str().unwrap_or("missing").to_string();
                    Value::Str(fact)
                } else {
                    panic!("Expected string for res")
                }
            },
        },
    ]
}

// === MAIN ===

fn main() {
    let exprs = garden_program();
    let mut context: HashMap<String, Value> = HashMap::new();

    for expr in exprs {
        let key = expr.path.join("/");
        let val = (expr.eval)(&context);
        println!("{:<20} = {:?}", key, &val);
        context.insert(key, val);
    }

    // Serialize to .value file
    let pretty = PrettyConfig::new().enumerate_arrays(true).struct_names(true);
    let ron_data = to_string_pretty(&context, pretty).unwrap();
    fs::write("values.ron", ron_data).expect("Failed to write .value file");
}

