//! Generates the Rust-side error lookup table from `error-map.toml`
//! (the single source of truth for the FFI error bus) into
//! `OUT_DIR/error_cases_generated.rs`, included by `src/error.rs`.

use serde::Deserialize;
use std::fmt::Write as _;

#[derive(Deserialize)]
struct ErrorMap {
    #[serde(rename = "case")]
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    domain: String,
    code: String,
    description: String,
    description_empty_reason: Option<String>,
}

fn rust_str(s: &str) -> String {
    format!("{s:?}")
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = format!("{manifest_dir}/error-map.toml");
    println!("cargo:rerun-if-changed={path}");

    let text = std::fs::read_to_string(&path).expect("read error-map.toml");
    let map: ErrorMap = toml::from_str(&text).expect("parse error-map.toml");
    assert!(!map.cases.is_empty(), "error-map.toml has no cases");

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// @generated from error-map.toml by mm-common/build.rs -- do not edit."
    );
    let _ = writeln!(out, "pub static ERROR_CASES: &[ErrorCaseDef] = &[");
    for c in &map.cases {
        let empty = match &c.description_empty_reason {
            Some(s) => format!("Some({})", rust_str(s)),
            None => "None".to_string(),
        };
        let _ = writeln!(
            out,
            "    ErrorCaseDef {{ domain: {}, code: {}, description: {}, description_empty_reason: {} }},",
            rust_str(&c.domain),
            rust_str(&c.code),
            rust_str(&c.description),
            empty
        );
    }
    let _ = writeln!(out, "];");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{out_dir}/error_cases_generated.rs"), out)
        .expect("write generated error cases");
}
