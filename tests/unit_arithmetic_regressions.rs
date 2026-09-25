//! Unit-aware collection/reduction results must follow scalar conversion rules.
use calc_api::calculator::{CalcRequest, CalcValue, evaluate};

fn request(expression: &str) -> CalcRequest {
    serde_json::from_value(serde_json::json!({"expression": expression})).unwrap()
}

fn eval(expression: &str) -> CalcValue {
    evaluate(request(expression))
        .unwrap_or_else(|error| panic!("{expression}: {error}"))
        .result
        .unwrap()
        .value
}

fn same(definitions: &str, actual: &str, expected: &str) {
    assert_eq!(
        eval(&format!("{definitions};{actual}")),
        eval(&format!("{definitions};{expected}")),
        "{actual} versus {expected}"
    );
}

const LENGTH: &str = "unit cm=100m";
const TEMPERATURE: &str = "unit F=C*9/5+32";

#[test]
fn sums_and_averages_match_scalar_addition_in_both_orders() {
    for (left, right, total) in [("250cm", "1m", "350cm"), ("1m", "250cm", "3.5m")] {
        same(LENGTH, &format!("{left}+{right}"), total);
        for arguments in [format!("{left},{right}"), format!("[{left},{right}]")] {
            same(LENGTH, &format!("sum({arguments})"), total);
            same(
                LENGTH,
                &format!("average({arguments})"),
                &format!("({left}+{right})/2"),
            );
        }
    }
}

#[test]
fn collection_addition_and_subtraction_match_each_scalar_leaf() {
    for op in ["+", "-"] {
        for (left, right) in [("250cm", "1m"), ("1m", "250cm")] {
            let scalar = format!("{left}{op}{right}");
            for expression in [
                format!("[{left}]{op}[{right}]"),
                format!("[{left}]{op}{right}"),
                format!("{left}{op}[{right}]"),
            ] {
                same(LENGTH, &expression, &format!("[{scalar}]"));
            }
            same(
                LENGTH,
                &format!("[[{left}], [{left};{left}]]{op}[[{right}], [{right};{right}]]"),
                &format!("[[{scalar}], [{scalar};{scalar}]]"),
            );
        }
        same(
            LENGTH,
            &format!("[250cm,1m;1m,250cm]{op}[1m,250cm;250cm,1m]"),
            &format!("[250cm{op}1m,1m{op}250cm;1m{op}250cm,250cm{op}1m]"),
        );
        same(
            LENGTH,
            &format!("[250cm,1m;1m,250cm]{op}[1m,250cm]"),
            &format!("[250cm{op}1m,1m{op}1m;1m{op}250cm,250cm{op}250cm]"),
        );
        same(
            LENGTH,
            &format!("[1m,250cm]{op}[250cm,1m;1m,250cm]"),
            &format!("[1m{op}250cm,1m{op}1m;250cm{op}1m,250cm{op}250cm]"),
        );
        same(
            LENGTH,
            &format!("[250cm,1m;1m,250cm]{op}1m"),
            &format!("[250cm{op}1m,1m{op}1m;1m{op}1m,250cm{op}1m]"),
        );
        same(
            LENGTH,
            &format!("1m{op}[250cm,1m;1m,250cm]"),
            &format!("[1m{op}250cm,1m{op}1m;1m{op}1m,1m{op}250cm]"),
        );
    }
}

#[test]
fn reductions_preserve_collection_shape_and_convert_nested_leaves() {
    for (left, right) in [("250cm", "1m"), ("1m", "250cm")] {
        for name in ["sum", "average"] {
            let scalar = if name == "sum" {
                format!("{left}+{right}")
            } else {
                format!("({left}+{right})/2")
            };
            same(
                LENGTH,
                &format!("{name}([[{left}],[{right}]])"),
                &format!("[{scalar}]"),
            );
            same(
                LENGTH,
                &format!("{name}([[{left}]],[[{right}]])"),
                &format!("[[{scalar}]]"),
            );
            same(
                LENGTH,
                &format!("{name}([{left};{left}],[{right};{right}])"),
                &format!("[{scalar};{scalar}]"),
            );
        }
    }
}

#[test]
fn offset_units_follow_scalar_rules_not_physical_temperature_algebra() {
    for (left, right, total) in [("100C", "32F", "100C"), ("32F", "100C", "244F")] {
        same(TEMPERATURE, &format!("{left}+{right}"), total);
        same(TEMPERATURE, &format!("sum({left},{right})"), total);
        same(
            TEMPERATURE,
            &format!("average([{left},{right}])"),
            &format!("({left}+{right})/2"),
        );
        for op in ["+", "-"] {
            same(
                TEMPERATURE,
                &format!("[{left}]{op}[{right}]"),
                &format!("[{left}{op}{right}]"),
            );
        }
    }
}

#[test]
fn equality_converts_matching_nested_leaves_and_remains_a_boolean() {
    for op in ["=", "!=", "≠"] {
        for (left, right) in [("250cm", "2.5m"), ("2.5m", "250cm"), ("250cm", "1m")] {
            same(
                LENGTH,
                &format!("([[{left}],[{left};{left}]]{op}[[{right}],[{right};{right}]])"),
                &format!("({left}{op}{right})"),
            );
        }
        same(
            TEMPERATURE,
            &format!("([32F]{op}[0C])"),
            &format!("(32F{op}0C)"),
        );
        same(
            TEMPERATURE,
            &format!("([0C]{op}[32F])"),
            &format!("(0C{op}32F)"),
        );
    }
    same(LENGTH, "([180deg]=[pi rad])", "(180deg=pi rad)");
    same(LENGTH, "([1m]=[1])", "false");
    same(LENGTH, "([1m,2m]=[100cm])", "false");
}

#[test]
fn extrema_compare_converted_values_but_return_the_selected_operand() {
    for arguments in ["250cm,1m", "1m,250cm", "[250cm,1m]", "[1m,250cm]"] {
        same(LENGTH, &format!("min({arguments})"), "1m");
        same(LENGTH, &format!("max({arguments})"), "250cm");
    }
    for arguments in ["50cm,1m", "1m,50cm"] {
        same(LENGTH, &format!("min({arguments})"), "50cm");
        same(LENGTH, &format!("max({arguments})"), "1m");
    }
    for arguments in ["10C,32F", "32F,10C"] {
        same(TEMPERATURE, &format!("min({arguments})"), "32F");
        same(TEMPERATURE, &format!("max({arguments})"), "10C");
    }
    same(LENGTH, "min(250cm,2.5m)", "250cm");
    same(LENGTH, "max(2.5m,250cm)", "2.5m");
    same(LENGTH, "min(1,1-1E-10)", "1-1E-10");
}

#[test]
fn incompatible_explicit_units_fail_in_collections_and_reductions() {
    let definitions = "unit cm=100m;unit tick=1000s";
    for (left, right) in [("250cm", "1tick"), ("1tick", "250cm")] {
        for expression in [
            format!("{left}+{right}"),
            format!("[{left}]+[{right}]"),
            format!("[[{left}]]-[[{right}]]"),
            format!("[{left};{left}]+{right}"),
            format!("([{left}]=[{right}])"),
            format!("([{left}]!=[{right}])"),
            format!("sum({left},{right})"),
            format!("sum([[{left}],[{right}]])"),
            format!("average([{left},{right}])"),
            format!("min({left},{right})"),
            format!("max({left},{right})"),
        ] {
            let error =
                evaluate(request(&format!("{definitions};{expression}"))).expect_err(&expression);
            assert_eq!(error.code, "calculation_error", "{expression}");
            assert!(
                error.message.contains("Cannot convert"),
                "{expression}: {error}"
            );
        }
    }
}

#[test]
fn unitless_broadcasting_and_documented_algebra_are_unchanged() {
    for (actual, expected) in [
        ("[250cm]+1", "[251cm]"),
        ("1+[250cm]", "[251cm]"),
        ("sum(250cm,1)", "251cm"),
        ("sum(1,250cm)", "251cm"),
        ("average(250cm,1)", "125.5cm"),
        ("sum([1,2;3,4])", "[1,2;3,4]"),
        ("prod([1,2;3,4])", "[1,2;3,4]"),
        ("[1,2;3,4]+[10,20]", "[11,12;23,24]"),
        ("[10,20]-[1,2;3,4]", "[9,8;17,16]"),
        ("[1,2;3,4]*[5,6;7,8]", "[19,22;43,50]"),
        ("[1,2;3,4]*[2,3]", "[8,18]"),
        ("[2,3]*[1,2;3,4]", "[8,18]"),
        ("[2,3,5]*[7,11,13]", "112"),
        ("[1,2;3,4]^2", "[7,10;15,22]"),
        ("[4,9;12,3]/[2,3;4,3]", "[2,3;3,1]"),
        ("sum([[1,2],[3,4]])", "[4,6]"),
        ("average([[1,2],[3,4]])", "[2,3]"),
        ("sum(k=1,2,[k,k+1])", "[3,5]"),
        (
            "f(k)={[250cm] if k=1;[1m] otherwise};sum(k=1,2,f(k))",
            "[350cm]",
        ),
    ] {
        same(LENGTH, actual, expected);
    }
    for expression in [
        "sum()",
        "sum([])",
        "average([])",
        "average([1,2;3,4])",
        "[1,2]+[3]",
    ] {
        assert!(evaluate(request(expression)).is_err(), "{expression}");
    }
}
