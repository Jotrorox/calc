//! A stateless, lossless JSON adapter around Kalker's own calculation engine.

use kalk::{errors::KalkError, kalk_value::KalkValue, parser};
use serde::{Deserialize, Serialize};

pub const MAX_EXPRESSION_BYTES: usize = 16_384;
pub const MIN_PRECISION: u32 = 32;
pub const MAX_PRECISION: u32 = 4_096;
pub const EVALUATION_TIMEOUT_MS: u32 = 1_500;
pub const MAX_RECURSION_DEPTH: u32 = 128;
pub const MAX_CONTEXT_ENTRIES: usize = 64;

fn default_precision() -> u32 {
    128
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AngleUnit {
    #[default]
    Rad,
    Deg,
}

impl AngleUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rad => "rad",
            Self::Deg => "deg",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcRequest {
    pub expression: String,
    /// Prior inputs evaluated sequentially in the same request-local context.
    #[serde(default)]
    pub context: Vec<String>,
    /// Requested binary precision; upstream algorithms can have lower accuracy.
    #[serde(default = "default_precision")]
    pub precision: u32,
    #[serde(default)]
    pub angle_unit: AngleUnit,
}

impl CalcRequest {
    pub fn validate(&self) -> Result<(), CalcError> {
        if self.expression.trim().is_empty() {
            return Err(CalcError::new(
                "invalid_request",
                "expression must not be empty",
            ));
        }
        if self.context.len() > MAX_CONTEXT_ENTRIES {
            return Err(CalcError::new(
                "invalid_request",
                format!("context must not exceed {MAX_CONTEXT_ENTRIES} entries"),
            ));
        }
        if self.context.iter().any(|entry| entry.trim().is_empty()) {
            return Err(CalcError::new(
                "invalid_request",
                "context entries must not be empty",
            ));
        }
        if self
            .expression
            .len()
            .saturating_add(self.context.iter().map(String::len).sum::<usize>())
            > MAX_EXPRESSION_BYTES
        {
            return Err(CalcError::new(
                "invalid_request",
                format!(
                    "expression and context must not exceed {MAX_EXPRESSION_BYTES} combined UTF-8 bytes"
                ),
            ));
        }
        if !(MIN_PRECISION..=MAX_PRECISION).contains(&self.precision) {
            return Err(CalcError::new(
                "invalid_request",
                format!("precision must be between {MIN_PRECISION} and {MAX_PRECISION} bits"),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalcResponse {
    /// The last statement's result; declarations alone produce null.
    pub result: Option<CalcResult>,
    pub precision: u32,
    pub angle_unit: AngleUnit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalcResult {
    /// A decimal rendering built from the same full-precision components as value.
    pub formatted: String,
    pub value: CalcValue,
}

/// Numbers are strings so JSON clients do not silently round them to doubles.
/// Nonfinite values use "NaN", "inf", and "-inf" strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CalcValue {
    Number {
        real: String,
        imaginary: String,
        unit: Option<String>,
    },
    Boolean {
        value: bool,
    },
    Vector {
        values: Vec<CalcValue>,
    },
    Matrix {
        rows: Vec<Vec<CalcValue>>,
    },
}

impl From<&KalkValue> for CalcValue {
    fn from(value: &KalkValue) -> Self {
        match value {
            KalkValue::Number(real, imaginary, unit) => Self::Number {
                real: normalize_decimal(&real.to_string()),
                imaginary: normalize_decimal(&imaginary.to_string()),
                unit: unit.clone(),
            },
            KalkValue::Boolean(value) => Self::Boolean { value: *value },
            KalkValue::Vector(values) => Self::Vector {
                values: values.iter().map(Self::from).collect(),
            },
            KalkValue::Matrix(rows) => Self::Matrix {
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(Self::from).collect())
                    .collect(),
            },
        }
    }
}

impl std::fmt::Display for CalcValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number {
                real,
                imaginary,
                unit,
            } => {
                if imaginary == "0" || imaginary == "-0" {
                    write!(f, "{real}")?;
                } else if real == "0" || real == "-0" {
                    write!(f, "{imaginary}i")?;
                } else if let Some(magnitude) = imaginary.strip_prefix('-') {
                    write!(f, "{real} - {magnitude}i")?;
                } else {
                    write!(f, "{real} + {imaginary}i")?;
                }
                if let Some(unit) = unit {
                    write!(f, " {unit}")?;
                }
                Ok(())
            }
            Self::Boolean { value } => write!(f, "{value}"),
            Self::Vector { values } => {
                write!(f, "(")?;
                format_values(f, values)?;
                write!(f, ")")
            }
            Self::Matrix { rows } => {
                write!(f, "[")?;
                for (index, row) in rows.iter().enumerate() {
                    if index > 0 {
                        write!(f, "; ")?;
                    }
                    format_values(f, row)?;
                }
                write!(f, "]")
            }
        }
    }
}

fn format_values(f: &mut std::fmt::Formatter<'_>, values: &[CalcValue]) -> std::fmt::Result {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{value}")?;
    }
    Ok(())
}

// Remove only fractional zeroes: trimming integer or exponent zeroes changes values.
fn normalize_decimal(decimal: &str) -> String {
    let (mantissa, exponent) = decimal
        .find(['e', 'E'])
        .map_or((decimal, ""), |index| decimal.split_at(index));
    let mantissa = if mantissa.contains('.') {
        mantissa.trim_end_matches('0').trim_end_matches('.')
    } else {
        mantissa
    };
    format!("{mantissa}{exponent}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalcError {
    pub code: String,
    pub message: String,
}

impl CalcError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CalcError {}

impl From<KalkError> for CalcError {
    fn from(error: KalkError) -> Self {
        let code = match error {
            KalkError::TimedOut => "timeout",
            KalkError::StackOverflow => "recursion_limit",
            _ => "calculation_error",
        };
        Self::new(code, error.to_string())
    }
}

/// Evaluate one request in a fresh context. Run this in the supervised worker
/// process: upstream's cooperative timeout does not cover every parsing/native path.
pub fn evaluate(request: CalcRequest) -> Result<CalcResponse, CalcError> {
    request.validate()?;
    let started = std::time::Instant::now();
    let mut context = parser::Context::new()
        .set_angle_unit(request.angle_unit.as_str())
        .set_timeout(Some(EVALUATION_TIMEOUT_MS))
        .set_max_recursion_depth(MAX_RECURSION_DEPTH);
    for entry in &request.context {
        let remaining = EVALUATION_TIMEOUT_MS
            .saturating_sub(started.elapsed().as_millis().min(u32::MAX as u128) as u32);
        if remaining == 0 {
            return Err(CalcError::new("timeout", "Operation took too long."));
        }
        context = context.set_timeout(Some(remaining));
        parser::eval(&mut context, entry, request.precision)?;
    }
    let remaining = EVALUATION_TIMEOUT_MS
        .saturating_sub(started.elapsed().as_millis().min(u32::MAX as u128) as u32);
    if remaining == 0 {
        return Err(CalcError::new("timeout", "Operation took too long."));
    }
    context = context.set_timeout(Some(remaining));
    let result =
        parser::eval(&mut context, &request.expression, request.precision)?.map(|result| {
            let value = CalcValue::from(result.value());
            CalcResult {
                formatted: value.to_string(),
                value,
            }
        });
    Ok(CalcResponse {
        result,
        precision: request.precision,
        angle_unit: request.angle_unit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(expression: &str) -> CalcRequest {
        CalcRequest {
            expression: expression.into(),
            context: vec![],
            precision: 128,
            angle_unit: AngleUnit::Rad,
        }
    }

    fn result(expression: &str) -> CalcResult {
        evaluate(request(expression))
            .unwrap_or_else(|error| panic!("{expression}: {error}"))
            .result
            .unwrap()
    }

    fn real(expression: &str) -> String {
        match result(expression).value {
            CalcValue::Number { real, .. } => real,
            other => panic!("expected number for {expression}, got {other:?}"),
        }
    }

    fn close(expression: &str, expected: f64) {
        let actual: f64 = real(expression).parse().unwrap();
        assert!(
            (actual - expected).abs() < 1e-7,
            "{expression}: {actual} != {expected}"
        );
    }

    macro_rules! real_cases {
        ($($name:ident: $expression:literal => $expected:expr),* $(,)?) => {$ (
            #[test]
            fn $name() { close($expression, $expected); }
        )*};
    }

    real_cases! {
        arithmetic: "2+3*4" => 14.0,
        parentheses: "(2+3)*4" => 20.0,
        power: "2^10" => 1024.0,
        factorial: "5!" => 120.0,
        implicit_multiplication: "a=3;2a(a+1)" => 24.0,
        unicode_pi: "π" => std::f64::consts::PI,
        unicode_square_root: "√(81)" => 9.0,
        scientific_notation: "1.25E3" => 1250.0,
        binary_literal: "0b1010" => 10.0,
        octal_literal: "0o17" => 15.0,
        hexadecimal_literal: "0xff" => 255.0,
        radix_suffix: "1101.101_2" => 13.625,
        variables_and_functions: "a=3;f(x)=2x+a;f(4)" => 11.0,
        multiline: "a=3\nf(x)=2x+a\nf(4)" => 11.0,
        piecewise: "f(x)={x^2 if x>0;0 otherwise};f(3)" => 9.0,
        recursive_function: "f(x)={f(x-1) if x>=1;x otherwise};f(5)" => 0.0,
        sine: "sin(pi/2)" => 1.0,
        cosine: "cos(0)" => 1.0,
        inverse_trigonometry: "atan(1)" => std::f64::consts::FRAC_PI_4,
        hyperbolic: "cosh(0)" => 1.0,
        natural_logarithm: "ln(e)" => 1.0,
        logarithm_base: "log(8,2)" => 3.0,
        gamma: "gamma(5)" => 24.0,
        combinations: "comb(5,2)" => 10.0,
        permutations: "perm(5,2)" => 20.0,
        gcd: "gcd(12,18)" => 6.0,
        lcm: "lcm(12,18)" => 36.0,
        bitwise_and: "bitand(6,3)" => 2.0,
        bitwise_or: "bitor(6,3)" => 7.0,
        bitwise_xor: "bitxor(6,3)" => 5.0,
        bitshift: "bitshift(3,2)" => 12.0,
        sum: "sum(n=1,5,2n)" => 30.0,
        product: "prod(n=1,4,n)" => 24.0,
        vector_sum: "sum(1,2,3)" => 6.0,
        vector_average: "average(1,2,3)" => 2.0,
        vector_length: "length((1,2,3))" => 3.0,
        vector_dot_product: "(2,3,5)*(7,11,13)" => 112.0,
        matrix_determinant: "det([1,2;3,4])" => -2.0,
        matrix_trace: "trace([1,2;3,4])" => 5.0,
        derivative: "f(x)=2x^2+x;f'(2)" => 9.0,
        integral: "integrate(0,pi,sin(x),dx)" => 2.0,
        equation: "2x=10" => 5.0,
        ceiling: "ceil(1.2)" => 2.0,
        floor: "floor(1.8)" => 1.0,
        rounding: "round(1.8)" => 2.0,
        fractional_part: "frac(1.25)" => 0.25,
        complex_absolute_value: "abs(3+4i)" => 5.0,
        real_part: "Re(3+4i)" => 3.0,
        imaginary_part: "Im(3+4i)" => 4.0,
    }

    #[test]
    fn request_defaults() {
        let request: CalcRequest = serde_json::from_value(json!({"expression":"1"})).unwrap();
        assert_eq!(request.precision, 128);
        assert_eq!(request.angle_unit, AngleUnit::Rad);
    }

    #[test]
    fn rejects_invalid_json_request_shapes() {
        for value in [
            json!({}),
            json!({"expression":1}),
            json!({"expression":"1","unexpected":true}),
            json!({"expression":"1","precision":2.5}),
            json!({"expression":"1","precision":-1}),
            json!({"expression":"1","angle_unit":"gradians"}),
            json!({"expression":"1","precision":null}),
        ] {
            assert!(
                serde_json::from_value::<CalcRequest>(value.clone()).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn validates_expression_and_precision_boundaries() {
        for expression in [
            "",
            " \n\t",
            &"x".repeat(MAX_EXPRESSION_BYTES + 1),
            &"π".repeat(MAX_EXPRESSION_BYTES / 2 + 1),
        ] {
            assert_eq!(
                evaluate(request(expression)).unwrap_err().code,
                "invalid_request"
            );
        }
        let mut req = request("1");
        for precision in [0, 31, 4097, u32::MAX] {
            req.precision = precision;
            assert_eq!(evaluate(req.clone()).unwrap_err().code, "invalid_request");
        }
        for precision in [32, 128, 4096] {
            req.precision = precision;
            assert_eq!(evaluate(req.clone()).unwrap().precision, precision);
        }
        assert!(
            request(&"1".repeat(MAX_EXPRESSION_BYTES))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn structured_complex_numbers_preserve_sign_and_i() {
        assert_eq!(
            result("3-4i").value,
            CalcValue::Number {
                real: "3".into(),
                imaginary: "-4".into(),
                unit: None
            }
        );
        assert_eq!(result("-4i").formatted, "-4i");
        assert_eq!(result("sqrt(-1)").formatted, "1i");
        assert_eq!(result("3-4i").formatted, "3 - 4i");
    }

    #[test]
    fn booleans_remain_booleans() {
        assert_eq!(
            result("2<3 and 3<4").value,
            CalcValue::Boolean { value: true }
        );
        assert_eq!(result("2>3").value, CalcValue::Boolean { value: false });
    }

    #[test]
    fn vector_matrix_and_nested_results() {
        assert_eq!(result("(1,2,3)*2").formatted, "(2, 4, 6)");
        assert_eq!(result("[1,2;3,4]*[2,0;0,2]").formatted, "[2, 4; 6, 8]");
        let value = serde_json::to_value(result("(1,2,3)").value).unwrap();
        assert_eq!(value["type"], "vector");
        assert_eq!(value["values"][0]["real"], "1");
        let value = serde_json::to_value(result("[1,2;3,4]").value).unwrap();
        assert_eq!(value["type"], "matrix");
        assert_eq!(value["rows"][1][1]["real"], "4");
        let value = result("((1,2),(3,4))").value;
        assert!(matches!(value, CalcValue::Vector { .. }));
    }

    #[test]
    fn comprehensions() {
        assert_eq!(result("[x:0≤x and 5>x]").formatted, "(0, 1, 2, 3, 4)");
    }

    #[test]
    fn equation_systems() {
        let CalcValue::Vector { values } = result("{x+y=3;x-y=1}").value else {
            panic!("expected vector")
        };
        assert_eq!(values.len(), 2);
        for (value, expected) in values.iter().zip([2.0, 1.0]) {
            let CalcValue::Number { real, .. } = value else {
                panic!("expected number")
            };
            assert!((real.parse::<f64>().unwrap() - expected).abs() < 1e-7);
        }
    }

    #[test]
    fn custom_units_and_conversion() {
        let value = result("unit cm=100m;250cm to m").value;
        let CalcValue::Number { real, unit, .. } = value else {
            panic!("expected number")
        };
        assert_eq!(real.parse::<f64>().unwrap(), 2.5);
        assert_eq!(unit.as_deref(), Some("m"));
    }

    #[test]
    fn angle_modes_and_explicit_angle_units() {
        let mut req = request("sin(90)");
        req.angle_unit = AngleUnit::Deg;
        let response = evaluate(req).unwrap();
        assert_eq!(response.angle_unit, AngleUnit::Deg);
        let CalcValue::Number { real, .. } = response.result.unwrap().value else {
            panic!("expected number")
        };
        assert!((real.parse::<f64>().unwrap() - 1.0).abs() < 1e-10);
        close("sin(90deg)", 1.0);
    }

    #[test]
    fn declarations_return_null_and_contexts_are_isolated() {
        assert!(
            evaluate(request("secret_value=42"))
                .unwrap()
                .result
                .is_none()
        );
        assert_eq!(
            evaluate(request("secret_value")).unwrap_err().code,
            "calculation_error"
        );
        assert!(evaluate(request("f(x)=x^2")).unwrap().result.is_none());
    }

    #[test]
    fn context_supports_ans_and_loaded_definitions() {
        let mut req = request("ans*factor");
        req.context = vec!["factor=4".into(), "2+3".into()];
        assert_eq!(evaluate(req).unwrap().result.unwrap().formatted, "20");
        assert!(evaluate(request("ans")).is_err());
    }

    #[test]
    fn context_validation() {
        let mut req = request("1");
        req.context = vec!["1".into(); MAX_CONTEXT_ENTRIES + 1];
        assert_eq!(req.validate().unwrap_err().code, "invalid_request");
        req.context = vec!["1".repeat(MAX_EXPRESSION_BYTES)];
        assert_eq!(req.validate().unwrap_err().code, "invalid_request");
        req.context = vec![" \n".into()];
        assert_eq!(req.validate().unwrap_err().code, "invalid_request");
        req.context = vec!["1".into(); MAX_CONTEXT_ENTRIES];
        assert!(req.validate().is_ok());
        req.context = vec!["unknown_fn(1)".into()];
        assert_eq!(evaluate(req).unwrap_err().code, "calculation_error");
    }

    #[test]
    fn high_precision_does_not_pass_through_f64() {
        assert_eq!(real("9007199254740993"), "9007199254740993");
        let mut req = request("1/7");
        req.precision = 256;
        let response = evaluate(req).unwrap();
        let CalcValue::Number { real, .. } = response.result.unwrap().value else {
            panic!("expected number")
        };
        assert!(
            real.starts_with("1.428571428571428571428571428571428571428571428571428571"),
            "{real}"
        );
        assert!(real.ends_with("e-1"));
        assert!(real.len() > 60);
    }

    #[test]
    fn nonfinite_values_remain_valid_json_strings() {
        for expression in ["1/0", "0/0"] {
            let value = serde_json::to_value(result(expression)).unwrap();
            assert!(value["value"]["real"].is_string());
            assert!(!value["value"]["real"].is_null());
        }
    }

    #[test]
    fn malformed_expressions_are_errors() {
        for expression in ["(", "2+", "unknown_fn(1)", "sqrt()"] {
            assert_eq!(
                evaluate(request(expression)).unwrap_err().code,
                "calculation_error",
                "{expression}"
            );
        }
    }

    #[test]
    fn recursion_is_bounded() {
        assert_eq!(
            evaluate(request("f(x)=f(x+1);f(1)")).unwrap_err().code,
            "recursion_limit"
        );
    }

    #[test]
    fn timeout_is_bounded() {
        let started = std::time::Instant::now();
        assert_eq!(
            evaluate(request("sum(n=1,1000000000000,n)"))
                .unwrap_err()
                .code,
            "timeout"
        );
        assert!(started.elapsed().as_secs() < 10);
    }

    #[test]
    fn decimal_normalization_preserves_magnitude() {
        for (input, expected) in [
            ("100", "100"),
            ("100.000", "100"),
            ("1.2000e100", "1.2e100"),
            ("0.000", "0"),
            ("-0.000", "-0"),
            ("1.000e-10", "1e-10"),
            ("NaN", "NaN"),
            ("inf", "inf"),
            ("-inf", "-inf"),
        ] {
            assert_eq!(normalize_decimal(input), expected);
        }
    }

    #[test]
    fn responses_and_errors_round_trip_for_worker_protocol() {
        let response = evaluate(request("(9007199254740993,2+3i)")).unwrap();
        let encoded = serde_json::to_vec(&response).unwrap();
        assert_eq!(
            serde_json::from_slice::<CalcResponse>(&encoded).unwrap(),
            response
        );
        let error = CalcError::new("calculation_error", "invalid expression");
        let encoded = serde_json::to_vec(&error).unwrap();
        assert_eq!(
            serde_json::from_slice::<CalcError>(&encoded).unwrap(),
            error
        );
    }
}
