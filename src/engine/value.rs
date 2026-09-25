//! Numeric values, collection algebra, and the calculator's built-in functions.
//!
//! This module defines the application's mathematical behavior directly. Rug is
//! used only as an arbitrary-precision arithmetic primitive (MPFR and MPC).

use crate::calculator::CalcError;
use rug::{
    Assign, Complex, Float, Integer,
    float::{Constant, Round},
    ops::Pow,
};
use std::cmp::Ordering;

type Result<T> = std::result::Result<T, CalcError>;

#[derive(Clone, Debug)]
pub(crate) enum Value {
    Number(Float, Float, Option<String>),
    Boolean(bool),
    Vector(Vec<Value>),
    Matrix(Vec<Vec<Value>>),
}

fn error(message: impl Into<String>) -> CalcError {
    CalcError::new("calculation_error", message)
}

impl Value {
    pub(crate) fn number<T>(precision: u32, value: T) -> Self
    where
        Float: Assign<T>,
    {
        Self::Number(
            Float::with_val(precision, value),
            Float::new(precision),
            None,
        )
    }

    pub(crate) fn parse(text: &str, precision: u32) -> Result<Self> {
        let parsed = Float::parse(text).map_err(|_| error(format!("Invalid number: {text}")))?;
        Ok(Self::number(precision, parsed))
    }

    pub(crate) fn parse_radix(text: &str, radix: u32, precision: u32) -> Result<Self> {
        if radix == 10 {
            return Self::parse(text, precision);
        }
        if !(2..=36).contains(&radix) {
            return Err(error("Number base must be between 2 and 36."));
        }
        let parsed = Float::parse_radix(text, radix as i32)
            .map_err(|_| error(format!("Invalid base-{radix} number: {text}")))?;
        Ok(Self::number(precision, parsed))
    }

    pub(crate) fn real(&self) -> Result<&Float> {
        match self {
            Self::Number(real, imaginary, _) if imaginary.is_zero() => Ok(real),
            _ => Err(error("Expected a real number.")),
        }
    }

    pub(crate) fn as_f64(&self) -> Result<f64> {
        Ok(self.real()?.to_f64())
    }

    pub(crate) fn truthy(&self) -> Result<bool> {
        match self {
            Self::Boolean(value) => Ok(*value),
            _ => Err(error("Expected a boolean condition.")),
        }
    }

    pub(crate) fn with_unit(mut self, unit: Option<String>) -> Result<Self> {
        match &mut self {
            Self::Number(_, _, tag) => {
                *tag = unit;
                Ok(self)
            }
            _ => Err(error("Only numbers can have units.")),
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        matches!(self, Self::Number(real, imaginary, _) if real.is_zero() && imaginary.is_zero())
    }

    fn complex(&self, precision: u32) -> Result<Complex> {
        match self {
            Self::Number(real, imaginary, _) => Ok(Complex::with_val(precision, (real, imaginary))),
            _ => Err(error("Expected a number.")),
        }
    }

    fn from_complex(value: Complex, unit: Option<String>) -> Self {
        Self::Number(value.real().clone(), value.imag().clone(), unit)
    }
}

pub(crate) fn constant(name: &str, precision: u32) -> Option<Value> {
    let real = match name {
        "pi" | "π" => Float::with_val(precision, Constant::Pi),
        "tau" | "τ" => Float::with_val(precision, Constant::Pi) * 2,
        "e" => Float::with_val(precision, 1).exp(),
        "phi" | "ϕ" | "φ" => (Float::with_val(precision, 5).sqrt() + 1) / 2,
        "i" => {
            return Some(Value::Number(
                Float::new(precision),
                Float::with_val(precision, 1),
                None,
            ));
        }
        "true" => return Some(Value::Boolean(true)),
        "false" => return Some(Value::Boolean(false)),
        _ => return None,
    };
    Some(Value::number(precision, real))
}

// Comparisons use the established absolute threshold, including its exact
// binary value. Spelling that value in decimal avoids an f64 arithmetic path.
const COMPARISON_TOLERANCE: &str =
    "0.000000010000000000000000209225608301284726753266340892878361046314239501953125";

fn close(a: &Float, b: &Float, precision: u32) -> bool {
    Float::with_val(precision, a - b).abs()
        < Float::with_val(precision, Float::parse(COMPARISON_TOLERANCE).unwrap())
}

fn equal(a: &Value, b: &Value, precision: u32) -> bool {
    compare_equality(a, b, precision, false, &mut |_, _| Ok(None)).unwrap_or(false)
}

fn compare_equality(
    a: &Value,
    b: &Value,
    precision: u32,
    unequal: bool,
    convert: &mut impl FnMut(&Value, &Value) -> Result<Option<Value>>,
) -> Result<bool> {
    let converted = convert(a, b)?;
    let b = converted.as_ref().unwrap_or(b);
    match (a, b) {
        (Value::Number(ar, ai, au), Value::Number(br, bi, bu)) => {
            if au != bu {
                return Ok(unequal);
            }
            if unequal {
                let threshold =
                    Float::with_val(precision, Float::parse(COMPARISON_TOLERANCE).unwrap());
                Ok(Float::with_val(precision, ar - br).abs() >= threshold
                    || Float::with_val(precision, ai - bi).abs() >= threshold)
            } else {
                Ok(close(ar, br, precision) && close(ai, bi, precision))
            }
        }
        (Value::Boolean(a), Value::Boolean(b)) => Ok((a == b) != unequal),
        (Value::Vector(a), Value::Vector(b)) => compare_sequence(a, b, precision, unequal, convert),
        (Value::Matrix(a), Value::Matrix(b)) => {
            if a.len() != b.len() {
                return Ok(unequal);
            }
            for (a, b) in a.iter().zip(b) {
                if compare_sequence(a, b, precision, unequal, convert)? == unequal {
                    return Ok(unequal);
                }
            }
            Ok(!unequal)
        }
        _ => Err(error("Equality requires compatible value types.")),
    }
}

fn compare_sequence(
    a: &[Value],
    b: &[Value],
    precision: u32,
    unequal: bool,
    convert: &mut impl FnMut(&Value, &Value) -> Result<Option<Value>>,
) -> Result<bool> {
    if a.len() != b.len() {
        return Ok(unequal);
    }
    for (a, b) in a.iter().zip(b) {
        if compare_equality(a, b, precision, unequal, convert)? == unequal {
            return Ok(unequal);
        }
    }
    Ok(!unequal)
}

fn order(a: &Value, b: &Value) -> Result<Ordering> {
    a.real()?
        .partial_cmp(b.real()?)
        .ok_or_else(|| error("NaN cannot be ordered."))
}

pub(crate) fn unary(op: &str, value: &Value, precision: u32) -> Result<Value> {
    match op {
        "+" => Ok(value.clone()),
        "-" => binary("*", &Value::number(precision, -1), value, precision),
        "not" => Ok(Value::Boolean(!value.truthy()?)),
        "%" => binary("/", value, &Value::number(precision, 100), precision),
        "!" => map_numeric(value, &|v| {
            let Value::Number(real, _, unit) = v else {
                return Err(error("Expected a number."));
            };
            Ok(Value::Number(
                (Float::with_val(precision, real) + 1u32).gamma(),
                Float::new(precision),
                unit.clone(),
            ))
        }),
        "transpose" => transpose(value),
        _ => Err(error(format!("Unknown unary operator {op}."))),
    }
}

fn map_numeric(value: &Value, f: &impl Fn(&Value) -> Result<Value>) -> Result<Value> {
    match value {
        Value::Vector(values) => Ok(Value::Vector(
            values
                .iter()
                .map(|v| map_numeric(v, f))
                .collect::<Result<_>>()?,
        )),
        Value::Matrix(rows) => Ok(Value::Matrix(
            rows.iter()
                .map(|row| row.iter().map(|v| map_numeric(v, f)).collect())
                .collect::<Result<_>>()?,
        )),
        _ => f(value),
    }
}

fn shape(rows: &[Vec<Value>]) -> Result<(usize, usize)> {
    let width = rows.first().map_or(0, Vec::len);
    if rows.iter().any(|row| row.len() != width) {
        return Err(error("Matrix rows must have equal lengths."));
    }
    Ok((rows.len(), width))
}

fn dot(a: &[Value], b: &[Value], precision: u32) -> Result<Value> {
    if a.len() != b.len() {
        return Err(error("Vector dimensions do not match."));
    }
    let mut total = Value::number(precision, 0);
    for (a, b) in a.iter().zip(b) {
        total = binary("+", &total, &binary("*", a, b, precision)?, precision)?;
    }
    Ok(total)
}

pub(crate) fn binary(op: &str, a: &Value, b: &Value, precision: u32) -> Result<Value> {
    binary_with_conversion(op, a, b, precision, &mut |_, _| Ok(None))
}

// Conversion belongs to the request's engine; collection shape and broadcasting
// stay here so unit-aware operations follow exactly the same dispatch as numbers.
pub(crate) fn binary_with_conversion(
    op: &str,
    a: &Value,
    b: &Value,
    precision: u32,
    convert: &mut impl FnMut(&Value, &Value) -> Result<Option<Value>>,
) -> Result<Value> {
    if matches!(op, "=" | "==" | "!=" | "≠") {
        return Ok(Value::Boolean(compare_equality(
            a,
            b,
            precision,
            matches!(op, "!=" | "≠"),
            convert,
        )?));
    }
    let converted = convert(a, b)?;
    let b = converted.as_ref().unwrap_or(b);
    if matches!(op, "<" | ">" | "<=" | ">=" | "≤" | "≥") {
        let a = a.real()?;
        let b = b.real()?;
        let equal = close(a, b, precision);
        return Ok(Value::Boolean(match op {
            "<" => a < b && !equal,
            ">" => a > b && !equal,
            "<=" | "≤" => a < b || equal,
            _ => a > b || equal,
        }));
    }
    if matches!(op, "and" | "or") {
        return Ok(Value::Boolean(if op == "and" {
            a.truthy()? && b.truthy()?
        } else {
            a.truthy()? || b.truthy()?
        }));
    }
    match (a, b) {
        (Value::Matrix(rows), Value::Number(_, _, _)) if op == "^" => {
            matrix_power(rows, b, precision)
        }
        (Value::Vector(a), Value::Vector(b)) if op == "*" => dot(a, b, precision),
        (Value::Matrix(a), Value::Matrix(b)) if op == "*" => {
            let (height, shared) = shape(a)?;
            let (other_height, width) = shape(b)?;
            if shared != other_height {
                return Err(error("Matrix product dimensions do not match."));
            }
            let mut result = Vec::with_capacity(height);
            for row in a {
                let mut target = Vec::with_capacity(width);
                for column in 0..width {
                    let rhs: Vec<_> = b.iter().map(|r| r[column].clone()).collect();
                    target.push(dot(row, &rhs, precision)?);
                }
                result.push(target);
            }
            Ok(Value::Matrix(result))
        }
        (Value::Matrix(rows), Value::Vector(vector)) if op == "*" => {
            shape(rows)?;
            Ok(Value::Vector(
                rows.iter()
                    .map(|row| dot(row, vector, precision))
                    .collect::<Result<_>>()?,
            ))
        }
        (Value::Vector(vector), Value::Matrix(rows)) if op == "*" => {
            shape(rows)?;
            Ok(Value::Vector(
                rows.iter()
                    .map(|row| dot(vector, row, precision))
                    .collect::<Result<_>>()?,
            ))
        }
        (Value::Vector(a), Value::Vector(b)) => {
            if a.len() != b.len() {
                return Err(error("Vector dimensions do not match."));
            }
            Ok(Value::Vector(
                a.iter()
                    .zip(b)
                    .map(|(a, b)| binary_with_conversion(op, a, b, precision, convert))
                    .collect::<Result<_>>()?,
            ))
        }
        (Value::Matrix(a), Value::Matrix(b)) => {
            if shape(a)? != shape(b)? {
                return Err(error("Matrix dimensions do not match."));
            }
            Ok(Value::Matrix(
                a.iter()
                    .zip(b)
                    .map(|(a, b)| {
                        a.iter()
                            .zip(b)
                            .map(|(a, b)| binary_with_conversion(op, a, b, precision, convert))
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ))
        }
        (Value::Matrix(rows), Value::Vector(values)) => {
            shape(rows)?;
            if rows.len() != values.len() {
                return Err(error("Matrix row and vector dimensions do not match."));
            }
            Ok(Value::Matrix(
                rows.iter()
                    .zip(values)
                    .map(|(row, b)| {
                        row.iter()
                            .map(|a| binary_with_conversion(op, a, b, precision, convert))
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ))
        }
        (Value::Vector(values), Value::Matrix(rows)) => {
            shape(rows)?;
            if rows.len() != values.len() {
                return Err(error("Matrix row and vector dimensions do not match."));
            }
            Ok(Value::Matrix(
                rows.iter()
                    .zip(values)
                    .map(|(row, a)| {
                        row.iter()
                            .map(|b| binary_with_conversion(op, a, b, precision, convert))
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ))
        }
        (Value::Vector(values), _) => Ok(Value::Vector(
            values
                .iter()
                .map(|a| binary_with_conversion(op, a, b, precision, convert))
                .collect::<Result<_>>()?,
        )),
        (_, Value::Vector(values)) => Ok(Value::Vector(
            values
                .iter()
                .map(|b| binary_with_conversion(op, a, b, precision, convert))
                .collect::<Result<_>>()?,
        )),
        (Value::Matrix(rows), _) => {
            shape(rows)?;
            Ok(Value::Matrix(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|a| binary_with_conversion(op, a, b, precision, convert))
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ))
        }
        (_, Value::Matrix(rows)) => {
            shape(rows)?;
            Ok(Value::Matrix(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|b| binary_with_conversion(op, a, b, precision, convert))
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ))
        }
        (Value::Number(ar, ai, au), Value::Number(br, bi, bu)) => {
            let unit = bu.clone().or_else(|| au.clone());
            if matches!(op, "<<" | ">>") {
                let factor = Float::with_val(precision, 2).pow(b.real()?);
                let factor = Value::number(precision, factor);
                return binary(if op == "<<" { "*" } else { "/" }, a, &factor, precision);
            }
            if matches!(op, "%" | "mod") {
                if !ai.is_zero() || !bi.is_zero() {
                    return Err(error("Remainder requires real numbers."));
                }
                return Ok(Value::Number(
                    Float::with_val(precision, ar % br),
                    Float::new(precision),
                    unit,
                ));
            }
            if ai.is_zero() && bi.is_zero() && (op != "^" || ar >= &0 || br.is_integer()) {
                let value = match op {
                    "+" => Float::with_val(precision, ar + br),
                    "-" => Float::with_val(precision, ar - br),
                    "*" => Float::with_val(precision, ar * br),
                    "/" => Float::with_val(precision, ar / br),
                    "^" | "**" => Float::with_val(precision, ar).pow(br),
                    _ => return Err(error(format!("Unknown numeric operator {op}."))),
                };
                return Ok(Value::Number(value, Float::new(precision), unit));
            }
            let lhs = a.complex(precision)?;
            let rhs = b.complex(precision)?;
            let value = match op {
                "+" => lhs + rhs,
                "-" => lhs - rhs,
                "*" => lhs * rhs,
                "/" => lhs / rhs,
                "^" | "**" => lhs.pow(rhs),
                _ => return Err(error(format!("Unknown numeric operator {op}."))),
            };
            Ok(Value::from_complex(value, unit))
        }
        _ => Err(error("This operator requires numeric operands.")),
    }
}

fn matrix_power(rows: &[Vec<Value>], exponent: &Value, precision: u32) -> Result<Value> {
    let (height, width) = shape(rows)?;
    if height != width {
        return Err(error("Matrix powers require a square matrix."));
    }
    let real = exponent.real()?;
    if !real.is_integer() || real < &0 {
        return Err(error("Matrix power must be a nonnegative integer."));
    }
    let mut count = real
        .to_u32_saturating()
        .filter(|n| *n <= 1_000_000)
        .ok_or_else(|| error("Matrix power is too large."))?;
    // The language's zero matrix power has historically been the scalar one.
    if count == 0 {
        return Ok(Value::number(precision, 1));
    }
    let mut factor = Value::Matrix(rows.to_vec());
    let mut result: Option<Value> = None;
    while count > 0 {
        if count & 1 == 1 {
            result = Some(match result {
                Some(previous) => binary("*", &previous, &factor, precision)?,
                None => factor.clone(),
            });
        }
        count >>= 1;
        if count > 0 {
            factor = binary("*", &factor, &factor, precision)?;
        }
    }
    Ok(result.unwrap())
}

pub(crate) const BUILTIN_NAMES: &[&str] = &[
    "sin",
    "cos",
    "tan",
    "csc",
    "sec",
    "cot",
    "asin",
    "acos",
    "atan",
    "acsc",
    "asec",
    "acot",
    "sinh",
    "cosh",
    "tanh",
    "csch",
    "sech",
    "coth",
    "asinh",
    "acosh",
    "atanh",
    "acsch",
    "asech",
    "acoth",
    "sqrt",
    "√",
    "cbrt",
    "∛",
    "root",
    "exp",
    "ln",
    "log",
    "abs",
    "ceil",
    "floor",
    "round",
    "trunc",
    "frac",
    "sgn",
    "Re",
    "Im",
    "arg",
    "gamma",
    "Γ",
    "hypot",
    "gcd",
    "lcm",
    "nCr",
    "comb",
    "nPr",
    "perm",
    "bitcmp",
    "bitand",
    "bitor",
    "bitxor",
    "bitshift",
    "iverson",
    "average",
    "min",
    "max",
    "sum",
    "prod",
    "length",
    "sort",
    "append",
    "perms",
    "permutations",
    "matrix",
    "diag",
    "transpose",
    "trace",
    "det",
    "determinant",
    "integrate",
    "integral",
    "∫",
    "Σ",
    "∑",
    "∏",
];

pub(crate) fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

fn arity(name: &str, arguments: &[Value], expected: usize) -> Result<()> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(error(format!(
            "{name} expects {expected} argument(s), received {}.",
            arguments.len()
        )))
    }
}

pub(crate) fn builtin(
    name: &str,
    arguments: Vec<Value>,
    precision: u32,
    degrees: bool,
) -> Result<Value> {
    match name {
        "sum" | "prod" | "average" | "min" | "max" => return reduce(name, arguments, precision),
        "length" => {
            arity(name, &arguments, 1)?;
            return match &arguments[0] {
                Value::Vector(values) => Ok(Value::number(precision, values.len())),
                Value::Matrix(rows) => Ok(Value::number(precision, rows.len())),
                _ => Err(error("length expects a vector or matrix.")),
            };
        }
        "append" => {
            arity(name, &arguments, 2)?;
            let Value::Vector(mut values) = arguments[0].clone() else {
                return Err(error("append expects a vector as its first argument."));
            };
            values.push(arguments[1].clone());
            return Ok(Value::Vector(values));
        }
        "sort" => {
            arity(name, &arguments, 1)?;
            let Value::Vector(mut values) = arguments[0].clone() else {
                return Err(error("sort expects a vector."));
            };
            for value in &values {
                if !value.real()?.is_finite() {
                    return Err(error("sort requires finite real values."));
                }
            }
            values.sort_by(|a, b| order(a, b).unwrap_or(Ordering::Equal));
            return Ok(Value::Vector(values));
        }
        "perms" | "permutations" => {
            arity(name, &arguments, 1)?;
            return permutations(&arguments[0], precision);
        }
        "matrix" | "diag" | "transpose" | "trace" | "det" | "determinant" => {
            arity(name, &arguments, 1)?;
            return collection_builtin(name, &arguments[0], precision);
        }
        "iverson" => {
            arity(name, &arguments, 1)?;
            return Ok(Value::number(
                precision,
                if matches!(arguments[0], Value::Boolean(false)) {
                    0
                } else {
                    1
                },
            ));
        }
        "root" | "hypot" | "gcd" | "lcm" | "nCr" | "comb" | "nPr" | "perm" | "bitand" | "bitor"
        | "bitxor" | "bitshift" => {
            arity(name, &arguments, 2)?;
            return binary_builtin(name, &arguments[0], &arguments[1], precision, degrees);
        }
        "log" if arguments.len() == 2 => {
            let lhs = builtin("ln", vec![arguments[0].clone()], precision, degrees)?;
            let rhs = builtin("ln", vec![arguments[1].clone()], precision, degrees)?;
            return binary("/", &lhs, &rhs, precision);
        }
        _ => {}
    }
    if !is_builtin(name) {
        return Err(error(format!("Unknown function {name}.")));
    }
    arity(name, &arguments, 1)?;
    map_numeric(&arguments[0], &|value| {
        scalar_builtin(name, value, precision, degrees)
    })
}

fn scalar_builtin(name: &str, value: &Value, precision: u32, degrees: bool) -> Result<Value> {
    let Value::Number(real, imaginary, unit) = value else {
        return Err(error(format!("{name} expects a number.")));
    };
    let real = Float::with_val(precision, real);
    let imaginary = Float::with_val(precision, imaginary);
    let component = match name {
        "Re" => Some((real.clone(), Float::new(precision))),
        "Im" => Some((imaginary.clone(), Float::new(precision))),
        "ceil" => Some((real.clone().ceil(), imaginary.clone().ceil())),
        "floor" => Some((real.clone().floor(), imaginary.clone().floor())),
        "round" => Some((real.clone().round(), imaginary.clone().round())),
        "trunc" => Some((real.clone().trunc(), imaginary.clone().trunc())),
        "frac" => Some((real.clone().fract(), imaginary.clone().fract())),
        "gamma" | "Γ" => Some((real.clone().gamma(), Float::new(precision))),
        "cbrt" | "∛" => Some((real.clone().cbrt(), Float::new(precision))),
        "bitcmp" => return Ok(Value::number(precision, !bits(value)?)),
        _ => None,
    };
    if let Some((real, imaginary)) = component {
        return Ok(Value::Number(real, imaginary, unit.clone()));
    }
    let direct_angle = matches!(
        name,
        "sin"
            | "cos"
            | "tan"
            | "csc"
            | "sec"
            | "cot"
            | "sinh"
            | "cosh"
            | "tanh"
            | "csch"
            | "sech"
            | "coth"
    );
    let inverse_angle = matches!(
        name,
        "asin"
            | "acos"
            | "atan"
            | "acsc"
            | "asec"
            | "acot"
            | "asinh"
            | "acosh"
            | "atanh"
            | "acsch"
            | "asech"
            | "acoth"
    );
    let mut z = Complex::with_val(precision, (&real, &imaginary));
    if direct_angle && (unit.as_deref() == Some("deg") || degrees && unit.as_deref() != Some("rad"))
    {
        // The established angle convention converts the real component only.
        let converted = real.clone() * Float::with_val(precision, Constant::Pi) / 180;
        z = Complex::with_val(precision, (converted, imaginary.clone()));
    }
    let mut output = match name {
        "sqrt" | "√" => z.sqrt(),
        "exp" => z.exp(),
        "ln" => z.ln(),
        "log" => z.ln() / Float::with_val(precision, 10).ln(),
        "abs" => z.abs(),
        "arg" => z.arg(),
        "sgn" => {
            if value.is_zero() {
                Complex::with_val(precision, 0)
            } else {
                z.clone() / z.abs()
            }
        }
        "sin" => z.sin(),
        "cos" => z.cos(),
        "tan" => z.tan(),
        "csc" => z.sin().recip(),
        "sec" => z.cos().recip(),
        "cot" => z.tan().recip(),
        "asin" | "acos" | "atan" | "acsc" | "asec" | "acot" => inverse_function(name, z, precision),
        "sinh" => z.sinh(),
        "cosh" => z.cosh(),
        "tanh" => z.tanh(),
        "csch" => z.sinh().recip(),
        "sech" => z.cosh().recip(),
        "coth" => z.tanh().recip(),
        "asinh" | "acosh" | "atanh" | "acsch" | "asech" | "acoth" => {
            inverse_function(name, z, precision)
        }
        _ => return Err(error(format!("Unsupported numeric function {name}."))),
    };
    let result_unit = if inverse_angle && degrees {
        let converted = Float::with_val(precision, output.real()) * 180
            / Float::with_val(precision, Constant::Pi);
        output = Complex::with_val(precision, (converted, output.imag()));
        Some("deg".into())
    } else if direct_angle || inverse_angle {
        None
    } else {
        unit.clone()
    };
    Ok(Value::from_complex(output, result_unit))
}

// Values exactly on an inverse function's branch cut use the language's
// historical side of the cut, independent of incidental signed zeroes.
fn inverse_function(name: &str, mut z: Complex, precision: u32) -> Complex {
    let base = match name {
        "acsc" => "asin",
        "asec" => "acos",
        "acot" => "atan",
        "acsch" => "asinh",
        "asech" => "acosh",
        "acoth" => "atanh",
        _ => name,
    };
    if base != name {
        z = z.recip();
    }
    let real_cut = z.imag().is_zero() && z.real().clone().abs() > 1;
    let imaginary_cut = z.real().is_zero() && z.imag().clone().abs() > 1;
    let result = match base {
        "asin" => z.clone().asin(),
        "acos" => z.clone().acos(),
        "atan" => z.clone().atan(),
        "asinh" => z.clone().asinh(),
        "acosh" => z.clone().acosh(),
        "atanh" => z.clone().atanh(),
        _ => unreachable!(),
    };
    if real_cut && matches!(base, "asin" | "acos" | "atanh") {
        let mut imaginary = result.imag().clone().abs();
        if (z.real() > &0) != (base == "acos") {
            imaginary = -imaginary;
        }
        return Complex::with_val(precision, (result.real(), imaginary));
    }
    if imaginary_cut && matches!(base, "atan" | "asinh") {
        let mut real = result.real().clone().abs();
        if z.imag() < &0 {
            real = -real;
        }
        return Complex::with_val(precision, (real, result.imag()));
    }
    result
}

fn bits(value: &Value) -> Result<i32> {
    value
        .real()?
        .to_i32_saturating_round(Round::Nearest)
        .ok_or_else(|| error("Bit operations require finite numbers."))
}

fn integer(value: &Value) -> Result<Integer> {
    let real = value.real()?;
    if !real.is_integer() {
        return Err(error("Expected an integer."));
    }
    real.to_integer()
        .ok_or_else(|| error("Expected a finite integer."))
}

fn binary_builtin(
    name: &str,
    a: &Value,
    b: &Value,
    precision: u32,
    degrees: bool,
) -> Result<Value> {
    match name {
        "root" => binary(
            "^",
            a,
            &binary("/", &Value::number(precision, 1), b, precision)?,
            precision,
        ),
        "hypot" => {
            let aa = binary("*", a, a, precision)?;
            let bb = binary("*", b, b, precision)?;
            builtin(
                "sqrt",
                vec![binary("+", &aa, &bb, precision)?],
                precision,
                degrees,
            )
        }
        "gcd" => gcd(a, b, precision),
        "lcm" => {
            if a.is_zero() || b.is_zero() {
                return Ok(Value::number(precision, 0));
            }
            let product = binary("*", a, b, precision)?;
            let factor = gcd(a, b, precision)?;
            builtin(
                "abs",
                vec![binary("/", &product, &factor, precision)?],
                precision,
                degrees,
            )
        }
        "nCr" | "comb" | "nPr" | "perm" => {
            let n = integer(a)?;
            let r = integer(b)?;
            if n < 0 || r < 0 || r > n {
                return Err(error("Combinations and permutations require 0 <= r <= n."));
            }
            let combinations = matches!(name, "nCr" | "comb");
            let r = if combinations {
                r.clone().min(Integer::from(&n - &r))
            } else {
                r
            };
            let count = r
                .to_u32()
                .filter(|n| *n <= 100_000)
                .ok_or_else(|| error("Combination count is too large."))?;
            let mut result = Integer::from(1);
            for index in 0..count {
                result *= Integer::from(&n - index);
                if combinations {
                    result /= index + 1;
                }
            }
            Ok(Value::number(precision, result))
        }
        "bitand" => Ok(Value::number(precision, bits(a)? & bits(b)?)),
        "bitor" => Ok(Value::number(precision, bits(a)? | bits(b)?)),
        "bitxor" => Ok(Value::number(precision, bits(a)? ^ bits(b)?)),
        "bitshift" => {
            let value = bits(a)?;
            let shift = bits(b)?;
            if !(-31..=31).contains(&shift) {
                return Err(error("Bit shift must be between -31 and 31."));
            }
            Ok(Value::number(
                precision,
                if shift >= 0 {
                    value.wrapping_shl(shift as u32)
                } else {
                    value >> shift.unsigned_abs()
                },
            ))
        }
        _ => Err(error(format!("Unsupported binary function {name}."))),
    }
}

fn gcd(a: &Value, b: &Value, precision: u32) -> Result<Value> {
    if let (Ok(a), Ok(b)) = (a.real(), b.real()) {
        if !a.is_integer() || !b.is_integer() {
            return Err(error("gcd requires integers."));
        }
        let a = a
            .to_integer()
            .ok_or_else(|| error("gcd requires finite integers."))?;
        let b = b
            .to_integer()
            .ok_or_else(|| error("gcd requires finite integers."))?;
        return Ok(Value::number(precision, a.gcd(&b)));
    }
    let mut x = a.complex(precision)?;
    let mut y = b.complex(precision)?;
    if [x.real(), x.imag(), y.real(), y.imag()]
        .iter()
        .any(|n| !n.is_integer())
    {
        return Err(error("Complex gcd requires Gaussian integers."));
    }
    for _ in 0..10_000 {
        if y.real().is_zero() && y.imag().is_zero() {
            for _ in 0..4 {
                if x.real() >= &0 && x.imag() >= &0 {
                    break;
                }
                x *= Complex::with_val(precision, (0, 1));
            }
            return Ok(Value::from_complex(x, None));
        }
        let quotient = x.clone() / &y;
        let rounded = Complex::with_val(
            precision,
            (
                quotient.real().clone().round(),
                quotient.imag().clone().round(),
            ),
        );
        let remainder = x - rounded * &y;
        x = y;
        y = remainder;
    }
    Err(error("Gaussian gcd did not converge."))
}

fn reduce(name: &str, arguments: Vec<Value>, precision: u32) -> Result<Value> {
    reduce_with_conversion(name, arguments, precision, &mut |_, _| Ok(None))
}

pub(crate) fn reduce_with_conversion(
    name: &str,
    arguments: Vec<Value>,
    precision: u32,
    convert: &mut impl FnMut(&Value, &Value) -> Result<Option<Value>>,
) -> Result<Value> {
    let values = if arguments.len() == 1 {
        match &arguments[0] {
            Value::Vector(values) => values.clone(),
            Value::Matrix(_) => {
                return if matches!(name, "sum" | "prod") {
                    Ok(arguments[0].clone())
                } else {
                    Err(error(format!("{name} expects real arguments or a vector.")))
                };
            }
            _ => arguments,
        }
    } else {
        arguments
    };
    if values.is_empty() {
        return Err(error(format!("{name} requires at least one value.")));
    }
    if matches!(name, "min" | "max") {
        let mut selected = values[0].clone();
        for value in values.iter().skip(1) {
            let converted = convert(&selected, value)?;
            let ordering = order(converted.as_ref().unwrap_or(value), &selected)?;
            if (name == "min" && ordering.is_lt()) || (name == "max" && ordering.is_gt()) {
                selected = value.clone();
            }
        }
        return Ok(selected);
    }
    let mut result = Value::number(precision, if name == "prod" { 1 } else { 0 });
    for value in &values {
        result = binary_with_conversion(
            if name == "prod" { "*" } else { "+" },
            &result,
            value,
            precision,
            convert,
        )?;
    }
    if name == "average" {
        result = binary(
            "/",
            &result,
            &Value::number(precision, values.len()),
            precision,
        )?;
    }
    Ok(result)
}

fn transpose(value: &Value) -> Result<Value> {
    match value {
        Value::Matrix(rows) => {
            let (_, width) = shape(rows)?;
            Ok(Value::Matrix(
                (0..width)
                    .map(|column| rows.iter().map(|row| row[column].clone()).collect())
                    .collect(),
            ))
        }
        Value::Vector(values) => Ok(Value::Matrix(
            values.iter().map(|value| vec![value.clone()]).collect(),
        )),
        _ => Err(error("transpose requires a vector or matrix.")),
    }
}

fn collection_builtin(name: &str, value: &Value, precision: u32) -> Result<Value> {
    if name == "transpose" {
        return transpose(value);
    }
    if name == "matrix" {
        return match value {
            Value::Matrix(rows) => {
                shape(rows)?;
                Ok(value.clone())
            }
            Value::Vector(values) => {
                let rows = values
                    .iter()
                    .map(|v| match v {
                        Value::Vector(row) => Ok(row.clone()),
                        _ => Err(error("matrix expects a vector of row vectors.")),
                    })
                    .collect::<Result<Vec<_>>>()?;
                shape(&rows)?;
                Ok(Value::Matrix(rows))
            }
            _ => Err(error("matrix expects a vector of row vectors.")),
        };
    }
    if name == "diag" {
        let Value::Vector(values) = value else {
            return Err(error("diag expects a vector."));
        };
        if values.len() > 512 {
            return Err(error("Diagonal matrix is too large."));
        }
        let mut rows = vec![vec![Value::number(precision, 0); values.len()]; values.len()];
        for (index, value) in values.iter().enumerate() {
            rows[index][index] = value.clone();
        }
        return Ok(Value::Matrix(rows));
    }
    let Value::Matrix(rows) = value else {
        return Err(error(format!("{name} expects a matrix.")));
    };
    let (height, width) = shape(rows)?;
    if height != width || height == 0 {
        return Err(error(format!("{name} expects a nonempty square matrix.")));
    }
    if name == "trace" {
        let mut result = Value::number(precision, 0);
        for (index, row) in rows.iter().enumerate() {
            result = binary("+", &result, &row[index], precision)?;
        }
        return Ok(result);
    }
    determinant(rows, precision)
}

fn determinant(rows: &[Vec<Value>], precision: u32) -> Result<Value> {
    if rows.len() > 256 {
        return Err(error("Determinant matrix is too large."));
    }
    let mut work = rows.to_vec();
    let mut result = Value::number(precision, 1);
    for column in 0..work.len() {
        // Partial pivoting selects the greatest complex magnitude and keeps the
        // elimination stable without factorial-time minor expansion.
        let mut pivot = column;
        let mut largest = Float::new(precision);
        for (row_index, row) in work.iter().enumerate().skip(column) {
            let magnitude = row[column].complex(precision)?.abs().real().clone();
            if magnitude > largest {
                largest = magnitude;
                pivot = row_index;
            }
        }
        if largest.is_zero() {
            return Ok(Value::number(precision, 0));
        }
        if pivot != column {
            work.swap(pivot, column);
            result = unary("-", &result, precision)?;
        }
        let diagonal = work[column][column].clone();
        result = binary("*", &result, &diagonal, precision)?;
        let pivot_row = work[column].clone();
        for row in work.iter_mut().skip(column + 1) {
            let factor = binary("/", &row[column], &diagonal, precision)?;
            for (entry, pivot_entry) in row.iter_mut().zip(&pivot_row).skip(column + 1) {
                *entry = binary(
                    "-",
                    entry,
                    &binary("*", &factor, pivot_entry, precision)?,
                    precision,
                )?;
            }
            row[column] = Value::number(precision, 0);
        }
    }
    Ok(result)
}

fn permutations(value: &Value, precision: u32) -> Result<Value> {
    let Value::Vector(values) = value else {
        return Err(error("permutations expects a vector."));
    };
    if values.len() > 32 {
        return Err(error("Permutation vector is too large."));
    }
    fn visit(
        values: &[Value],
        used: &mut [bool],
        current: &mut Vec<Value>,
        output: &mut Vec<Vec<Value>>,
        precision: u32,
    ) -> Result<()> {
        if current.len() == values.len() {
            if output.len() >= 10_000 {
                return Err(error("Permutation output exceeds 10000 rows."));
            }
            output.push(current.clone());
            return Ok(());
        }
        for index in 0..values.len() {
            if used[index]
                || (0..index).any(|previous| {
                    !used[previous] && equal(&values[index], &values[previous], precision)
                })
            {
                continue;
            }
            used[index] = true;
            current.push(values[index].clone());
            visit(values, used, current, output, precision)?;
            current.pop();
            used[index] = false;
        }
        Ok(())
    }
    let mut output = Vec::new();
    visit(
        values,
        &mut vec![false; values.len()],
        &mut Vec::new(),
        &mut output,
        precision,
    )?;
    Ok(Value::Matrix(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_precision(value: &Float, expected: &str, decimal_tolerance: &str) {
        let reference = Float::with_val(value.prec(), Float::parse(expected).unwrap());
        let tolerance = Float::with_val(value.prec(), Float::parse(decimal_tolerance).unwrap());
        assert!(
            Float::with_val(value.prec(), value - reference).abs() < tolerance,
            "{value}"
        );
    }

    #[test]
    fn numeric_comparison_threshold_is_consistent_across_operators() {
        let precision = 256;
        let one = Value::number(precision, 1);
        let nearby = Value::parse("1.00000001", precision).unwrap();
        for (op, expected) in [
            ("=", true),
            ("!=", false),
            ("<", false),
            (">", false),
            ("<=", true),
            (">=", true),
        ] {
            let Value::Boolean(actual) = binary(op, &one, &nearby, precision).unwrap() else {
                panic!()
            };
            assert_eq!(actual, expected, "{op}");
        }
        let farther = Value::parse("1.000000010000000001", precision).unwrap();
        assert!(matches!(
            binary("=", &one, &farther, precision).unwrap(),
            Value::Boolean(false)
        ));
        let zero = Value::number(precision, 0);
        let infinity = binary("/", &one, &zero, precision).unwrap();
        let nan = binary("/", &zero, &zero, precision).unwrap();
        for value in [&infinity, &nan] {
            for op in ["=", "!=", "<", ">", "<=", ">="] {
                assert!(
                    matches!(
                        binary(op, value, value, precision).unwrap(),
                        Value::Boolean(false)
                    ),
                    "{op}"
                );
            }
        }
    }

    #[test]
    fn inverse_function_branch_cuts_keep_the_established_signs() {
        let p = 128;
        for (name, argument, positive) in [
            ("asin", 2, false),
            ("asin", -2, true),
            ("acos", 2, true),
            ("acos", -2, false),
            ("atanh", 2, false),
            ("atanh", -2, true),
        ] {
            let Value::Number(_, imaginary, _) =
                builtin(name, vec![Value::number(p, argument)], p, false).unwrap()
            else {
                panic!()
            };
            assert_eq!(imaginary > 0, positive, "{name}({argument})");
        }
        for name in ["atan", "asinh"] {
            let negative_imaginary = Value::Number(Float::new(p), Float::with_val(p, -2), None);
            let Value::Number(real, imaginary, _) =
                builtin(name, vec![negative_imaginary], p, false).unwrap()
            else {
                panic!()
            };
            assert!(real < 0 && imaginary < 0, "{name}(-2i)");
        }
    }

    #[test]
    fn bit_coercion_rounds_to_nearest_with_even_ties() {
        let p = 128;
        for (argument, expected) in [
            ("2.9", -4),
            ("2.5", -3),
            ("3.5", -5),
            ("-2.5", 1),
            ("-3.5", 3),
        ] {
            assert_eq!(
                builtin("bitcmp", vec![Value::parse(argument, p).unwrap()], p, false)
                    .unwrap()
                    .real()
                    .unwrap(),
                &expected
            );
        }
    }

    #[test]
    fn vector_matrix_products_and_matrix_reductions_preserve_language_behavior() {
        let p = 128;
        let vector = Value::Vector(vec![Value::number(p, 1), Value::number(p, 2)]);
        let matrix = Value::Matrix(vec![
            vec![Value::number(p, 3), Value::number(p, 4)],
            vec![Value::number(p, 5), Value::number(p, 6)],
        ]);
        let expected = Value::Vector(vec![Value::number(p, 11), Value::number(p, 17)]);
        assert!(equal(
            &binary("*", &vector, &matrix, p).unwrap(),
            &expected,
            p
        ));
        assert!(equal(
            &binary("*", &matrix, &vector, p).unwrap(),
            &expected,
            p
        ));
        for name in ["sum", "prod"] {
            assert!(equal(
                &builtin(name, vec![matrix.clone()], p, false).unwrap(),
                &matrix,
                p
            ));
        }
        for name in ["average", "min", "max"] {
            assert!(builtin(name, vec![matrix.clone()], p, false).is_err());
        }
    }

    #[test]
    fn constants_and_complex_transcendentals_retain_requested_precision() {
        let precision = 384;
        let pi = constant("pi", precision).unwrap();
        assert_precision(
            pi.real().unwrap(),
            "3.1415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679",
            "1e-95",
        );
        let z = Value::Number(
            Float::with_val(precision, 2),
            Float::with_val(precision, 3),
            None,
        );
        let logarithm = builtin("ln", vec![z], precision, false).unwrap();
        let Value::Number(real, imaginary, _) =
            builtin("exp", vec![logarithm], precision, false).unwrap()
        else {
            panic!()
        };
        assert_precision(&real, "2", "1e-110");
        assert_precision(&imaginary, "3", "1e-110");
    }

    #[test]
    fn radix_literals_and_cancellation_do_not_round_to_double() {
        let precision = 256;
        let large = Value::parse_radix(&format!("1{}1", "0".repeat(47)), 16, precision).unwrap();
        let power = binary(
            "^",
            &Value::number(precision, 2),
            &Value::number(precision, 192),
            precision,
        )
        .unwrap();
        assert_eq!(
            binary("-", &large, &power, precision)
                .unwrap()
                .real()
                .unwrap(),
            &1
        );
        let fraction =
            Value::parse_radix(&format!("0.{}1", "0".repeat(47)), 16, precision).unwrap();
        assert_eq!(
            binary("*", &fraction, &power, precision)
                .unwrap()
                .real()
                .unwrap(),
            &1
        );
    }

    #[test]
    fn determinant_handles_pivoting_and_complex_entries() {
        let p = 128;
        let matrix = Value::Matrix(vec![
            vec![Value::number(p, 0), Value::number(p, 2)],
            vec![constant("i", p).unwrap(), Value::number(p, 3)],
        ]);
        let Value::Number(real, imaginary, _) = builtin("det", vec![matrix], p, false).unwrap()
        else {
            panic!()
        };
        assert!(real.is_zero());
        assert_eq!(imaginary, -2);
    }

    #[test]
    fn incompatible_shapes_return_errors_before_indexing() {
        let p = 128;
        let ragged = Value::Matrix(vec![vec![Value::number(p, 1)], vec![]]);
        assert!(builtin("transpose", vec![ragged.clone()], p, false).is_err());
        assert!(binary("*", &ragged, &ragged, p).is_err());
        let left = Value::Vector(vec![Value::number(p, 1)]);
        let right = Value::Vector(vec![Value::number(p, 1), Value::number(p, 2)]);
        assert!(binary("*", &left, &right, p).is_err());
        assert!(binary("/", &left, &right, p).is_err());
    }

    #[test]
    fn permutation_enumeration_is_distinct_and_bounded() {
        let p = 128;
        let duplicate = Value::Vector(vec![
            Value::number(p, 1),
            Value::number(p, 1),
            Value::number(p, 2),
        ]);
        let Value::Matrix(rows) = builtin("perms", vec![duplicate], p, false).unwrap() else {
            panic!()
        };
        assert_eq!(rows.len(), 3);
        let too_many = Value::Vector((0..8).map(|n| Value::number(p, n)).collect());
        assert!(builtin("perms", vec![too_many], p, false).is_err());
    }
}
