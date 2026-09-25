//! Evaluation, lexical call scopes, units, and numerical operations for our language.
//!
//! The parser and numerical value operations are independent modules. This layer
//! owns all mutable state for a single request, including its common deadline.
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use super::parser::{self, Expr, Statement};
use super::value::{self, Value};
use crate::calculator::CalcError;

type Result<T> = std::result::Result<T, CalcError>;

#[derive(Clone)]
struct Function {
    parameters: Vec<String>,
    body: Expr,
}

#[derive(Clone)]
struct UnitDefinition {
    parent: String,
    formula: Expr,
}

pub(crate) struct Engine {
    precision: u32,
    degrees: bool,
    variables: HashMap<String, Value>,
    definitions: HashMap<String, Expr>,
    resolving: HashSet<String>,
    functions: HashMap<String, Function>,
    units: HashMap<String, Option<UnitDefinition>>,
    deadline: Instant,
    call_depth: usize,
    expression_depth: usize,
}

fn error(message: impl Into<String>) -> CalcError {
    CalcError::new("calculation_error", message)
}
fn timeout() -> CalcError {
    CalcError::new("timeout", "Operation took too long.")
}

impl Engine {
    pub(crate) fn new(precision: u32, degrees: bool) -> Self {
        Self {
            precision,
            degrees,
            variables: HashMap::new(),
            definitions: HashMap::new(),
            resolving: HashSet::new(),
            functions: HashMap::new(),
            units: HashMap::from([("rad".into(), None), ("deg".into(), None)]),
            deadline: Instant::now()
                + Duration::from_millis(u64::from(crate::calculator::EVALUATION_TIMEOUT_MS)),
            call_depth: 0,
            expression_depth: 0,
        }
    }

    fn check_deadline(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            Err(timeout())
        } else {
            Ok(())
        }
    }

    pub(crate) fn evaluate(&mut self, source: &str) -> Result<Option<Value>> {
        self.check_deadline()?;
        let statements = parser::parse(
            source,
            &self.functions.keys().cloned().collect::<Vec<_>>(),
            &self
                .variables
                .keys()
                .chain(self.definitions.keys())
                .cloned()
                .collect::<Vec<_>>(),
            &self.units.keys().cloned().collect::<Vec<_>>(),
        )?;
        self.check_deadline()?;
        let mut result = None;
        for statement in statements {
            self.check_deadline()?;
            result = match statement {
                Statement::Variable(name, expression) => {
                    if value::constant(&name, self.precision).is_some() {
                        return Err(error("Constants cannot be redefined."));
                    }
                    let mut references = Vec::new();
                    free_names(&expression, &HashSet::new(), &mut references);
                    if references.contains(&name) {
                        return Err(error("Variable references itself."));
                    }
                    self.definitions.insert(name, expression);
                    None
                }
                Statement::Function(name, parameters, body) => {
                    let unique: HashSet<_> = parameters.iter().collect();
                    if unique.len() != parameters.len() {
                        return Err(error("Function parameters must have distinct names."));
                    }
                    self.functions.insert(name, Function { parameters, body });
                    None
                }
                Statement::Unit(name, formula) => {
                    self.define_unit(name, formula)?;
                    None
                }
                Statement::Expression(expression) => {
                    let value = self.eval(&expression)?;
                    Some(value)
                }
            };
        }
        if let Some(value) = &result {
            self.variables.insert("ans".into(), value.clone());
        }
        Ok(result)
    }

    fn eval(&mut self, expression: &Expr) -> Result<Value> {
        self.check_deadline()?;
        if self.expression_depth >= 512 {
            return Err(CalcError::new(
                "recursion_limit",
                "Expression nesting limit exceeded.",
            ));
        }
        self.expression_depth += 1;
        let result = self.eval_inner(expression);
        self.expression_depth -= 1;
        result
    }

    fn eval_inner(&mut self, expression: &Expr) -> Result<Value> {
        match expression {
            Expr::Number(text, radix) => Value::parse_radix(text, *radix, self.precision),
            Expr::Boolean(boolean) => Ok(Value::Boolean(*boolean)),
            Expr::Name(name) => {
                if let Some(value) = self.variables.get(name) {
                    return Ok(value.clone());
                }
                if let Some(expression) = self.definitions.get(name).cloned() {
                    if !self.resolving.insert(name.clone()) {
                        return Err(error("Variable references itself."));
                    }
                    let saved = std::mem::take(&mut self.variables);
                    if let Some(ans) = saved.get("ans") {
                        self.variables.insert("ans".into(), ans.clone());
                    }
                    let result = self.eval(&expression);
                    self.variables = saved;
                    self.resolving.remove(name);
                    return result;
                }
                if let Some(value) = value::constant(name, self.precision) {
                    return Ok(value);
                }
                if self.units.contains_key(name) {
                    return self.number(1).with_unit(Some(name.clone()));
                }
                Err(error(format!("Unknown variable '{name}'.")))
            }
            Expr::Group(inner) => self.eval(inner),
            Expr::Unary(op, inner) => {
                let value = self.eval(inner)?;
                value::unary(op, &value, self.precision)
            }
            Expr::Binary(op, left, right) => {
                if op == "="
                    && !contains_equation(left)
                    && !contains_equation(right)
                    && !self.unknown_names(expression).is_empty()
                {
                    return self.solve_equations(std::slice::from_ref(expression));
                }
                let left = self.eval(left)?;
                if op == "and" && !left.truthy()? {
                    return Ok(Value::Boolean(false));
                }
                if op == "or" && left.truthy()? {
                    return Ok(Value::Boolean(true));
                }
                let mut evaluated_right = self.eval(right)?;
                if (op == "+" || op == "-")
                    && matches!(right.as_ref(), Expr::Unary(percent, _) if percent == "%")
                {
                    evaluated_right = value::binary("*", &left, &evaluated_right, self.precision)?;
                }
                self.binary(op, &left, &evaluated_right)
            }
            Expr::Call(name, arguments) => self.call(name, arguments),
            Expr::Derivative(name, order, arguments) => self.derivative(name, *order, arguments),
            Expr::Vector(expressions) => {
                let values = expressions
                    .iter()
                    .map(|item| self.eval(item))
                    .collect::<Result<_>>()?;
                Ok(Value::Vector(values))
            }
            Expr::Matrix(rows) => {
                let columns = rows.first().map_or(0, Vec::len);
                if rows.iter().any(|row| row.len() != columns) {
                    return Err(error("Matrix rows must have equal lengths."));
                }
                let values = rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|item| self.eval(item))
                            .collect::<Result<_>>()
                    })
                    .collect::<Result<_>>()?;
                Ok(Value::Matrix(values))
            }
            Expr::Index(expression, indices) => {
                let value = self.eval(expression)?;
                let indices = indices
                    .iter()
                    .map(|index| {
                        let value = self.eval(index)?.as_f64()?;
                        if !value.is_finite()
                            || value < 1.0
                            || value.fract() != 0.0
                            || value > usize::MAX as f64
                        {
                            return Err(error("Indices must be positive integers."));
                        }
                        Ok(value as usize - 1)
                    })
                    .collect::<Result<Vec<_>>>()?;
                match (&value, indices.as_slice()) {
                    (Value::Vector(values), [index]) => values.get(*index).cloned(),
                    (Value::Matrix(rows), [row, column]) => {
                        rows.get(*row).and_then(|row| row.get(*column)).cloned()
                    }
                    (Value::Matrix(rows), [row]) => {
                        rows.get(*row).map(|row| Value::Vector(row.clone()))
                    }
                    _ => return Err(error("Invalid number of indices for this value.")),
                }
                .ok_or_else(|| error("Index is outside the collection."))
            }
            Expr::Piecewise(branches) => {
                for (value, condition) in branches {
                    if let Some(condition) = condition
                        && !self.eval(condition)?.truthy()?
                    {
                        continue;
                    }
                    return self.eval(value);
                }
                Err(error("No piecewise condition matched."))
            }
            Expr::Comprehension(body, conditions) => self.comprehension(body, conditions),
            Expr::Unit(expression, name) => self.eval(expression)?.with_unit(Some(name.clone())),
            Expr::Convert(expression, name) => {
                let value = self.eval(expression)?;
                self.convert(&value, name)
            }
            Expr::Equations(equations) => self.solve_equations(equations),
        }
    }

    fn number(&self, number: i32) -> Value {
        Value::number(self.precision, number)
    }
    fn float(&self, number: f64) -> Value {
        Value::number(self.precision, number)
    }

    fn binary(&mut self, op: &str, left: &Value, right: &Value) -> Result<Value> {
        let mut right = right.clone();
        if matches!(op, "+" | "-" | "=" | "!=" | "<" | "<=" | ">" | ">=")
            && let (Value::Number(_, _, Some(left_unit)), Value::Number(_, _, Some(right_unit))) =
                (left, &right)
            && left_unit != right_unit
        {
            right = self.convert(&right, left_unit)?;
        }
        value::binary(op, left, &right, self.precision)
    }

    fn call(&mut self, name: &str, arguments: &[Expr]) -> Result<Value> {
        match name {
            "sum" | "Σ" | "∑" | "prod" | "∏"
                if arguments.len() == 3
                    && matches!(&arguments[0], Expr::Binary(op, _, _) if op == "=") =>
            {
                return self.aggregate(name, arguments);
            }
            "integrate" | "integral" | "∫" => return self.integrate(arguments),
            _ => {}
        }
        let values = arguments
            .iter()
            .map(|argument| self.eval(argument))
            .collect::<Result<Vec<_>>>()?;
        self.call_values(name, values)
    }

    fn call_values(&mut self, name: &str, arguments: Vec<Value>) -> Result<Value> {
        self.check_deadline()?;
        if value::is_builtin(name) {
            return value::builtin(name, arguments, self.precision, self.degrees);
        }
        if let Some(function) = self.functions.get(name).cloned() {
            if function.parameters.len() != arguments.len() {
                return Err(error(format!(
                    "Function '{name}' expects {} arguments, received {}.",
                    function.parameters.len(),
                    arguments.len()
                )));
            }
            if self.call_depth >= crate::calculator::MAX_RECURSION_DEPTH as usize {
                return Err(CalcError::new(
                    "recursion_limit",
                    "Function recursion limit exceeded.",
                ));
            }
            self.call_depth += 1;
            let bindings = function
                .parameters
                .into_iter()
                .zip(arguments)
                .collect::<Vec<_>>();
            let saved = std::mem::take(&mut self.variables);
            if let Some(ans) = saved.get("ans") {
                self.variables.insert("ans".into(), ans.clone());
            }
            let result = self.with_bindings(bindings, |engine| engine.eval(&function.body));
            self.variables = saved;
            self.call_depth -= 1;
            result
        } else {
            value::builtin(name, arguments, self.precision, self.degrees)
        }
    }

    // Restore outer values even when an inner evaluation fails. Repeated nested
    // sums and recursive calls must never leak their temporary parameters.
    fn with_bindings<T>(
        &mut self,
        bindings: Vec<(String, Value)>,
        run: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let saved = bindings
            .into_iter()
            .map(|(name, value)| {
                let previous = self.variables.insert(name.clone(), value);
                (name, previous)
            })
            .collect::<Vec<_>>();
        let result = run(self);
        for (name, previous) in saved.into_iter().rev() {
            if let Some(previous) = previous {
                self.variables.insert(name, previous);
            } else {
                self.variables.remove(&name);
            }
        }
        result
    }

    fn aggregate(&mut self, name: &str, arguments: &[Expr]) -> Result<Value> {
        let Expr::Binary(_, variable, start) = &arguments[0] else {
            unreachable!()
        };
        let Expr::Name(variable) = variable.as_ref() else {
            return Err(error("A sum or product needs a variable bound."));
        };
        let start = self.eval(start)?;
        let end = self.eval(&arguments[1])?;
        let start = start.real()?;
        let end = end.real()?;
        if !start.is_finite() || !end.is_finite() || !start.is_integer() || !end.is_integer() {
            return Err(error("Sum and product limits must be finite integers."));
        }
        // Avoid a massive native allocation when converting an extreme exponent
        // into an integer, while preserving bounds beyond f64's exact range.
        if start.get_exp().is_some_and(|exp| exp > 65_536)
            || end.get_exp().is_some_and(|exp| exp > 65_536)
        {
            return Err(timeout());
        }
        let mut index = start
            .to_integer()
            .ok_or_else(|| error("Invalid lower limit."))?;
        let end = end
            .to_integer()
            .ok_or_else(|| error("Invalid upper limit."))?;
        let product = matches!(name, "prod" | "∏");
        let mut result = self.number(if product { 1 } else { 0 });
        self.with_bindings(vec![(variable.clone(), self.number(0))], |engine| {
            while index <= end {
                engine.check_deadline()?;
                engine
                    .variables
                    .insert(variable.clone(), Value::number(engine.precision, &index));
                let term = engine.eval(&arguments[2])?;
                result = engine.binary(if product { "*" } else { "+" }, &result, &term)?;
                index += 1;
            }
            Ok(result)
        })
    }

    fn derivative(&mut self, name: &str, order: u32, arguments: &[Expr]) -> Result<Value> {
        if arguments.len() != 1 || order == 0 || order > 8 {
            return Err(error(
                "Derivatives require one argument and an order between 1 and 8.",
            ));
        }
        let at = self.eval(&arguments[0])?;
        // Use arbitrary precision evaluation with a precision-dependent step:
        // cancellation leaves ample significant bits for the displayed result.
        let exponent = -((self.precision / (order + 4)).clamp(8, 80) as i32);
        let step = self.float(2.0_f64.powi(exponent) * at.as_f64()?.abs().max(1.0));
        self.derivative_at(name, order, &at, &step)
    }

    fn derivative_at(&mut self, name: &str, order: u32, at: &Value, step: &Value) -> Result<Value> {
        if order == 0 {
            return self.call_values(name, vec![at.clone()]);
        }
        let plus = value::binary("+", at, step, self.precision)?;
        let minus = value::binary("-", at, step, self.precision)?;
        let above = self.derivative_at(name, order - 1, &plus, step)?;
        let below = self.derivative_at(name, order - 1, &minus, step)?;
        let numerator = value::binary("-", &above, &below, self.precision)?;
        let denominator = value::binary("*", step, &self.number(2), self.precision)?;
        value::binary("/", &numerator, &denominator, self.precision)
    }

    fn integrate(&mut self, arguments: &[Expr]) -> Result<Value> {
        if arguments.len() != 3 && arguments.len() != 4 {
            return Err(error(
                "Integration expects lower and upper limits, an expression, and a differential.",
            ));
        }
        let lower = self.eval(&arguments[0])?.as_f64()?;
        let upper = self.eval(&arguments[1])?.as_f64()?;
        if !lower.is_finite() || !upper.is_finite() {
            return Err(error("Integration limits must be finite."));
        }
        let (body, variable) = if arguments.len() == 4 {
            let Expr::Name(differential) = &arguments[3] else {
                return Err(error("Expected an integration differential such as dx."));
            };
            (
                &arguments[2],
                differential
                    .strip_prefix('d')
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| error("Expected an integration differential such as dx."))?
                    .to_owned(),
            )
        } else if let Some((body, variable)) = differential(&arguments[2]) {
            (body, variable)
        } else {
            return Err(error("Expected an integration differential such as dx."));
        };
        if lower == upper {
            return Ok(self.number(0));
        }
        self.with_bindings(vec![(variable.clone(), self.float(lower))], |engine| {
            let middle = (lower + upper) / 2.0;
            let a = engine.sample(body, &variable, lower)?;
            let m = engine.sample(body, &variable, middle)?;
            let b = engine.sample(body, &variable, upper)?;
            if !finite_number(&a) || !finite_number(&b) {
                return engine.tanh_sinh_integral(body, &variable, lower, upper);
            }
            let estimate = engine.simpson(lower, upper, &a, &m, &b)?;
            engine.adaptive_integral(body, &variable, lower, upper, a, m, b, estimate, 1e-11, 18)
        })
    }

    fn tanh_sinh_integral(
        &mut self,
        body: &Expr,
        variable: &str,
        lower: f64,
        upper: f64,
    ) -> Result<Value> {
        // The double-exponential coordinate change approaches each endpoint
        // without sampling it. Keeping coordinates in Float prevents cancellation
        // near the upper endpoint, where f64 would round the sample onto it.
        let p = self.precision;
        let lower_value = rug::Float::with_val(p, lower);
        let upper_value = rug::Float::with_val(p, upper);
        let width = rug::Float::with_val(p, &upper_value - &lower_value);
        let mut previous: Option<Value> = None;
        for refinement in 0..8 {
            let step = 0.5_f64 / 2.0_f64.powi(refinement);
            let nodes = (4.0 / step) as i32;
            let mut total = self.number(0);
            for index in -nodes..=nodes {
                self.check_deadline()?;
                let t = f64::from(index) * step;
                let u = std::f64::consts::FRAC_PI_2 * t.sinh();
                let exponential = rug::Float::with_val(p, -2.0 * u).exp();
                let denominator = exponential + 1u32;
                let fraction = rug::Float::with_val(p, &width / denominator);
                let at = rug::Float::with_val(p, &lower_value + fraction);
                if at == lower_value || at == upper_value {
                    continue;
                }
                self.variables
                    .insert(variable.into(), Value::Number(at, rug::Float::new(p), None));
                let sample = self.eval(body)?;
                if !finite_number(&sample) {
                    return Err(error("The integral is not finite over these limits."));
                }
                let weight = (upper - lower) * std::f64::consts::FRAC_PI_4 * t.cosh()
                    / u.cosh().powi(2)
                    * step;
                let contribution = value::binary("*", &sample, &self.float(weight), p)?;
                total = value::binary("+", &total, &contribution, p)?;
            }
            if let Some(previous) = previous {
                let difference = value::binary("-", &total, &previous, p)?;
                let difference = value::builtin("abs", vec![difference], p, false)?.as_f64()?;
                let magnitude = value::builtin("abs", vec![total.clone()], p, false)?.as_f64()?;
                if difference <= 1e-10 * magnitude.max(1.0) {
                    return Ok(total);
                }
            }
            previous = Some(total);
        }
        Err(error("The integral did not converge over these limits."))
    }

    fn sample(&mut self, body: &Expr, variable: &str, at: f64) -> Result<Value> {
        self.variables.insert(variable.into(), self.float(at));
        self.eval(body)
    }

    fn simpson(&self, lower: f64, upper: f64, a: &Value, m: &Value, b: &Value) -> Result<Value> {
        let four_m = value::binary("*", m, &self.number(4), self.precision)?;
        let sum = value::binary(
            "+",
            &value::binary("+", a, &four_m, self.precision)?,
            b,
            self.precision,
        )?;
        value::binary(
            "*",
            &sum,
            &self.float((upper - lower) / 6.0),
            self.precision,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn adaptive_integral(
        &mut self,
        body: &Expr,
        variable: &str,
        lower: f64,
        upper: f64,
        a: Value,
        m: Value,
        b: Value,
        whole: Value,
        tolerance: f64,
        depth: u32,
    ) -> Result<Value> {
        self.check_deadline()?;
        let middle = (lower + upper) / 2.0;
        let lm = self.sample(body, variable, (lower + middle) / 2.0)?;
        let rm = self.sample(body, variable, (middle + upper) / 2.0)?;
        let left = self.simpson(lower, middle, &a, &lm, &m)?;
        let right = self.simpson(middle, upper, &m, &rm, &b)?;
        let total = value::binary("+", &left, &right, self.precision)?;
        let difference = value::binary("-", &total, &whole, self.precision)?;
        let magnitude =
            value::builtin("abs", vec![difference.clone()], self.precision, false)?.as_f64()?;
        if !magnitude.is_finite() {
            return Err(error("The integral is not finite over these limits."));
        }
        if depth == 0 || magnitude <= 15.0 * tolerance {
            let correction = value::binary("/", &difference, &self.number(15), self.precision)?;
            return value::binary("+", &total, &correction, self.precision);
        }
        let left = self.adaptive_integral(
            body,
            variable,
            lower,
            middle,
            a,
            lm,
            m.clone(),
            left,
            tolerance / 2.0,
            depth - 1,
        )?;
        let right = self.adaptive_integral(
            body,
            variable,
            middle,
            upper,
            m,
            rm,
            b,
            right,
            tolerance / 2.0,
            depth - 1,
        )?;
        value::binary("+", &left, &right, self.precision)
    }

    fn comprehension(&mut self, body: &Expr, conditions: &[Expr]) -> Result<Value> {
        if conditions.is_empty() {
            return Err(error("A comprehension needs bounds."));
        }
        if conditions.len() > crate::calculator::MAX_RECURSION_DEPTH as usize {
            return Err(CalcError::new(
                "recursion_limit",
                "Too many comprehension variables.",
            ));
        }
        let mut output = Vec::new();
        self.comprehension_level(body, conditions, 0, &mut output)?;
        Ok(Value::Vector(output))
    }

    fn comprehension_level(
        &mut self,
        body: &Expr,
        conditions: &[Expr],
        level: usize,
        output: &mut Vec<Value>,
    ) -> Result<()> {
        self.check_deadline()?;
        if level == conditions.len() {
            if output.len() >= 100_000 {
                return Err(timeout());
            }
            output.push(self.eval(body)?);
            return Ok(());
        }
        let condition = &conditions[level];
        let mut comparisons = Vec::new();
        comparisons_in(condition, &mut comparisons);
        let variable = comparisons
            .iter()
            .find_map(|(_, left, right)| match (left, right) {
                (Expr::Name(name), _) if value::constant(name, self.precision).is_none() => {
                    Some(name.clone())
                }
                (_, Expr::Name(name)) if value::constant(name, self.precision).is_none() => {
                    Some(name.clone())
                }
                _ => None,
            })
            .ok_or_else(|| {
                error("A comprehension needs an integer variable with lower and upper bounds.")
            })?;
        let (mut lower, mut upper) = (f64::NEG_INFINITY, f64::INFINITY);
        for (op, left, right) in comparisons {
            let (op, bound) = if matches!(left, Expr::Name(name) if name == &variable)
                && !has_name(right, &variable)
            {
                (op, right)
            } else if matches!(right, Expr::Name(name) if name == &variable)
                && !has_name(left, &variable)
            {
                (reverse_comparison(op), left)
            } else {
                continue;
            };
            let bound = self.eval(bound)?.as_f64()?;
            match op {
                ">" => lower = lower.max(bound.floor() + 1.0),
                ">=" => lower = lower.max(bound.ceil()),
                "<" => upper = upper.min(bound.ceil() - 1.0),
                "<=" => upper = upper.min(bound.floor()),
                "=" => {
                    lower = lower.max(bound.ceil());
                    upper = upper.min(bound.floor());
                }
                _ => {}
            }
        }
        if !lower.is_finite() || !upper.is_finite() {
            return Err(error(
                "A comprehension needs finite lower and upper bounds.",
            ));
        }
        if lower.abs() > i64::MAX as f64
            || upper.abs() > i64::MAX as f64
            || upper - lower > 100_000.0
        {
            return Err(timeout());
        }
        self.with_bindings(vec![(variable.clone(), self.number(0))], |engine| {
            for index in (lower as i64)..=(upper as i64) {
                engine
                    .variables
                    .insert(variable.clone(), Value::number(engine.precision, index));
                if engine.eval(condition)?.truthy()? {
                    engine.comprehension_level(body, conditions, level + 1, output)?;
                }
            }
            Ok(())
        })
    }

    fn unknown_names(&self, expression: &Expr) -> Vec<String> {
        let mut names = Vec::new();
        free_names(expression, &HashSet::new(), &mut names);
        names.retain(|name| {
            !self.variables.contains_key(name)
                && !self.definitions.contains_key(name)
                && !self.units.contains_key(name)
                && value::constant(name, self.precision).is_none()
        });
        names
    }

    fn solve_equations(&mut self, equations: &[Expr]) -> Result<Value> {
        if equations.is_empty() {
            return Err(error("An equation system cannot be empty."));
        }
        let mut names = Vec::new();
        for equation in equations {
            if !matches!(equation, Expr::Binary(op, _, _) if op == "=") {
                return Err(error("An equation system must contain equalities."));
            }
            for name in self.unknown_names(equation) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        if names.is_empty() {
            let mut all_true = true;
            for equation in equations {
                all_true &= self.eval(equation)?.truthy()?;
            }
            return Ok(Value::Boolean(all_true));
        }
        names.sort();
        if names.len() != equations.len() || names.len() > 16 {
            return Err(error(
                "An equation system needs one equation per unknown, with at most 16 unknowns.",
            ));
        }
        self.with_bindings(
            names
                .iter()
                .map(|name| (name.clone(), self.number(1)))
                .collect(),
            |engine| {
                if names.len() == 1
                    && let Some(solution) = engine.linear_solution(&equations[0], &names[0])?
                {
                    return Ok(solution);
                }
                let roots = engine.newton_system(equations, &names)?;
                let values = roots
                    .into_iter()
                    .map(|root| {
                        // Exact integral solutions should not acquire a floating point
                        // artifact merely because the root search uses double precision.
                        let rounded = root.round();
                        engine.float(if (root - rounded).abs() <= 1e-12 * root.abs().max(1.0) {
                            rounded
                        } else {
                            root
                        })
                    })
                    .collect::<Vec<_>>();
                if values.len() == 1 {
                    Ok(values.into_iter().next().unwrap())
                } else {
                    Ok(Value::Vector(values))
                }
            },
        )
    }

    fn linear_solution(&mut self, equation: &Expr, name: &str) -> Result<Option<Value>> {
        let zero = self.residual_value(equation, name, self.number(0))?;
        let one = self.residual_value(equation, name, self.number(1))?;
        let two = self.residual_value(equation, name, self.number(2))?;
        if !finite_number(&zero) || !finite_number(&one) || !finite_number(&two) {
            return Ok(None);
        }
        let slope = value::binary("-", &one, &zero, self.precision)?;
        let next_slope = value::binary("-", &two, &one, self.precision)?;
        if slope.is_zero() || !value::binary("=", &slope, &next_slope, self.precision)?.truthy()? {
            return Ok(None);
        }
        let negative_zero = value::unary("-", &zero, self.precision)?;
        let candidate = value::binary("/", &negative_zero, &slope, self.precision)?;
        let residual = self.residual_value(equation, name, candidate.clone())?;
        let magnitude = value::builtin("abs", vec![residual], self.precision, false)?.as_f64()?;
        if magnitude.is_finite() && magnitude <= 1e-10 {
            Ok(Some(candidate))
        } else {
            Ok(None)
        }
    }

    fn residual_value(&mut self, equation: &Expr, name: &str, point: Value) -> Result<Value> {
        self.variables.insert(name.into(), point);
        let Expr::Binary(_, left, right) = equation else {
            unreachable!()
        };
        let left = self.eval(left)?;
        let right = self.eval(right)?;
        self.binary("-", &left, &right)
    }

    fn residuals(
        &mut self,
        equations: &[Expr],
        names: &[String],
        point: &[f64],
    ) -> Result<Vec<f64>> {
        for (name, coordinate) in names.iter().zip(point) {
            self.variables.insert(name.clone(), self.float(*coordinate));
        }
        equations
            .iter()
            .map(|equation| {
                let Expr::Binary(_, left, right) = equation else {
                    unreachable!()
                };
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                self.binary("-", &left, &right)?.as_f64()
            })
            .collect()
    }

    fn newton_system(&mut self, equations: &[Expr], names: &[String]) -> Result<Vec<f64>> {
        let size = names.len();
        for initial in [1.0, 0.0, 2.0, -1.0, 10.0, -10.0] {
            let mut point = vec![initial; size];
            for _ in 0..100 {
                self.check_deadline()?;
                let residual = self.residuals(equations, names, &point)?;
                // f64::max ignores NaN, so validate every component before folding.
                if !residual.iter().all(|value| value.is_finite()) {
                    break;
                }
                let norm = residual
                    .iter()
                    .fold(0.0_f64, |norm, value| norm.max(value.abs()));
                if norm < 1e-11 {
                    return Ok(point);
                }
                let mut matrix = vec![vec![0.0; size]; size];
                for column in 0..size {
                    let step = 1e-5 * point[column].abs().max(1.0);
                    let mut above = point.clone();
                    let mut below = point.clone();
                    above[column] += step;
                    below[column] -= step;
                    let above = self.residuals(equations, names, &above)?;
                    let below = self.residuals(equations, names, &below)?;
                    for row in 0..size {
                        matrix[row][column] = (above[row] - below[row]) / (2.0 * step);
                    }
                }
                let Some(delta) = solve_linear(matrix, residual) else {
                    break;
                };
                let mut accepted = false;
                let mut scale = 1.0;
                for _ in 0..20 {
                    let candidate = point
                        .iter()
                        .zip(&delta)
                        .map(|(at, change)| at - scale * change)
                        .collect::<Vec<_>>();
                    if !candidate.iter().all(|value| value.is_finite()) {
                        scale *= 0.5;
                        continue;
                    }
                    let next = self.residuals(equations, names, &candidate)?;
                    if !next.iter().all(|value| value.is_finite()) {
                        scale *= 0.5;
                        continue;
                    }
                    let next_norm = next
                        .iter()
                        .fold(0.0_f64, |norm, value| norm.max(value.abs()));
                    if next_norm < norm {
                        point = candidate;
                        accepted = true;
                        break;
                    }
                    scale *= 0.5;
                }
                if !accepted {
                    break;
                }
            }
        }
        Err(error(
            "Could not find a finite solution to the equation system.",
        ))
    }

    fn define_unit(&mut self, name: String, formula: Expr) -> Result<()> {
        if value::constant(&name, self.precision).is_some()
            || self.variables.contains_key(&name)
            || self.definitions.contains_key(&name)
        {
            return Err(error("A unit name cannot replace a constant or variable."));
        }
        let formula = unit_formula(formula);
        let mut names = Vec::new();
        names_in(&formula, &mut names);
        names.retain(|candidate| {
            value::constant(candidate, self.precision).is_none()
                && !self.variables.contains_key(candidate)
                && !self.definitions.contains_key(candidate)
        });
        if names.len() != 1 || names[0] == name {
            return Err(error(
                "A unit definition must reference exactly one different base unit.",
            ));
        }
        let parent = names.remove(0);
        // Reject cycles before installing a new conversion.
        let mut ancestor = parent.as_str();
        let mut visited = HashSet::new();
        while let Some(Some(definition)) = self.units.get(ancestor) {
            if ancestor == name || !visited.insert(ancestor.to_owned()) {
                return Err(error("Unit definitions cannot contain cycles."));
            }
            ancestor = &definition.parent;
        }
        if ancestor == name {
            return Err(error("Unit definitions cannot contain cycles."));
        }
        self.units.entry(parent.clone()).or_insert(None);
        self.units
            .insert(name, Some(UnitDefinition { parent, formula }));
        Ok(())
    }

    fn convert(&mut self, value: &Value, target: &str) -> Result<Value> {
        let Value::Number(_, _, Some(source)) = value else {
            return Err(error("Unit conversion requires a number with a unit."));
        };
        if source == target {
            return Ok(value.clone());
        }
        let mut result = value.clone().with_unit(None)?;
        if matches!((source.as_str(), target), ("deg", "rad") | ("rad", "deg")) {
            let pi = value::constant("pi", self.precision).unwrap();
            result = if source == "deg" {
                value::binary(
                    "/",
                    &value::binary("*", &result, &pi, self.precision)?,
                    &self.number(180),
                    self.precision,
                )?
            } else {
                value::binary(
                    "/",
                    &value::binary("*", &result, &self.number(180), self.precision)?,
                    &pi,
                    self.precision,
                )?
            };
            return result.with_unit(Some(target.to_owned()));
        }
        // Unit declarations form a small graph. Each edge is a formula from its
        // parent coordinate to the declared coordinate, and its inverse.
        let mut queue = VecDeque::from([(source.clone(), Vec::<(UnitDefinition, bool)>::new())]);
        let mut seen = HashSet::from([source.clone()]);
        let mut path = None;
        while let Some((current, steps)) = queue.pop_front() {
            if current == target {
                path = Some(steps);
                break;
            }
            for (unit, definition) in &self.units {
                let Some(definition) = definition else {
                    continue;
                };
                let edge = if definition.parent == current {
                    Some((unit, true))
                } else if unit == &current {
                    Some((&definition.parent, false))
                } else {
                    None
                };
                if let Some((next, forward)) = edge
                    && seen.insert(next.clone())
                {
                    let mut next_steps = steps.clone();
                    next_steps.push((definition.clone(), forward));
                    queue.push_back((next.clone(), next_steps));
                }
            }
        }
        for (definition, forward) in
            path.ok_or_else(|| error(format!("Cannot convert '{source}' to '{target}'.")))?
        {
            result = if forward {
                self.apply_unit(&definition, result)?
            } else {
                self.invert_unit(&definition, result)?
            };
        }
        result.with_unit(Some(target.to_owned()))
    }

    fn apply_unit(&mut self, definition: &UnitDefinition, value: Value) -> Result<Value> {
        self.with_bindings(vec![(definition.parent.clone(), value)], |engine| {
            engine.eval(&definition.formula)
        })?
        .with_unit(None)
    }

    fn invert_unit(&mut self, definition: &UnitDefinition, target: Value) -> Result<Value> {
        let zero = self.apply_unit(definition, self.number(0))?;
        let one = self.apply_unit(definition, self.number(1))?;
        let two = self.apply_unit(definition, self.number(2))?;
        let slope = value::binary("-", &one, &zero, self.precision)?;
        let next_slope = value::binary("-", &two, &one, self.precision)?;
        let linear = value::binary("=", &slope, &next_slope, self.precision)?.truthy()?;
        if linear && !slope.real()?.is_zero() {
            let candidate = value::binary(
                "/",
                &value::binary("-", &target, &zero, self.precision)?,
                &slope,
                self.precision,
            )?;
            // Preserve nonfinite propagation for an already nonfinite input;
            // finite targets must always pass the forward validation below.
            if let Value::Number(real, imaginary, _) = &target
                && (!real.is_finite() || !imaginary.is_finite())
            {
                return Ok(candidate);
            }
            // Equal sampled slopes only suggest an affine formula. Check its
            // inverse against the actual formula before taking the fast path.
            if let Value::Number(real, imaginary, _) = &candidate
                && real.is_finite()
                && imaginary.is_finite()
                && let Ok(Value::Number(ar, ai, _)) = self.apply_unit(definition, candidate.clone())
                && let Value::Number(tr, ti, _) = &target
                && let Value::Number(zr, zi, _) = &zero
                && let Value::Number(or, _, _) = &one
            {
                let sample_scale = zr.clone().abs().max(&or.clone().abs());
                let matches = [(&ar, tr, zr, real), (&ai, ti, zi, imaginary)]
                    .into_iter()
                    .all(|(actual, expected, offset, coordinate)| {
                        // Allow precision-scaled roundoff, including cancellation
                        // in the sampled slope amplified by the candidate. Keep
                        // this separate from the language's absolute equality
                        // tolerance and never narrow the affine path to f64.
                        let slope_error_scale = sample_scale.clone() * coordinate.clone().abs();
                        let scale = actual
                            .clone()
                            .abs()
                            .max(&expected.clone().abs())
                            .max(&offset.clone().abs())
                            .max(&slope_error_scale);
                        let tolerance = scale >> (self.precision - 4);
                        actual.is_finite()
                            && expected.is_finite()
                            && rug::Float::with_val(self.precision, actual - expected).abs()
                                <= tolerance
                    });
                if matches {
                    return Ok(candidate);
                }
            }
        }
        let expected = target.as_f64()?;
        for initial in [1.0, expected, -1.0, 10.0] {
            let mut point = initial;
            for _ in 0..100 {
                self.check_deadline()?;
                let actual = self.apply_unit(definition, self.float(point))?.as_f64()?;
                if (actual - expected).abs() < 1e-12 * expected.abs().max(1.0) {
                    return Ok(self.float(point));
                }
                let step = 1e-5 * point.abs().max(1.0);
                let high = self
                    .apply_unit(definition, self.float(point + step))?
                    .as_f64()?;
                let low = self
                    .apply_unit(definition, self.float(point - step))?
                    .as_f64()?;
                let derivative = (high - low) / (2.0 * step);
                if !derivative.is_finite() || derivative.abs() < 1e-15 {
                    break;
                }
                point -= (actual - expected) / derivative;
                if !point.is_finite() {
                    break;
                }
            }
        }
        Err(error("Could not invert this unit conversion."))
    }
}

fn differential(expression: &Expr) -> Option<(&Expr, String)> {
    match expression {
        Expr::Binary(op, body, differential) if op == "*" => {
            if let Expr::Name(name) = differential.as_ref() {
                return name
                    .strip_prefix('d')
                    .filter(|name| !name.is_empty())
                    .map(|name| (body.as_ref(), name.to_owned()));
            }
            None
        }
        Expr::Group(inner) => differential(inner),
        _ => None,
    }
}

fn solve_linear(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Option<Vec<f64>> {
    let size = rhs.len();
    for column in 0..size {
        let pivot = (column..size).max_by(|a, b| {
            matrix[*a][column]
                .abs()
                .total_cmp(&matrix[*b][column].abs())
        })?;
        if !matrix[pivot][column].is_finite() || matrix[pivot][column].abs() < 1e-14 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let divisor = matrix[column][column];
        for entry in &mut matrix[column][column..] {
            *entry /= divisor;
        }
        rhs[column] /= divisor;
        let pivot_row = matrix[column].clone();
        for row in 0..size {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for (entry, pivot) in matrix[row].iter_mut().zip(&pivot_row).skip(column) {
                *entry -= factor * pivot;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    Some(rhs)
}

fn comparisons_in<'a>(expression: &'a Expr, output: &mut Vec<(&'a str, &'a Expr, &'a Expr)>) {
    match expression {
        Expr::Group(inner) => comparisons_in(inner, output),
        Expr::Binary(op, left, right) if op == "and" => {
            comparisons_in(left, output);
            comparisons_in(right, output);
        }
        Expr::Binary(op, left, right)
            if matches!(op.as_str(), "=" | "!=" | "<" | "<=" | ">" | ">=") =>
        {
            output.push((op, left, right))
        }
        _ => {}
    }
}

fn reverse_comparison(op: &str) -> &str {
    match op {
        "<" => ">",
        "<=" => ">=",
        ">" => "<",
        ">=" => "<=",
        _ => op,
    }
}

fn has_name(expression: &Expr, name: &str) -> bool {
    let mut names = Vec::new();
    names_in(expression, &mut names);
    names.iter().any(|candidate| candidate == name)
}

fn names_in(expression: &Expr, names: &mut Vec<String>) {
    match expression {
        Expr::Name(name) => {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        Expr::Unary(_, inner)
        | Expr::Group(inner)
        | Expr::Unit(inner, _)
        | Expr::Convert(inner, _) => names_in(inner, names),
        Expr::Binary(_, left, right) => {
            names_in(left, names);
            names_in(right, names);
        }
        Expr::Call(_, items)
        | Expr::Derivative(_, _, items)
        | Expr::Vector(items)
        | Expr::Equations(items) => {
            for item in items {
                names_in(item, names);
            }
        }
        Expr::Matrix(rows) => {
            for row in rows {
                for item in row {
                    names_in(item, names);
                }
            }
        }
        Expr::Index(inner, indices) | Expr::Comprehension(inner, indices) => {
            names_in(inner, names);
            for index in indices {
                names_in(index, names);
            }
        }
        Expr::Piecewise(branches) => {
            for (body, condition) in branches {
                names_in(body, names);
                if let Some(condition) = condition {
                    names_in(condition, names);
                }
            }
        }
        Expr::Number(_, _) | Expr::Boolean(_) => {}
    }
}

// Inside a conversion formula, a unit occurrence denotes its coordinate. In
// ordinary expressions the same syntax attaches a unit to a numeric value.
fn unit_formula(expression: Expr) -> Expr {
    match expression {
        Expr::Unit(inner, name) => Expr::Binary(
            "*".into(),
            Box::new(unit_formula(*inner)),
            Box::new(Expr::Name(name)),
        ),
        Expr::Group(inner) => Expr::Group(Box::new(unit_formula(*inner))),
        Expr::Unary(op, inner) => Expr::Unary(op, Box::new(unit_formula(*inner))),
        Expr::Binary(op, left, right) => Expr::Binary(
            op,
            Box::new(unit_formula(*left)),
            Box::new(unit_formula(*right)),
        ),
        Expr::Call(name, arguments) => {
            Expr::Call(name, arguments.into_iter().map(unit_formula).collect())
        }
        other => other,
    }
}

fn contains_equation(expression: &Expr) -> bool {
    match expression {
        Expr::Equations(_) => true,
        Expr::Binary(op, left, right) => {
            op == "=" || contains_equation(left) || contains_equation(right)
        }
        Expr::Group(inner)
        | Expr::Unary(_, inner)
        | Expr::Unit(inner, _)
        | Expr::Convert(inner, _) => contains_equation(inner),
        _ => false,
    }
}

// Unlike the unit-formula visitor, equation discovery must recognize the bound
// variables introduced by sums, products, and integrals.
fn free_names(expression: &Expr, bound: &HashSet<String>, names: &mut Vec<String>) {
    match expression {
        Expr::Name(name) => {
            if !bound.contains(name) && !names.contains(name) {
                names.push(name.clone());
            }
        }
        Expr::Call(name, arguments)
            if matches!(name.as_str(), "sum" | "prod" | "Σ" | "∑" | "∏")
                && arguments.len() == 3
                && matches!(&arguments[0], Expr::Binary(op, _, _) if op == "=") =>
        {
            let Expr::Binary(_, variable, start) = &arguments[0] else {
                unreachable!()
            };
            free_names(start, bound, names);
            free_names(&arguments[1], bound, names);
            let mut inner_bound = bound.clone();
            if let Expr::Name(variable) = variable.as_ref() {
                inner_bound.insert(variable.clone());
            }
            free_names(&arguments[2], &inner_bound, names);
        }
        Expr::Call(name, arguments)
            if matches!(name.as_str(), "integrate" | "integral" | "∫")
                && matches!(arguments.len(), 3 | 4) =>
        {
            free_names(&arguments[0], bound, names);
            free_names(&arguments[1], bound, names);
            let mut inner_bound = bound.clone();
            let body = if arguments.len() == 4 {
                if let Expr::Name(name) = &arguments[3]
                    && let Some(name) = name.strip_prefix('d')
                {
                    inner_bound.insert(name.to_owned());
                }
                &arguments[2]
            } else if let Some((body, name)) = differential(&arguments[2]) {
                inner_bound.insert(name);
                body
            } else {
                &arguments[2]
            };
            free_names(body, &inner_bound, names);
        }
        Expr::Group(inner)
        | Expr::Unary(_, inner)
        | Expr::Unit(inner, _)
        | Expr::Convert(inner, _) => free_names(inner, bound, names),
        Expr::Binary(_, left, right) => {
            free_names(left, bound, names);
            free_names(right, bound, names);
        }
        Expr::Call(_, items)
        | Expr::Derivative(_, _, items)
        | Expr::Vector(items)
        | Expr::Equations(items) => {
            for item in items {
                free_names(item, bound, names);
            }
        }
        Expr::Matrix(rows) => {
            for row in rows {
                for item in row {
                    free_names(item, bound, names);
                }
            }
        }
        Expr::Comprehension(body, conditions) => {
            let mut inner_bound = bound.clone();
            for condition in conditions {
                let mut comparisons = Vec::new();
                comparisons_in(condition, &mut comparisons);
                if let Some(variable) =
                    comparisons
                        .iter()
                        .find_map(|(_, left, right)| match (left, right) {
                            (Expr::Name(name), _) if value::constant(name, 32).is_none() => {
                                Some(name.clone())
                            }
                            (_, Expr::Name(name)) if value::constant(name, 32).is_none() => {
                                Some(name.clone())
                            }
                            _ => None,
                        })
                {
                    inner_bound.insert(variable);
                }
                free_names(condition, &inner_bound, names);
            }
            free_names(body, &inner_bound, names);
        }
        Expr::Index(inner, indices) => {
            free_names(inner, bound, names);
            for index in indices {
                free_names(index, bound, names);
            }
        }
        Expr::Piecewise(branches) => {
            for (body, condition) in branches {
                free_names(body, bound, names);
                if let Some(condition) = condition {
                    free_names(condition, bound, names);
                }
            }
        }
        Expr::Number(_, _) | Expr::Boolean(_) => {}
    }
}

fn finite_number(value: &Value) -> bool {
    matches!(value, Value::Number(real, imaginary, _) if real.is_finite() && imaginary.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real(expression: &str) -> f64 {
        Engine::new(128, false)
            .evaluate(expression)
            .unwrap()
            .unwrap()
            .as_f64()
            .unwrap()
    }

    #[test]
    fn call_scopes_do_not_capture_caller_parameters() {
        assert_eq!(real("x=2;g(t)=x;f(x)=g(0);f(10)"), 2.0);
        assert_eq!(real("x=2;y=x+1;f(x)=y;f(10)"), 3.0);
        assert_eq!(real("x=7;f(x)=sum(x=1,3,x)+x;f(10)+x"), 23.0);
    }

    #[test]
    fn lazy_definition_cycles_are_calculation_errors() {
        for expression in ["x=2;x=x+1;x", "a=b;b=a;a", "x=2;x=(x=2)"] {
            assert_eq!(
                Engine::new(128, false)
                    .evaluate(expression)
                    .unwrap_err()
                    .code,
                "calculation_error"
            );
        }
        assert_eq!(real("a=b;b=3;a"), 3.0);
    }

    #[test]
    fn ans_changes_between_programs() {
        let mut engine = Engine::new(128, false);
        assert!(engine.evaluate("2;ans+1").is_err());
        engine.evaluate("2").unwrap();
        assert_eq!(
            engine
                .evaluate("5;ans+1")
                .unwrap()
                .unwrap()
                .as_f64()
                .unwrap(),
            3.0
        );
        assert_eq!(
            engine.evaluate("ans+1").unwrap().unwrap().as_f64().unwrap(),
            4.0
        );
    }

    #[test]
    fn aggregate_bounds_preserve_large_integers() {
        assert_eq!(
            real("sum(n=9007199254740993,9007199254740993,n)-9007199254740992"),
            1.0
        );
        assert!(
            Engine::new(128, false)
                .evaluate("sum(n=1,3,n)=6")
                .unwrap()
                .unwrap()
                .truthy()
                .unwrap()
        );
    }

    #[test]
    fn collection_comparisons_do_not_solve_bound_variables() {
        let result = Engine::new(128, false)
            .evaluate("[x:0<=x and x<3]=(0,1,2)")
            .unwrap()
            .unwrap();
        assert!(result.truthy().unwrap());
    }

    #[test]
    fn numerical_scopes_restore_values_after_errors() {
        let mut engine = Engine::new(128, false);
        engine.evaluate("n=7").unwrap();
        assert!(engine.evaluate("sum(n=1,3,missing_name)").is_err());
        assert_eq!(
            engine.evaluate("n").unwrap().unwrap().as_f64().unwrap(),
            7.0
        );
        assert!(engine.evaluate("integrate(0,1,x)").is_err());
    }

    #[test]
    fn linear_equations_preserve_complex_and_precise_solutions() {
        let solution = Engine::new(128, false).evaluate("x+2i=1").unwrap().unwrap();
        let Value::Number(real, imaginary, _) = solution else {
            panic!("expected a number");
        };
        assert_eq!(real, 1);
        assert_eq!(imaginary, -2);
        let solution = Engine::new(128, false)
            .evaluate("2x=18014398509481986")
            .unwrap()
            .unwrap();
        assert_eq!(
            solution.real().unwrap(),
            &rug::Float::with_val(128, 9007199254740993u64)
        );
    }

    #[test]
    fn integrals_allow_integrable_endpoint_singularities() {
        for (expression, expected) in [
            ("integrate(0,1,1/sqrt(x),dx)", 2.0),
            ("integrate(0,1,ln(x),dx)", -1.0),
            ("integrate(0,1,1/sqrt(1-x),dx)", 2.0),
        ] {
            assert!((real(expression) - expected).abs() < 1e-8, "{expression}");
        }
    }

    #[test]
    fn comprehension_scope_count_is_bounded() {
        let conditions =
            vec!["x>=0 and x<=0"; crate::calculator::MAX_RECURSION_DEPTH as usize + 1].join(",");
        let expression = format!("[x:{conditions}]");
        assert_eq!(
            Engine::new(128, false)
                .evaluate(&expression)
                .unwrap_err()
                .code,
            "recursion_limit"
        );
    }

    #[test]
    fn deadline_is_shared_across_programs() {
        let mut engine = Engine::new(128, false);
        engine.evaluate("2").unwrap();
        engine.deadline = Instant::now();
        assert_eq!(engine.evaluate("3").unwrap_err().code, "timeout");
    }
}
