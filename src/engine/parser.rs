//! The calculator's expression grammar, implemented locally with a bounded Pratt parser.
//!
//! Tokens retain whole names until parsing, because preceding declarations determine
//! whether juxtaposition denotes a function call, a unit suffix, or multiplication.

use std::collections::HashSet;

use crate::calculator::{CalcError, MAX_RECURSION_DEPTH};

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Variable(String, Expr),
    Function(String, Vec<String>, Expr),
    Unit(String, Expr),
    Expression(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(String, u32),
    Boolean(bool),
    Name(String),
    Group(Box<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Derivative(String, u32, Vec<Expr>),
    Vector(Vec<Expr>),
    Matrix(Vec<Vec<Expr>>),
    Index(Box<Expr>, Vec<Expr>),
    Piecewise(Vec<(Expr, Option<Expr>)>),
    Comprehension(Box<Expr>, Vec<Expr>),
    Unit(Box<Expr>, String),
    Convert(Box<Expr>, String),
    Equations(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(String, u32),
    Word(String),
    Op(String),
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    IndexOpen,
    IndexClose,
    Comma,
    Separator,
    Newline,
    Colon,
    Bar,
    Prime,
    Inverse,
    FloorOpen,
    FloorClose,
    CeilOpen,
    CeilClose,
    End,
}

fn error(message: impl Into<String>) -> CalcError {
    CalcError::new("calculation_error", message)
}

fn recursion_error() -> CalcError {
    CalcError::new(
        "recursion_limit",
        "Expression nesting exceeds the recursion limit",
    )
}

fn subscript(c: char) -> Option<char> {
    "₀₁₂₃₄₅₆₇₈₉"
        .chars()
        .position(|n| n == c)
        .map(|n| (b'0' + n as u8) as char)
}

fn superscript(c: char) -> Option<char> {
    "⁰¹²³⁴⁵⁶⁷⁸⁹"
        .chars()
        .position(|n| n == c)
        .map(|n| (b'0' + n as u8) as char)
}

fn canonical_name(name: &str) -> String {
    match name {
        "π" => "pi",
        "τ" => "tau",
        "ϕ" | "φ" => "phi",
        "Γ" => "gamma",
        "Σ" | "∑" => "sum",
        "∏" => "prod",
        "∫" => "integrate",
        "√" => "sqrt",
        "∛" => "cbrt",
        "°" => "deg",
        other => other,
    }
    .to_owned()
}

fn lex(source: &str) -> Result<Vec<Token>, CalcError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c.is_whitespace() && c != '\n' {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            let mut radix = 10;
            if c == '0' && matches!(chars.get(i + 1), Some('b' | 'o' | 'x')) {
                radix = match chars[i + 1] {
                    'b' => 2,
                    'o' => 8,
                    _ => 16,
                };
                i += 2;
                let number_start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_digit()
                        || chars[i] == '.'
                        || (radix == 16 && chars[i].is_ascii_hexdigit()))
                {
                    i += 1;
                }
                if i == number_start {
                    return Err(error("A radix prefix must be followed by digits"));
                }
                tokens.push(Token::Number(
                    chars[number_start..i].iter().collect(),
                    radix,
                ));
                continue;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let exponent_digits = i + 1 + usize::from(matches!(chars.get(i + 1), Some('+' | '-')));
            if chars.get(i) == Some(&'E')
                && chars.get(exponent_digits).is_some_and(char::is_ascii_digit)
            {
                i += 1;
                if matches!(chars.get(i), Some('+' | '-')) {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let number: String = chars[start..i].iter().collect();
            if chars.get(i) == Some(&'_') {
                i += 1;
                let radix_start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                radix = chars[radix_start..i]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .map_err(|_| error("A radix suffix must contain a base"))?;
            } else if chars.get(i).and_then(|c| subscript(*c)).is_some() {
                let mut base = String::new();
                while let Some(digit) = chars.get(i).and_then(|c| subscript(*c)) {
                    base.push(digit);
                    i += 1;
                }
                radix = base.parse().map_err(|_| error("Invalid radix"))?;
            }
            if !(2..=36).contains(&radix) {
                return Err(error("Number bases must be between 2 and 36"));
            }
            tokens.push(Token::Number(number, radix));
            continue;
        }
        if c == '⁻' && chars.get(i + 1) == Some(&'¹') {
            tokens.push(Token::Inverse);
            i += 2;
            continue;
        }
        if let Some(digit) = superscript(c) {
            let mut number = String::from(digit);
            i += 1;
            while let Some(digit) = chars.get(i).and_then(|c| superscript(*c)) {
                number.push(digit);
                i += 1;
            }
            tokens.push(Token::Op("^".into()));
            tokens.push(Token::Number(number, 10));
            continue;
        }
        let symbol_name = match c {
            '√' | '∛' | '∑' | '∏' | '∫' | '°' | 'π' | 'τ' | 'ϕ' | 'φ' | 'Γ' | 'Σ' => {
                Some(canonical_name(&c.to_string()))
            }
            _ => None,
        };
        if let Some(name) = symbol_name {
            tokens.push(Token::Word(name));
            i += 1;
            continue;
        }
        if (c.is_alphabetic() && c != 'ᵀ') || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len()
                && ((chars[i].is_alphanumeric()
                    && chars[i] != 'ᵀ'
                    && superscript(chars[i]).is_none())
                    || chars[i] == '_'
                    || subscript(chars[i]).is_some())
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push(match word.as_str() {
                "and" | "or" | "not" => Token::Op(word),
                "mod" => Token::Op("%".into()),
                _ => Token::Word(canonical_name(&word)),
            });
            continue;
        }
        let following = chars.get(i + 1).copied();
        let double_operator = match (c, following) {
            ('*', Some('*')) => Some("^"),
            ('!', Some('=')) => Some("!="),
            ('<', Some('=')) => Some("<="),
            ('>', Some('=')) => Some(">="),
            ('<', Some('<')) => Some("<<"),
            ('>', Some('>')) => Some(">>"),
            ('=', Some('=')) => Some("="),
            _ => None,
        };
        if let Some(op) = double_operator {
            tokens.push(Token::Op(op.into()));
            i += 2;
            continue;
        }
        if c == '″' {
            tokens.extend([Token::Prime, Token::Prime]);
            i += 1;
            continue;
        }
        tokens.push(match c {
            '(' => Token::LeftParen,
            ')' => Token::RightParen,
            '[' => Token::LeftBracket,
            ']' => Token::RightBracket,
            '{' => Token::LeftBrace,
            '}' => Token::RightBrace,
            '⟦' => Token::IndexOpen,
            '⟧' => Token::IndexClose,
            ',' => Token::Comma,
            ';' => Token::Separator,
            '\n' => Token::Newline,
            ':' => Token::Colon,
            '|' => Token::Bar,
            '\'' | '′' => Token::Prime,
            '⌊' => Token::FloorOpen,
            '⌋' => Token::FloorClose,
            '⌈' => Token::CeilOpen,
            '⌉' => Token::CeilClose,
            '+' | '-' | '*' | '/' | '^' | '%' | '!' | '=' | '<' | '>' => Token::Op(c.to_string()),
            '−' => Token::Op("-".into()),
            '×' | '⋅' | '·' => Token::Op("*".into()),
            '÷' => Token::Op("/".into()),
            '≠' => Token::Op("!=".into()),
            '≤' => Token::Op("<=".into()),
            '≥' => Token::Op(">=".into()),
            '∧' => Token::Op("and".into()),
            '∨' => Token::Op("or".into()),
            '¬' => Token::Op("not".into()),
            'ᵀ' => Token::Op("transpose".into()),
            _ => return Err(error(format!("Unexpected character '{c}'"))),
        });
        i += 1;
    }
    tokens.push(Token::End);
    Ok(tokens)
}

/// Parse a request input using names from earlier inputs in its local context.
pub fn parse(
    source: &str,
    functions: &[String],
    variables: &[String],
    units: &[String],
) -> Result<Vec<Statement>, CalcError> {
    let mut parser = Parser {
        tokens: lex(source)?,
        position: 0,
        depth: 0,
        bars: 0,
        integral: 0,
        unit_formula: false,
        functions: functions.iter().cloned().collect(),
        variables: variables.iter().cloned().collect(),
        units: units
            .iter()
            .cloned()
            .chain(["deg".into(), "rad".into()])
            .collect(),
    };
    parser.functions.extend(
        super::value::BUILTIN_NAMES
            .iter()
            .map(|name| (*name).to_owned()),
    );
    parser.functions.extend(
        ["sum", "prod", "integrate", "integral"]
            .into_iter()
            .map(String::from),
    );
    // These names are grammatical atoms even when no preceding input defines them.
    parser.variables.extend(
        ["pi", "tau", "e", "phi", "i", "ans", "true", "false"]
            .into_iter()
            .map(String::from),
    );
    let mut statements = Vec::new();
    parser.skip_separators();
    while parser.peek() != &Token::End {
        statements.push(parser.statement()?);
        if !matches!(
            parser.peek(),
            Token::End | Token::Separator | Token::Newline
        ) {
            return Err(error(format!(
                "Expected a statement separator, found {:?}",
                parser.peek()
            )));
        }
        parser.skip_separators();
    }
    Ok(statements)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    depth: u32,
    bars: u32,
    integral: u32,
    unit_formula: bool,
    functions: HashSet<String>,
    variables: HashSet<String>,
    units: HashSet<String>,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }
    fn next(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        if token != Token::End {
            self.position += 1;
        }
        token
    }
    fn take(&mut self, token: &Token) -> bool {
        if self.peek() == token {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, token: Token) -> Result<(), CalcError> {
        if self.take(&token) {
            Ok(())
        } else {
            Err(error(format!(
                "Expected {token:?}, found {:?}",
                self.peek()
            )))
        }
    }
    fn take_word(&mut self, word: &str) -> bool {
        self.take(&Token::Word(word.to_owned()))
    }
    fn skip_separators(&mut self) {
        while self.take_separator() {}
    }
    fn take_separator(&mut self) -> bool {
        self.take(&Token::Separator) || self.take(&Token::Newline)
    }
    fn skip_newlines(&mut self) {
        while self.take(&Token::Newline) {}
    }

    fn statement(&mut self) -> Result<Statement, CalcError> {
        if self.take_word("unit") {
            let Token::Word(name) = self.next() else {
                return Err(error("Expected the new unit's name"));
            };
            self.expect(Token::Op("=".into()))?;
            self.unit_formula = true;
            let formula = self.expression(0)?;
            self.unit_formula = false;
            let mut names = Vec::new();
            collect_names(&formula, &mut names);
            self.units.extend(
                names
                    .into_iter()
                    .filter(|name| !self.variables.contains(name)),
            );
            self.units.insert(name.clone());
            return Ok(Statement::Unit(name, formula));
        }
        if let Token::Word(name) = self.peek().clone() {
            if self.tokens.get(self.position + 1) == Some(&Token::Op("=".into()))
                && !self.has_top_level_logical_operator(true)
                && !matches!(name.as_str(), "true" | "false")
            {
                self.position += 2;
                self.variables.insert(name.clone());
                return Ok(Statement::Variable(name, self.expression(0)?));
            }
            if self.tokens.get(self.position + 1) == Some(&Token::LeftParen)
                && !self.has_top_level_logical_operator(false)
            {
                let saved = self.position;
                self.position += 2;
                let mut parameters = Vec::new();
                let mut valid = true;
                if self.peek() != &Token::RightParen {
                    loop {
                        match self.next() {
                            Token::Word(parameter) => parameters.push(parameter),
                            _ => {
                                valid = false;
                                break;
                            }
                        }
                        if !self.take(&Token::Comma) {
                            break;
                        }
                    }
                }
                if valid && self.take(&Token::RightParen) && self.take(&Token::Op("=".into())) {
                    if parameters.iter().collect::<HashSet<_>>().len() != parameters.len() {
                        return Err(error("Function parameters must have distinct names"));
                    }
                    self.functions.insert(name.clone());
                    let previous_variables = self.variables.clone();
                    self.variables.extend(parameters.iter().cloned());
                    let body = self.expression(0)?;
                    self.variables = previous_variables;
                    return Ok(Statement::Function(name, parameters, body));
                }
                self.position = saved;
            }
        }
        Ok(Statement::Expression(self.expression(0)?))
    }

    fn has_top_level_logical_operator(&self, include_comparisons: bool) -> bool {
        let mut nesting = 0usize;
        let mut saw_equality = false;
        for token in &self.tokens[self.position..] {
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace => nesting += 1,
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    nesting = nesting.saturating_sub(1)
                }
                Token::Op(op)
                    if nesting == 0
                        && matches!(
                            op.as_str(),
                            "and" | "or" | "=" | "!=" | "<" | "<=" | ">" | ">="
                        ) =>
                {
                    if saw_equality && (include_comparisons || matches!(op.as_str(), "and" | "or"))
                    {
                        return true;
                    }
                    saw_equality = op == "=";
                }
                Token::Separator | Token::Newline | Token::End if nesting == 0 => break,
                _ => {}
            }
        }
        false
    }

    fn expression(&mut self, minimum: u8) -> Result<Expr, CalcError> {
        self.depth += 1;
        if self.depth > MAX_RECURSION_DEPTH {
            return Err(recursion_error());
        }
        let result = self.expression_inner(minimum);
        self.depth -= 1;
        result
    }

    fn expression_inner(&mut self, minimum: u8) -> Result<Expr, CalcError> {
        self.skip_newlines();
        let mut left = self.prefix()?;
        loop {
            if let Token::Word(word) = self.peek()
                && !self.variables.contains(word)
                && let Some(remainder) = word.strip_prefix("mod").filter(|tail| !tail.is_empty())
            {
                let mut replacement = vec![Token::Op("%".into())];
                let mut tail = lex(remainder)?;
                tail.pop();
                replacement.extend(tail);
                self.tokens
                    .splice(self.position..=self.position, replacement);
            }
            // Index syntax is recognized only after a value; nested vectors retain
            // their ordinary meaning when the same brackets begin an expression.
            if minimum <= 11
                && (self.peek() == &Token::IndexOpen
                    || (self.peek() == &Token::LeftBracket
                        && self.tokens.get(self.position + 1) == Some(&Token::LeftBracket)))
            {
                let unicode = self.take(&Token::IndexOpen);
                if !unicode {
                    self.position += 2;
                }
                let close = if unicode {
                    Token::IndexClose
                } else {
                    Token::RightBracket
                };
                let indices = self.comma_list(close.clone())?;
                if !unicode {
                    self.expect(Token::RightBracket)?;
                }
                left = checked(Expr::Index(Box::new(left), indices))?;
                continue;
            }
            if minimum <= 10
                && let Token::Op(op) = self.peek().clone()
            {
                let suffix_percent = op == "%"
                    && !self.starts_value(self.position + 1)
                    && self.tokens.get(self.position + 1) != Some(&Token::Op("-".into()));
                if op == "!" || op == "transpose" || suffix_percent {
                    self.next();
                    left = checked(Expr::Unary(op, Box::new(left)))?;
                    continue;
                }
                if op == "^" && self.tokens.get(self.position + 1) == Some(&Token::Word("T".into()))
                {
                    self.position += 2;
                    left = checked(Expr::Unary("transpose".into(), Box::new(left)))?;
                    continue;
                }
            }
            if !self.unit_formula
                && minimum <= 10
                && let Token::Word(unit) = self.peek().clone()
                && self.units.contains(&unit)
            {
                self.next();
                left = checked(Expr::Unit(Box::new(left), unit))?;
                continue;
            }
            if minimum == 0 && self.take_word("to") {
                let Token::Word(unit) = self.next() else {
                    return Err(error("Expected a unit after 'to'"));
                };
                left = checked(Expr::Convert(Box::new(left), unit))?;
                continue;
            }
            let (operator, precedence, right_associative, implicit) =
                if let Token::Op(op) = self.peek() {
                    match op.as_str() {
                        "or" => (op.clone(), 1, false, false),
                        "and" => (op.clone(), 2, false, false),
                        "=" | "!=" | "<" | "<=" | ">" | ">=" => (op.clone(), 3, false, false),
                        "<<" | ">>" => (op.clone(), 4, false, false),
                        "+" | "-" => (op.clone(), 5, false, false),
                        "*" | "/" | "%" => (op.clone(), 6, false, false),
                        "^" => (op.clone(), 8, true, false),
                        _ => break,
                    }
                } else if self.starts_value(self.position) {
                    ("*".into(), 6, false, true)
                } else {
                    break;
                };
            if precedence < minimum {
                break;
            }
            if !implicit {
                self.next();
            }
            let right = self.expression(if right_associative {
                precedence
            } else {
                precedence + 1
            })?;
            if precedence == 3
                && let Some(previous_right) = comparison_tail(&left)
            {
                let next_comparison =
                    Expr::Binary(operator, Box::new(previous_right.clone()), Box::new(right));
                left = checked(Expr::Binary(
                    "and".into(),
                    Box::new(left),
                    Box::new(next_comparison),
                ))?;
                continue;
            }
            left = checked(Expr::Binary(operator, Box::new(left), Box::new(right)))?;
        }
        Ok(left)
    }

    fn starts_value(&self, position: usize) -> bool {
        match self.tokens.get(position) {
            Some(
                Token::Number(..)
                | Token::LeftParen
                | Token::LeftBracket
                | Token::LeftBrace
                | Token::FloorOpen
                | Token::CeilOpen,
            ) => true,
            Some(Token::Bar) => self.bars == 0,
            Some(Token::Word(word)) => !matches!(word.as_str(), "if" | "otherwise" | "to" | "unit"),
            _ => false,
        }
    }

    fn prefix(&mut self) -> Result<Expr, CalcError> {
        match self.next() {
            Token::Number(value, radix) => Ok(Expr::Number(value, radix)),
            Token::Word(word) => self.name(word),
            Token::Op(op) if matches!(op.as_str(), "+" | "-" | "not") => {
                let value = self.expression(if op == "not" { 3 } else { 9 })?;
                checked(Expr::Unary(op, Box::new(value)))
            }
            Token::LeftParen => {
                self.skip_newlines();
                if self.take(&Token::RightParen) {
                    return Ok(Expr::Vector(Vec::new()));
                }
                let first = self.expression(0)?;
                self.skip_newlines();
                if self.take(&Token::Comma) {
                    let mut values = vec![first];
                    values.extend(self.comma_list(Token::RightParen)?);
                    checked(Expr::Vector(values))
                } else {
                    self.expect(Token::RightParen)?;
                    checked(Expr::Group(Box::new(first)))
                }
            }
            Token::LeftBracket => self.bracket(),
            Token::LeftBrace => self.brace(),
            Token::Bar => {
                self.bars += 1;
                let value = self.expression(0)?;
                self.bars -= 1;
                self.expect(Token::Bar)?;
                checked(Expr::Call("abs".into(), vec![value]))
            }
            Token::FloorOpen => {
                let value = self.expression(0)?;
                self.expect(Token::FloorClose)?;
                checked(Expr::Call("floor".into(), vec![value]))
            }
            Token::CeilOpen => {
                let value = self.expression(0)?;
                self.expect(Token::CeilClose)?;
                checked(Expr::Call("ceil".into(), vec![value]))
            }
            token => Err(error(format!("Expected an expression, found {token:?}"))),
        }
    }

    fn name(&mut self, mut name: String) -> Result<Expr, CalcError> {
        let log_base = name
            .strip_prefix("log_")
            .filter(|base| !base.is_empty() && base.chars().all(|c| c.is_ascii_digit()))
            .map(String::from)
            .or_else(|| {
                name.strip_prefix("log")
                    .filter(|base| !base.is_empty() && base.chars().all(|c| subscript(c).is_some()))
                    .map(|base| base.chars().filter_map(subscript).collect())
            });
        let is_atomic_name = log_base.is_some()
            || self.functions.contains(&name)
            || self.variables.contains(&name)
            || self.units.contains(&name)
            || (self.integral > 0 && name.starts_with('d') && name.len() > 1);
        if !is_atomic_name {
            let prefix = self
                .functions
                .iter()
                .chain(self.variables.iter())
                .chain(self.units.iter())
                .filter(|candidate| {
                    name.starts_with(candidate.as_str()) && candidate.len() < name.len()
                })
                .max_by_key(|candidate| candidate.len())
                .cloned();
            let prefix = prefix.or_else(|| {
                if !name.contains('_')
                    && !name.chars().any(|c| subscript(c).is_some())
                    && self.peek() != &Token::LeftParen
                {
                    name.chars()
                        .next()
                        .map(|c| c.to_string())
                        .filter(|first| first.len() < name.len())
                } else {
                    None
                }
            });
            if let Some(prefix) = prefix {
                let remainder = &name[prefix.len()..];
                let mut tail = lex(remainder)?;
                tail.pop();
                self.tokens.splice(self.position..self.position, tail);
                name = prefix;
            }
        }
        if name == "true" || name == "false" {
            return Ok(Expr::Boolean(name == "true"));
        }
        let known_function = self.functions.contains(&name) || log_base.is_some();
        if self.take(&Token::Inverse) {
            name = format!("a{name}");
        }
        let mut primes = 0;
        while self.take(&Token::Prime) {
            primes += 1;
        }
        let function = known_function
            || primes > 0
            || (!self.variables.contains(&name)
                && !self.units.contains(&name)
                && self.peek() == &Token::LeftParen);
        if function {
            let is_integral = matches!(name.as_str(), "integrate" | "integral");
            if is_integral {
                self.integral += 1;
            }
            let mut arguments = if self.take(&Token::LeftParen) {
                self.comma_list(Token::RightParen)?
            } else {
                // Numeric juxtaposition accepts a full multiplicative argument
                // (sin2x), while a named argument is a single atom (sinx^2).
                let minimum = if matches!(self.peek(), Token::Word(_)) {
                    11
                } else {
                    6
                };
                vec![self.expression(minimum)?]
            };
            if is_integral {
                self.integral -= 1;
            }
            if let Some(base) = log_base {
                name = "log".into();
                arguments.push(Expr::Number(base, 10));
            }
            if primes > 0 {
                checked(Expr::Derivative(name, primes, arguments))
            } else {
                checked(Expr::Call(name, arguments))
            }
        } else {
            Ok(Expr::Name(name))
        }
    }

    fn comma_list(&mut self, closing: Token) -> Result<Vec<Expr>, CalcError> {
        let mut values = Vec::new();
        self.skip_newlines();
        if self.take(&closing) {
            return Ok(values);
        }
        loop {
            values.push(self.expression(0)?);
            self.skip_newlines();
            if self.take(&closing) {
                break;
            }
            self.expect(Token::Comma)?;
        }
        Ok(values)
    }

    fn bracket(&mut self) -> Result<Expr, CalcError> {
        self.skip_separators();
        if self.take(&Token::RightBracket) {
            return Ok(Expr::Vector(Vec::new()));
        }
        let first = self.expression(0)?;
        if self.take(&Token::Colon) {
            let conditions = self.comma_list(Token::RightBracket)?;
            return checked(Expr::Comprehension(Box::new(first), conditions));
        }
        let mut rows = Vec::new();
        let mut row = vec![first];
        let mut matrix = false;
        loop {
            if self.take(&Token::RightBracket) {
                rows.push(row);
                break;
            }
            if self.take_separator() {
                matrix = true;
                rows.push(row);
                row = Vec::new();
                self.skip_separators();
                if self.take(&Token::RightBracket) {
                    break;
                }
            } else {
                self.expect(Token::Comma)?;
            }
            row.push(self.expression(0)?);
        }
        if matrix {
            checked(Expr::Matrix(rows))
        } else {
            checked(Expr::Vector(rows.pop().unwrap_or_default()))
        }
    }

    fn brace(&mut self) -> Result<Expr, CalcError> {
        self.skip_separators();
        let first = self.expression(0)?;
        if self.take_word("if") {
            let condition = self.expression(0)?;
            let mut branches = vec![(first, Some(condition))];
            loop {
                if self.take(&Token::RightBrace) {
                    break;
                }
                if !self.take_separator() {
                    return Err(error("Expected a separator between piecewise branches"));
                }
                self.skip_separators();
                if self.take(&Token::RightBrace) {
                    break;
                }
                let value = self.expression(0)?;
                let condition = if self.take_word("if") {
                    Some(self.expression(0)?)
                } else if self.take_word("otherwise") {
                    None
                } else {
                    return Err(error(
                        "Expected 'if' or 'otherwise' in a piecewise expression",
                    ));
                };
                branches.push((value, condition));
            }
            checked(Expr::Piecewise(branches))
        } else if self.take_word("otherwise") {
            self.skip_separators();
            self.expect(Token::RightBrace)?;
            checked(Expr::Piecewise(vec![(first, None)]))
        } else {
            let mut equations = vec![first];
            while !self.take(&Token::RightBrace) {
                if !self.take_separator() {
                    return Err(error("Expected a separator between equations"));
                }
                self.skip_separators();
                if self.take(&Token::RightBrace) {
                    break;
                }
                equations.push(self.expression(0)?);
            }
            checked(Expr::Equations(equations))
        }
    }
}

fn comparison_tail(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Binary(op, _, right)
            if matches!(op.as_str(), "=" | "!=" | "<" | "<=" | ">" | ">=") =>
        {
            Some(right)
        }
        Expr::Binary(op, _, right) if op == "and" => comparison_tail(right),
        _ => None,
    }
}

fn checked(expr: Expr) -> Result<Expr, CalcError> {
    if expression_depth(&expr) > MAX_RECURSION_DEPTH {
        Err(recursion_error())
    } else {
        Ok(expr)
    }
}

fn expression_depth(expr: &Expr) -> u32 {
    let depth = match expr {
        Expr::Number(..) | Expr::Boolean(..) | Expr::Name(..) => 0,
        Expr::Group(value)
        | Expr::Unary(_, value)
        | Expr::Unit(value, _)
        | Expr::Convert(value, _) => expression_depth(value),
        Expr::Binary(_, left, right) => expression_depth(left).max(expression_depth(right)),
        Expr::Call(_, values)
        | Expr::Derivative(_, _, values)
        | Expr::Vector(values)
        | Expr::Equations(values) => values.iter().map(expression_depth).max().unwrap_or(0),
        Expr::Matrix(rows) => rows
            .iter()
            .flatten()
            .map(expression_depth)
            .max()
            .unwrap_or(0),
        Expr::Index(value, indices) | Expr::Comprehension(value, indices) => {
            expression_depth(value).max(indices.iter().map(expression_depth).max().unwrap_or(0))
        }
        Expr::Piecewise(branches) => branches
            .iter()
            .map(|(value, condition)| {
                expression_depth(value).max(condition.as_ref().map(expression_depth).unwrap_or(0))
            })
            .max()
            .unwrap_or(0),
    };
    depth + 1
}

fn collect_names(expr: &Expr, names: &mut Vec<String>) {
    match expr {
        Expr::Name(name) => names.push(name.clone()),
        Expr::Group(value)
        | Expr::Unary(_, value)
        | Expr::Unit(value, _)
        | Expr::Convert(value, _) => collect_names(value, names),
        Expr::Binary(_, left, right) => {
            collect_names(left, names);
            collect_names(right, names);
        }
        Expr::Call(_, values)
        | Expr::Derivative(_, _, values)
        | Expr::Vector(values)
        | Expr::Equations(values) => {
            for value in values {
                collect_names(value, names);
            }
        }
        Expr::Matrix(rows) => {
            for value in rows.iter().flatten() {
                collect_names(value, names);
            }
        }
        Expr::Index(value, indices) | Expr::Comprehension(value, indices) => {
            collect_names(value, names);
            for index in indices {
                collect_names(index, names);
            }
        }
        Expr::Piecewise(branches) => {
            for (value, condition) in branches {
                collect_names(value, names);
                if let Some(condition) = condition {
                    collect_names(condition, names);
                }
            }
        }
        Expr::Number(..) | Expr::Boolean(..) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calculator::{CalcRequest, evaluate};

    fn formatted(source: &str) -> String {
        let request: CalcRequest =
            serde_json::from_value(serde_json::json!({"expression": source})).unwrap();
        evaluate(request)
            .unwrap_or_else(|error| panic!("{source}: {error}"))
            .result
            .unwrap()
            .formatted
    }

    #[test]
    fn comparison_context_distinguishes_definitions_from_checks() {
        assert_eq!(formatted("x=2; x=2 and x>1"), "true");
        assert_eq!(formatted("x=2; x=3<4"), "false");
        assert_eq!(formatted("x=3; f(x)=2x; f(x)=6 and f3=6"), "true");
        assert_eq!(formatted("f(a,b,c)=a*b=c; f(2,3,6)"), "true");
        assert_eq!(formatted("true=(1=1); false=(1!=1)"), "true");
        assert!(matches!(
            parse("x=(2<3)", &[], &[], &[]).unwrap().as_slice(),
            [Statement::Variable(_, _)]
        ));
    }

    #[test]
    fn compact_calls_and_multiplication_keep_their_precedence() {
        assert_eq!(formatted("x=3; 2sqrt(64)/3x+2"), "18");
        assert_eq!(formatted("x=3; 2/sqrt(64)3x+2"), "4.25");
        assert_eq!(formatted("x=3; sin2x=sin(2*x) and sin2*x=sin(2*x)"), "true");
        assert_eq!(
            formatted("x=3; sinx^2=sin(x)^2 and sinx/2=sin(x)/2"),
            "true"
        );
        assert_eq!(formatted("sqrt2sqrt4"), "2");
        assert_eq!(formatted("integrate(0,pi,sinxdx)=2"), "true");
    }

    #[test]
    fn newlines_continue_operators_and_separate_matrix_rows() {
        assert_eq!(formatted("x=3\nx=3 and\n2+\n3=5"), "true");
        assert_eq!(formatted("[1,2\n3,4]"), "[1, 2; 3, 4]");
        assert_eq!(formatted("sum(\n1,\n2,\n3\n)"), "6");
        assert!(parse("2+;3", &[], &[], &[]).is_err());
    }

    #[test]
    fn numeric_tokens_preserve_exact_digits_and_exponent_fallback() {
        assert!(
            matches!(parse("0x20000000000001", &[], &[], &[]).unwrap().as_slice(),
            [Statement::Expression(Expr::Number(digits, 16))] if digits == "20000000000001")
        );
        assert_eq!(formatted("E=3; 2E + 1E2"), "106");
        assert_eq!(formatted("2mod3"), "2");
        assert_eq!(formatted("10%-50"), "10");
        assert_eq!(formatted("(10%+50)=50.1"), "true");
    }

    #[test]
    fn parser_bounds_recursive_and_flat_tree_depth() {
        for source in [
            format!("{}1{}", "(".repeat(300), ")".repeat(300)),
            "1+".repeat(300) + "1",
            "1^".repeat(300) + "1",
        ] {
            assert_eq!(
                parse(&source, &[], &[], &[]).unwrap_err().code,
                "recursion_limit"
            );
        }
        assert!(parse(&format!("[{}1]", "1,".repeat(300)), &[], &[], &[]).is_ok());
    }
}
