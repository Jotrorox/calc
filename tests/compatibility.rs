//! Fixed results captured before replacing the engine. Unlike the executable
//! examples, these expectations are never evaluated by the implementation under test.
use calc_api::calculator::{CalcRequest, CalcValue, evaluate};
use rug::Float;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    request: CalcRequest,
    value: CalcValue,
}

fn compare(actual: &CalcValue, expected: &CalcValue) -> Result<(), String> {
    match (actual, expected) {
        (
            CalcValue::Number {
                real: ar,
                imaginary: ai,
                unit: au,
            },
            CalcValue::Number {
                real: er,
                imaginary: ei,
                unit: eu,
            },
        ) if au == eu => {
            for (a, e) in [(ar, er), (ai, ei)] {
                let a = Float::with_val(512, Float::parse(a).map_err(|e| e.to_string())?);
                let e = Float::with_val(512, Float::parse(e).map_err(|e| e.to_string())?);
                if a == e || (a.is_nan() && e.is_nan()) {
                    continue;
                }
                let tolerance =
                    Float::with_val(512, e.clone().abs().max(&Float::with_val(512, 1))) * 1e-7;
                if !a.is_finite() || !e.is_finite() || (a - e).abs() > tolerance {
                    return Err(format!("numeric mismatch: {actual:?} versus {expected:?}"));
                }
            }
            Ok(())
        }
        (CalcValue::Boolean { value: a }, CalcValue::Boolean { value: e }) if a == e => Ok(()),
        (CalcValue::Vector { values: a }, CalcValue::Vector { values: e }) => {
            compare_sequence(a, e)
        }
        (CalcValue::Matrix { rows: a }, CalcValue::Matrix { rows: e }) if a.len() == e.len() => {
            for (a, e) in a.iter().zip(e) {
                compare_sequence(a, e)?;
            }
            Ok(())
        }
        _ => Err(format!("value mismatch: {actual:?} versus {expected:?}")),
    }
}

fn compare_sequence(actual: &[CalcValue], expected: &[CalcValue]) -> Result<(), String> {
    if actual.len() != expected.len() {
        return Err("collection size mismatch".into());
    }
    for (a, e) in actual.iter().zip(expected) {
        compare(a, e)?;
    }
    Ok(())
}

#[test]
fn preserves_fixed_results_from_previous_engine() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/compatibility.json")).unwrap();
    assert_eq!(cases.len(), 227);
    let mut failures = Vec::new();
    for case in cases {
        let expression = case.request.expression.clone();
        match evaluate(case.request) {
            Ok(response) => match response.result {
                Some(result) => {
                    if let Err(error) = compare(&result.value, &case.value) {
                        failures.push(format!("{expression}: {error}"));
                    }
                }
                None => failures.push(format!("{expression}: missing result")),
            },
            Err(error) => failures.push(format!("{expression}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn eval(expression: &str) -> CalcValue {
    let request: CalcRequest =
        serde_json::from_value(serde_json::json!({"expression":expression})).unwrap();
    evaluate(request)
        .unwrap_or_else(|error| panic!("{expression}: {error}"))
        .result
        .unwrap()
        .value
}

#[test]
fn preserves_lazy_definitions_and_local_parameters() {
    assert_eq!(eval("x=2;y=x+1;x=4;y"), eval("5"));
    assert_eq!(eval("f(x)=a*x;a=3;f(2)"), eval("6"));
    assert_eq!(eval("x=7;f(x)=x^2;f(3)+x"), eval("16"));
    assert_eq!(eval("x=7;sum(x=1,3,x)+x"), eval("13"));
}

#[test]
fn preserves_precision_for_large_integer_and_radix_arithmetic() {
    assert_eq!(eval("9007199254740993-9007199254740992"), eval("1"));
    let request: CalcRequest = serde_json::from_value(serde_json::json!({
        "expression":"(2^1000+1)-2^1000", "precision":2048
    }))
    .unwrap();
    assert_eq!(evaluate(request).unwrap().result.unwrap().value, eval("1"));
    assert_eq!(eval("0x20000000000001-0x20000000000000"), eval("1"));
}

#[test]
fn preserves_unit_arithmetic_and_nonlinear_conversion() {
    assert_eq!(eval("unit cm=100m;250cm+1m"), eval("unit cm=100m;350cm"));
    compare(&eval("unit foo=m^2;9foo to m"), &eval("unit foo=m^2;3m")).unwrap();
    assert_eq!(
        eval("unit F=C*9/5+32;100C+32F"),
        eval("unit F=C*9/5+32;100C")
    );
}
