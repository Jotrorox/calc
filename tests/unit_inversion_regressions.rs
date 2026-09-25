use calc_api::calculator::{AngleUnit, CalcRequest, CalcValue, evaluate};
use rug::Float;

fn eval(expression: &str, precision: u32) -> CalcValue {
    evaluate(CalcRequest {
        expression: expression.to_owned(),
        context: vec![],
        precision,
        angle_unit: AngleUnit::Rad,
    })
    .unwrap_or_else(|error| panic!("{expression}: {error}"))
    .result
    .unwrap()
    .value
}

fn assert_number(expression: &str, precision: u32, expected: &str, unit: &str, tolerance: &str) {
    let CalcValue::Number {
        real,
        imaginary,
        unit: actual_unit,
    } = eval(expression, precision)
    else {
        panic!("{expression}: expected a number");
    };
    assert_eq!(actual_unit.as_deref(), Some(unit), "{expression}");
    let parse = |text: &str| Float::with_val(precision, Float::parse(text).unwrap());
    assert!(parse(&imaginary).is_zero(), "{expression}: {imaginary}i");
    let actual = parse(&real);
    assert!(actual.is_finite(), "{expression}: {real}");
    assert!(
        (actual - parse(expected)).abs() <= parse(tolerance),
        "{expression}: got {real}, expected {expected} within {tolerance}"
    );
}

#[test]
fn invert_unit_rejects_deceptive_affine_samples() {
    for (formula, target, expected) in [
        ("m+m*(m-1)*(m-2)", "9", "3"),
        ("2*m+5+m*(m-1)*(m-2)", "17", "3"),
        ("m+m*(m-1)*(m-2)*(m-3)", "28", "4"),
        ("m+0.000000000001*m*(m-1)*(m-2)", "9.000000000504", "9"),
    ] {
        assert_number(
            &format!("unit foo={formula};{target}foo to m"),
            128,
            expected,
            "m",
            "1e-10",
        );
    }
}

#[test]
fn invert_unit_preserves_forward_inverse_round_trips() {
    let declaration = "unit foo=m+m*(m-1)*(m-2)";
    assert_number(&format!("{declaration};3m to foo"), 128, "9", "foo", "0");
    for point in [3, 4, 7] {
        assert_number(
            &format!("{declaration};({point}m to foo) to m"),
            128,
            &point.to_string(),
            "m",
            "1e-10",
        );
    }
    for target in [9, 28, 217] {
        assert_number(
            &format!("{declaration};({target}foo to m) to foo"),
            128,
            &target.to_string(),
            "foo",
            "1e-9",
        );
    }
}

#[test]
fn invert_unit_keeps_high_precision_affine_conversions() {
    for precision in [32, 128, 256, 4096] {
        assert_number("unit cm=100m;250cm to m", precision, "2.5", "m", "0");
        assert_number("unit F=C*9/5+32;212F to C", precision, "100", "C", "1e-5");
    }
    assert_eq!(
        eval("unit cm=100m;(1+i)cm to m", 256),
        eval("unit cm=100m;(0.01+0.01i)m", 256),
    );
    assert_number("unit F=C*9/5+32;32F to C", 128, "0", "C", "0");
    assert_number("unit F=C*9/5+32;212F to C", 256, "100", "C", "1e-70");
    assert_number(
        "unit cm=100m;900719925474099300cm to m",
        256,
        "9007199254740993",
        "m",
        "0",
    );
    assert_number(
        "unit foo=m/3+1000000;(9007199254740993m to foo) to m",
        256,
        "9007199254740993",
        "m",
        "1e-54",
    );
    assert_number(
        "unit foo=3m;1foo to m",
        256,
        "0.333333333333333333333333333333333333333333333333333333333333333333333333333333",
        "m",
        "1e-75",
    );
}

#[test]
fn invert_unit_preserves_already_nonfinite_affine_targets() {
    for (target, expected) in [("1/0", "inf"), ("-1/0", "-inf"), ("0/0", "NaN")] {
        assert_eq!(
            eval(&format!("unit cm=100m;({target})cm to m"), 128),
            CalcValue::Number {
                real: expected.to_owned(),
                imaginary: "0".to_owned(),
                unit: Some("m".to_owned()),
            },
        );
    }
    assert_eq!(
        eval("unit cm=100m;(i/0)cm to m", 128),
        eval("unit cm=100m;(((i/0)-0)/100)m", 128),
    );
}

#[test]
fn invert_unit_rejects_nonfinite_forward_candidate_for_finite_target() {
    assert_number(
        "unit foo=m+m*(m-1)*(m-2)/(m-9);(9foo to m) to foo",
        128,
        "9",
        "foo",
        "1e-10",
    );
}

#[test]
fn invert_unit_keeps_affine_slopes_outside_f64_range() {
    for slope in ["1E-400", "1E400", "-1E-400", "-1E400"] {
        assert_number(
            &format!("unit foo={slope}*m;(9007199254740993m to foo) to m"),
            256,
            "9007199254740993",
            "m",
            "1e-58",
        );
    }
}
