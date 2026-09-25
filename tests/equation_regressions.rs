//! Public API regressions for finite Newton-system convergence.
use calc_api::calculator::{CalcRequest, CalcValue, evaluate};

fn request(expression: &str) -> CalcRequest {
    serde_json::from_value(serde_json::json!({"expression": expression})).unwrap()
}

fn assert_no_finite_solution(expression: &str) {
    let error = evaluate(request(expression)).expect_err(expression);
    assert_eq!(error.code, "calculation_error", "{expression}: {error}");
    assert!(
        error.message.contains("finite solution"),
        "{expression}: {error}"
    );
}

fn assert_roots(expression: &str, expected: &[f64]) {
    let result = evaluate(request(expression))
        .unwrap_or_else(|error| panic!("{expression}: {error}"))
        .result
        .expect("equations should return roots")
        .value;
    let roots = match result {
        CalcValue::Vector { values } => values,
        value => vec![value],
    };
    assert_eq!(roots.len(), expected.len(), "{expression}");
    for (root, expected) in roots.iter().zip(expected) {
        let CalcValue::Number {
            real,
            imaginary,
            unit,
        } = root
        else {
            panic!("{expression}: expected a numeric root, got {root:?}");
        };
        let actual: f64 = real.parse().unwrap();
        assert!(actual.is_finite(), "{expression}: {root:?}");
        assert_eq!(imaginary, "0", "{expression}");
        assert_eq!(unit, &None, "{expression}");
        assert!(
            (actual - expected).abs() < 1e-8,
            "{expression}: expected {expected}, got {actual}"
        );
    }
}

#[test]
fn newton_system_rejects_nan_residuals_in_every_component() {
    for expression in [
        "x+0/0=0",
        "{x-1=0;y+0/0=0}",
        "{x+0/0=0;y-1=0}",
        "{x+0/0=0;y+0/0=0}",
    ] {
        assert_no_finite_solution(expression);
    }
}

#[test]
fn newton_system_rejects_infinite_residuals() {
    for expression in ["x+1/0=0", "x-1/0=0", "{x-1=0;y+1/0=0}"] {
        assert_no_finite_solution(expression);
    }
}

#[test]
fn newton_system_tries_other_seeds_after_invalid_residuals() {
    assert_roots("f(x)={0/0 if x>=0;x^2 otherwise};(f(x)=4)", &[-2.0]);
    assert_roots("1/(x-1)=1", &[2.0]);
}

#[test]
fn newton_system_backtracks_past_invalid_residuals() {
    // The first full Newton step leaves the valid domain. A shorter step
    // remains valid and leads to the root rather than accepting NaN as zero.
    assert_roots(
        "f(x)={0/0 if x<=0;ln(x)+2 otherwise};(f(x)=0)",
        &[(-2.0_f64).exp()],
    );
    assert_roots(
        "f(x)={0/0 if x<=0;ln(x)+2 otherwise};{f(x)=0;y-2=0}",
        &[(-2.0_f64).exp(), 2.0],
    );
    assert_roots(
        "f(x)={1/0 if x<=0;ln(x)+2 otherwise};(f(x)=0)",
        &[(-2.0_f64).exp()],
    );
}

#[test]
fn newton_system_rejects_nonfinite_candidate_coordinates() {
    // The residual at seed 1 divided by the finite-difference slope overflows.
    // Even though the function returns zero at infinity, that is not a root.
    assert_roots(
        "f(x)={10^308 if x=1;0 if abs(x)>10^309;0.0001*x otherwise};(f(x)=0)",
        &[0.0],
    );
}

#[test]
fn newton_system_preserves_ordinary_nonlinear_roots() {
    assert_roots("x^2=2", &[2.0_f64.sqrt()]);
    assert_roots("3x^3-2x=x^2+2", &[1.2707763267]);
    assert_roots("{x+y+z=9;z*x-y=10;4y+12x-2=42}", &[3.0, 2.0, 4.0]);
}
