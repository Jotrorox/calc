//! Exercise the real HTTP server and isolated calculator workers without a test HTTP dependency.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const EXPENSIVE_EXPRESSION: &str = "sum(n=1, 1000000000000, n)";

struct Server {
    child: Child,
    address: SocketAddr,
}

impl Server {
    fn start() -> Self {
        Self::with_options(2_000, 4)
    }

    fn with_options(timeout_ms: u32, workers: u32) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test port");
        let address = listener.local_addr().expect("reserved port address");
        drop(listener);
        let child = Command::new(env!("CARGO_BIN_EXE_calc-api"))
            .env("HOST", "127.0.0.1")
            .env("PORT", address.port().to_string())
            .env("CALC_TIMEOUT_MS", timeout_ms.to_string())
            .env("CALC_MAX_WORKERS", workers.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start calc-api");
        let mut server = Self { child, address };
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            if let Some(status) = server.child.try_wait().expect("check server process") {
                let mut error = String::new();
                if let Some(mut stderr) = server.child.stderr.take() {
                    let _ = stderr.read_to_string(&mut error);
                }
                panic!("server exited before listening: {status}: {error}");
            }
            if TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_ok() {
                return server;
            }
            assert!(Instant::now() < deadline, "server did not start in time");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn request(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Response {
        request(self.address, method, path, headers, body)
    }

    fn calc(&self, body: Value) -> Response {
        self.request(
            "POST",
            "/calc",
            &[("Content-Type", "application/json")],
            &serde_json::to_vec(&body).expect("encode test input"),
        )
    }

    fn expression(&self, expression: &str) -> Response {
        self.calc(json!({ "expression": expression }))
    }

    fn health(&self) -> Response {
        self.request("GET", "/health", &[], &[])
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct Response {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Response {
    fn json(&self) -> Value {
        assert!(
            self.headers
                .get("content-type")
                .is_some_and(|content_type| content_type.starts_with("application/json")),
            "expected JSON response: {self:?}"
        );
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|error| panic!("invalid JSON response ({error}): {self:?}"))
    }

    fn success(&self) -> Value {
        assert_eq!(self.status, 200, "unexpected response: {self:?}");
        self.json()
    }

    fn error(&self, expected_status: u16) -> Value {
        assert_eq!(
            self.status, expected_status,
            "unexpected response: {self:?}"
        );
        let body = self.json();
        assert!(
            body["error"]["code"]
                .as_str()
                .is_some_and(|code| !code.is_empty()),
            "{body}"
        );
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
            "{body}"
        );
        assert!(
            body.get("result").is_none(),
            "error includes a result: {body}"
        );
        body
    }

    fn real(&self) -> f64 {
        let body = self.success();
        assert_eq!(body["result"]["value"]["type"], "number", "{body}");
        assert_eq!(
            body["result"]["value"]["imaginary"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap(),
            0.0,
            "unexpected imaginary component: {body}"
        );
        number_component(&body["result"]["value"], "real")
    }
}

fn number_component(value: &Value, component: &str) -> f64 {
    value[component]
        .as_str()
        .unwrap_or_else(|| panic!("missing string {component}: {value}"))
        .parse()
        .unwrap_or_else(|error| panic!("non-numeric {component}: {value}: {error}"))
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} ± {tolerance}, got {actual}"
    );
}

fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Response {
    let mut stream = TcpStream::connect_timeout(&address, IO_TIMEOUT).expect("connect to API");
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .expect("set write timeout");
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .expect("write HTTP headers");
    stream.write_all(body).expect("write HTTP body");
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .expect("read HTTP response before deadline");
    let separator = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP header separator");
    let header_text = std::str::from_utf8(&raw[..separator]).expect("UTF-8 HTTP headers");
    let mut lines = header_text.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: BTreeMap<String, String> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("HTTP header colon");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let body = raw[separator + 4..].to_vec();
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        decode_chunks(&body)
    } else {
        body
    };
    if method != "HEAD"
        && let Some(length) = headers.get("content-length")
    {
        assert_eq!(
            body.len(),
            length.parse::<usize>().unwrap(),
            "truncated HTTP body"
        );
    }
    Response {
        status,
        headers,
        body,
    }
}

fn decode_chunks(mut input: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    loop {
        let end = input
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("chunk size delimiter");
        let size = std::str::from_utf8(&input[..end])
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let size = usize::from_str_radix(size, 16).expect("hexadecimal chunk size");
        input = &input[end + 2..];
        if size == 0 {
            return result;
        }
        assert!(input.len() >= size + 2, "truncated chunk");
        result.extend_from_slice(&input[..size]);
        assert_eq!(&input[size..size + 2], b"\r\n");
        input = &input[size + 2..];
    }
}

#[test]
fn health_is_json_and_head_has_no_body() {
    let server = Server::start();
    assert_eq!(server.health().success(), json!({ "status": "ok" }));
    let response = server.request("HEAD", "/health", &[], &[]);
    assert_eq!(response.status, 200);
    assert!(response.body.is_empty());
}

#[test]
fn routes_and_methods_are_restricted() {
    let server = Server::start();
    for path in ["/", "/missing", "/calc/", "/health/"] {
        server.request("GET", path, &[], &[]).error(404);
    }
    for (method, path) in [
        ("GET", "/calc"),
        ("PUT", "/calc"),
        ("DELETE", "/calc"),
        ("POST", "/health"),
    ] {
        server.request(method, path, &[], &[]).error(405);
    }
}

#[test]
fn calculation_accepts_json_media_types() {
    let server = Server::start();
    for media_type in [
        "application/json",
        "application/json; charset=utf-8",
        "Application/JSON",
    ] {
        let response = server.request(
            "POST",
            "/calc",
            &[("Content-Type", media_type)],
            br#"{"expression":"6 * 7"}"#,
        );
        assert_eq!(response.real(), 42.0, "media type {media_type}");
    }
}

#[test]
fn calculation_requires_a_json_media_type() {
    let server = Server::start();
    server
        .request("POST", "/calc", &[], br#"{"expression":"1"}"#)
        .error(415);
    for media_type in [
        "text/plain",
        "application/x-www-form-urlencoded",
        "text/json",
        "application/jsonp",
    ] {
        server
            .request(
                "POST",
                "/calc",
                &[("Content-Type", media_type)],
                br#"{"expression":"1"}"#,
            )
            .error(415);
    }
}

#[test]
fn malformed_json_and_wrong_shapes_are_rejected() {
    let server = Server::start();
    for body in [
        "",
        "{",
        "null",
        "[]",
        "42",
        "true",
        "\"1+2\"",
        "{}",
        r#"{"expression":1}"#,
        r#"{"expression":null}"#,
        r#"{"expression":true}"#,
        r#"{"expression":[]}"#,
        r#"{"expression":"1",}"#,
        r#"{"expression":"1"} {"expression":"2"}"#,
        r#"{"expression":"1","expression":"2"}"#,
        r#"{"expression":"1","unexpected":true}"#,
    ] {
        let response = server.request(
            "POST",
            "/calc",
            &[("Content-Type", "application/json")],
            body.as_bytes(),
        );
        response.error(400);
    }
    server
        .request(
            "POST",
            "/calc",
            &[("Content-Type", "application/json")],
            b"{\"expression\":\"\xff\"}",
        )
        .error(400);
    assert_eq!(server.expression("2+2").real(), 4.0);
}

#[test]
fn expressions_must_be_nonempty_and_within_byte_limit() {
    let server = Server::start();
    for expression in ["", " ", "\t\r\n", "\u{2003}"] {
        server.expression(expression).error(400);
    }
    server.expression(&"1".repeat(16_385)).error(400);
    // Multibyte input must be bounded in bytes, not Unicode scalar values.
    server.expression(&"π".repeat(8_193)).error(400);
    let boundary = format!("1{}", " ".repeat(16_383));
    assert_eq!(server.expression(&boundary).real(), 1.0);
}

#[test]
fn oversized_http_bodies_are_rejected_before_calculation() {
    let server = Server::start();
    let body = serde_json::to_vec(&json!({ "expression": " ".repeat(32_768) })).unwrap();
    server
        .request(
            "POST",
            "/calc",
            &[("Content-Type", "application/json")],
            &body,
        )
        .error(413);
    assert_eq!(server.health().success()["status"], "ok");
}

#[test]
fn valid_body_near_limit_survives_internal_worker_serialization() {
    let server = Server::start();
    let expression = "1".to_owned() + &"\t".repeat(16_374);
    assert_eq!(expression.len(), 16_375);
    let body = serde_json::to_vec(&json!({ "expression": expression })).unwrap();
    assert_eq!(body.len(), 32_766);
    // Adding default fields to the worker request must not reject an accepted HTTP body.
    let response = server.request(
        "POST",
        "/calc",
        &[("Content-Type", "application/json")],
        &body,
    );
    assert_eq!(response.real(), 1.0);
}

#[test]
fn precision_defaults_and_boundaries_are_enforced() {
    let server = Server::start();
    let default = server.expression("1/3").success();
    assert_eq!(default["precision"], 128);
    assert_eq!(default["angle_unit"], "rad");
    for precision in [32, 33, 64, 128, 256, 4_096] {
        let response = server.calc(json!({ "expression": "1/3", "precision": precision }));
        assert_eq!(response.success()["precision"], precision);
        assert_close(response.real(), 1.0 / 3.0, 1e-9);
    }
    for precision in [
        json!(0),
        json!(31),
        json!(4_097),
        json!(-1),
        json!(1.5),
        json!("128"),
        json!(true),
        json!(null),
        json!(4_294_967_296_u64),
    ] {
        server
            .calc(json!({ "expression": "1", "precision": precision }))
            .error(400);
    }
}

#[test]
fn angle_units_change_trigonometry_and_reject_unknown_values() {
    let server = Server::start();
    assert_close(server.expression("sin(pi/2)").real(), 1.0, 1e-12);
    let degrees = server.calc(json!({ "expression": "sin(90)", "angle_unit": "deg" }));
    assert_eq!(degrees.success()["angle_unit"], "deg");
    assert_close(degrees.real(), 1.0, 1e-12);
    let radians = server.calc(json!({ "expression": "sin(90)", "angle_unit": "rad" }));
    assert_close(radians.real(), 90_f64.sin(), 1e-12);
    for angle_unit in [
        json!("degrees"),
        json!("RAD"),
        json!(""),
        json!(1),
        json!(null),
        json!(false),
    ] {
        server
            .calc(json!({ "expression": "1", "angle_unit": angle_unit }))
            .error(400);
    }
}

#[test]
fn requested_precision_preserves_information_beyond_double_precision() {
    let server = Server::start();
    let expression = "2^200+1-2^200";
    assert_eq!(
        server
            .calc(json!({ "expression": expression, "precision": 256 }))
            .real(),
        1.0
    );
    assert_eq!(
        server
            .calc(json!({ "expression": expression, "precision": 32 }))
            .real(),
        0.0
    );
}

#[test]
fn arithmetic_precedence_functions_and_unicode_work_over_http() {
    let server = Server::start();
    for (expression, expected) in [
        ("2+3*4", 14.0),
        ("(2+3)*4", 20.0),
        ("2^3^2", 512.0),
        ("5!", 120.0),
        ("sqrt(81)", 9.0),
        ("abs(-7)", 7.0),
        ("2(3+4)", 14.0),
        ("sin(π/2)", 1.0),
        ("2π", std::f64::consts::TAU),
        ("ln(e)", 1.0),
        ("log(100)", 2.0),
    ] {
        assert_close(server.expression(expression).real(), expected, 1e-10);
    }
}

#[test]
fn complex_numbers_have_structured_real_and_imaginary_components() {
    let server = Server::start();
    let response = server.expression("(2+3i)*(4-i)").success();
    let value = &response["result"]["value"];
    assert_eq!(value["type"], "number");
    assert_close(number_component(value, "real"), 11.0, 1e-12);
    assert_close(number_component(value, "imaginary"), 10.0, 1e-12);
    assert!(value["unit"].is_null());
    assert!(
        response["result"]["formatted"]
            .as_str()
            .is_some_and(|formatted| !formatted.is_empty())
    );
    let root = server.expression("sqrt(-1)").success();
    assert_close(
        number_component(&root["result"]["value"], "imaginary"),
        1.0,
        1e-12,
    );
}

#[test]
fn vectors_and_matrices_have_structured_elements() {
    let server = Server::start();
    let vector = server.expression("[1,2,3]*2").success();
    let vector = &vector["result"]["value"];
    assert_eq!(vector["type"], "vector");
    let values = vector["values"].as_array().expect("vector entries");
    assert_eq!(values.len(), 3);
    for (value, expected) in values.iter().zip([2.0, 4.0, 6.0]) {
        assert_eq!(value["type"], "number");
        assert_eq!(number_component(value, "real"), expected);
    }
    let matrix = server.expression("[1,2;3,4]").success();
    let matrix = &matrix["result"]["value"];
    assert_eq!(matrix["type"], "matrix");
    let rows = matrix["rows"].as_array().expect("matrix rows");
    assert_eq!(rows.len(), 2);
    for (row, expected) in rows.iter().zip([[1.0, 2.0], [3.0, 4.0]]) {
        let row = row.as_array().expect("matrix row entries");
        assert_eq!(row.len(), 2);
        for (value, expected) in row.iter().zip(expected) {
            assert_eq!(number_component(value, "real"), expected);
        }
    }
}

#[test]
fn calculus_and_summation_work_over_http() {
    let server = Server::start();
    assert_close(
        server.expression("integrate(0, pi, sin(x), dx)").real(),
        2.0,
        1e-8,
    );
    assert_close(server.expression("f(x)=2x^2+x; f'(2)").real(), 9.0, 1e-6);
    assert_eq!(server.expression("sum(n=1, 100, n)").real(), 5_050.0);
}

#[test]
fn boolean_results_are_typed() {
    let server = Server::start();
    for (expression, expected) in [("1 < 2", true), ("2 < 1", false)] {
        let response = server.expression(expression).success();
        assert_eq!(response["result"]["value"]["type"], "boolean");
        assert_eq!(response["result"]["value"]["value"], expected);
    }
}

#[test]
fn user_definitions_are_evaluated_and_requests_are_stateless() {
    let server = Server::start();
    assert_eq!(server.expression("a=7; a*6").real(), 42.0);
    server.expression("a").error(422);
    assert_eq!(server.expression("f(x)=x^2+1; f(4)").real(), 17.0);
    server.expression("f(4)").error(422);
    assert!(server.expression("localvalue=123").success()["result"].is_null());
    server.expression("localvalue").error(422);
    assert_eq!(server.expression("6*7").real(), 42.0);
    server.expression("ans").error(422);
}

#[test]
fn context_replays_previous_inputs_and_answers_in_request_order() {
    let server = Server::start();
    let response = server.calc(json!({
        "expression": "ans*factor",
        "context": ["factor=4", "2+3"]
    }));
    assert_eq!(response.real(), 20.0);
    assert_eq!(
        server
            .calc(json!({
                "expression": "f(ans)",
                "context": ["f(x)=x^2", "5+2"]
            }))
            .real(),
        49.0
    );
    server.expression("factor").error(422);
    server.expression("ans").error(422);
    server.expression("f(2)").error(422);
    assert_eq!(
        server
            .calc(json!({ "expression": "1+2", "context": [] }))
            .real(),
        3.0
    );
}

#[test]
fn context_validation_enforces_types_entry_count_and_combined_byte_limit() {
    let server = Server::start();
    for context in [
        json!(null),
        json!("1"),
        json!([1]),
        json!([null]),
        json!([""]),
        json!(["\t \n"]),
        json!(vec!["1"; 65]),
    ] {
        server
            .calc(json!({ "expression": "2", "context": context }))
            .error(400);
    }
    assert_eq!(
        server
            .calc(json!({ "expression": "ans+1", "context": vec!["1"; 64] }))
            .real(),
        2.0
    );
    server
        .calc(json!({ "expression": "1", "context": [" ".repeat(16_383) + "1"] }))
        .error(400);
    server
        .calc(json!({ "expression": "π", "context": [" ".repeat(16_382) + "1"] }))
        .error(400);
    let context = "1".to_owned() + &" ".repeat(16_380);
    assert_eq!(
        server
            .calc(json!({ "expression": "ans", "context": [context] }))
            .real(),
        1.0
    );
}

#[test]
fn errors_and_deadlines_apply_to_context_evaluation() {
    let server = Server::with_options(250, 1);
    server
        .calc(json!({ "expression": "42", "context": ["unknown_function(1)"] }))
        .error(422);
    let error = server
        .calc(json!({ "expression": "42", "context": [EXPENSIVE_EXPRESSION] }))
        .error(408);
    assert_eq!(error["error"]["code"], "timeout");
    assert_eq!(server.expression("42").real(), 42.0);
}

#[test]
fn invalid_calculations_return_errors_and_workers_recover() {
    let server = Server::start();
    for expression in [
        "unknown_variable",
        "1+",
        "(",
        "sqrt()",
        "unknown_function(1)",
    ] {
        server.expression(expression).error(422);
        assert_eq!(server.expression("3*7").real(), 21.0);
    }
    assert_eq!(server.health().success()["status"], "ok");
}

#[test]
fn expensive_calculation_is_killed_at_deadline_and_worker_slot_recovers() {
    let server = Server::with_options(250, 1);
    let started = Instant::now();
    let error = server.expression(EXPENSIVE_EXPRESSION).error(408);
    assert_eq!(error["error"]["code"], "timeout");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "deadline did not bound execution"
    );
    assert_eq!(server.expression("20+22").real(), 42.0);
    assert_eq!(server.health().success()["status"], "ok");
}

#[test]
fn parser_pathological_input_is_bounded_and_does_not_kill_server() {
    let server = Server::with_options(250, 1);
    let expression = format!("{}1{}", "(".repeat(7_000), ")".repeat(7_000));
    let started = Instant::now();
    let response = server.expression(&expression);
    match response.status {
        200 => assert_eq!(response.real(), 1.0),
        400 | 408 | 422 => {
            response.error(response.status);
        }
        _ => panic!("pathological parser response: {response:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "parser exceeded execution deadline"
    );
    assert_eq!(server.health().success()["status"], "ok");
    assert_eq!(server.expression("40+2").real(), 42.0);
}

#[test]
fn concurrency_limit_is_enforced_and_health_remains_available() {
    let server = Server::with_options(700, 1);
    let barrier = Arc::new(Barrier::new(3));
    let mut calculations = Vec::new();
    for _ in 0..2 {
        let barrier = Arc::clone(&barrier);
        let address = server.address;
        calculations.push(thread::spawn(move || {
            barrier.wait();
            request(
                address,
                "POST",
                "/calc",
                &[("Content-Type", "application/json")],
                &serde_json::to_vec(&json!({ "expression": EXPENSIVE_EXPRESSION })).unwrap(),
            )
        }));
    }
    barrier.wait();
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    assert_eq!(server.health().success()["status"], "ok");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "health was blocked by computation"
    );
    let mut statuses = calculations
        .into_iter()
        .map(|calculation| {
            let response = calculation.join().expect("calculation client thread");
            response.error(response.status);
            if response.status == 503 {
                assert_eq!(
                    response.headers.get("retry-after").map(String::as_str),
                    Some("1")
                );
                assert_eq!(response.json()["error"]["code"], "busy");
            }
            response.status
        })
        .collect::<Vec<_>>();
    statuses.sort_unstable();
    assert_eq!(statuses, [408, 503]);
    assert_eq!(server.expression("21*2").real(), 42.0);
}

#[test]
fn independent_calculations_can_complete_concurrently() {
    let server = Server::with_options(2_000, 4);
    let barrier = Arc::new(Barrier::new(5));
    let clients = (1..=4)
        .map(|value| {
            let address = server.address;
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let body = json!({ "expression": format!("shared={value}; shared^2") });
                let response = request(
                    address,
                    "POST",
                    "/calc",
                    &[("Content-Type", "application/json")],
                    &serde_json::to_vec(&body).unwrap(),
                );
                assert_eq!(response.real(), f64::from(value * value));
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    for client in clients {
        client.join().expect("independent calculation client");
    }
    server.expression("shared").error(422);
}

fn compare_calculator_values(actual: &Value, expected: &Value) -> Result<(), String> {
    if actual["type"] != expected["type"] {
        return Err(format!(
            "different result types: actual={actual}, expected={expected}"
        ));
    }
    match actual["type"].as_str() {
        Some("number") => {
            if actual["unit"] != expected["unit"] {
                return Err(format!(
                    "different units: actual={actual}, expected={expected}"
                ));
            }
            for component in ["real", "imaginary"] {
                let actual_number = number_component(actual, component);
                let expected_number = number_component(expected, component);
                if actual_number == expected_number
                    || (actual_number.is_nan() && expected_number.is_nan())
                {
                    continue;
                }
                let tolerance = 1e-7 * expected_number.abs().max(1.0);
                if (actual_number - expected_number).abs() > tolerance
                    || !actual_number.is_finite()
                    || !expected_number.is_finite()
                {
                    return Err(format!(
                        "different {component}: actual={actual}, expected={expected}, tolerance={tolerance}"
                    ));
                }
            }
        }
        Some("boolean") => {
            if actual["value"] != expected["value"] {
                return Err(format!(
                    "different booleans: actual={actual}, expected={expected}"
                ));
            }
        }
        Some("vector") => compare_calculator_sequences(&actual["values"], &expected["values"])?,
        Some("matrix") => {
            let actual_rows = actual["rows"].as_array().expect("matrix rows");
            let expected_rows = expected["rows"].as_array().expect("expected matrix rows");
            if actual_rows.len() != expected_rows.len() {
                return Err(format!(
                    "different matrix row counts: actual={actual}, expected={expected}"
                ));
            }
            for (actual, expected) in actual_rows.iter().zip(expected_rows) {
                compare_calculator_sequences(actual, expected)?;
            }
        }
        _ => return Err(format!("unrecognized calculator value: {actual}")),
    }
    Ok(())
}

fn compare_calculator_sequences(actual: &Value, expected: &Value) -> Result<(), String> {
    let actual_values = actual.as_array().expect("calculator sequence");
    let expected_values = expected.as_array().expect("expected calculator sequence");
    if actual_values.len() != expected_values.len() {
        return Err(format!(
            "different sequence lengths: actual={actual}, expected={expected}"
        ));
    }
    for (actual, expected) in actual_values.iter().zip(expected_values) {
        compare_calculator_values(actual, expected)?;
    }
    Ok(())
}

fn validate_calculator_value(value: &Value) -> Result<(), String> {
    match value["type"].as_str() {
        Some("number") => {
            for component in ["real", "imaginary"] {
                if value[component]
                    .as_str()
                    .is_none_or(|number| number.parse::<f64>().is_err())
                {
                    return Err(format!("invalid numeric string {component}: {value}"));
                }
            }
            if !value["unit"].is_null() && !value["unit"].is_string() {
                return Err(format!("invalid unit: {value}"));
            }
        }
        Some("boolean") => {
            if !value["value"].is_boolean() {
                return Err(format!("invalid boolean: {value}"));
            }
        }
        Some("vector") => {
            let values = value["values"]
                .as_array()
                .ok_or_else(|| format!("invalid vector: {value}"))?;
            for value in values {
                validate_calculator_value(value)?;
            }
        }
        Some("matrix") => {
            let rows = value["rows"]
                .as_array()
                .ok_or_else(|| format!("invalid matrix: {value}"))?;
            let mut width = None;
            for row in rows {
                let row = row
                    .as_array()
                    .ok_or_else(|| format!("invalid matrix row: {row}"))?;
                if width.is_some_and(|width| width != row.len()) {
                    return Err(format!("inconsistent matrix row widths: {value}"));
                }
                width = Some(row.len());
                for value in row {
                    validate_calculator_value(value)?;
                }
            }
        }
        _ => return Err(format!("unrecognized calculator value: {value}")),
    }
    Ok(())
}

#[test]
fn documented_capability_corpus_executes_through_the_public_api() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("../docs/capability-cases.json"))
        .expect("valid documented capability corpus");
    assert!(
        cases.len() >= 150,
        "capability coverage unexpectedly shrank"
    );
    let server = Server::start();
    let mut failures = Vec::new();
    for case in cases {
        let expression = case["expression"].as_str().expect("documented expression");
        let category = case["category"].as_str().expect("documented category");
        let mut request = json!({ "expression": expression });
        for option in ["context", "precision", "angle_unit"] {
            if let Some(value) = case.get(option) {
                request[option] = value.clone();
            }
        }
        let response = server.calc(request);
        if response.status != 200 {
            failures.push(format!(
                "{category}: {expression}: HTTP {}: {}",
                response.status,
                response.json()
            ));
            continue;
        }
        let response = response.success();
        let actual = &response["result"]["value"];
        if let Err(error) = validate_calculator_value(actual) {
            failures.push(format!("{category}: {expression}: {error}"));
            continue;
        }
        if let Some(expected) = case["expected"].as_str() {
            // The expected expression is a separately parsed mathematical literal,
            // keeping the fixture readable for scalar, complex, vector and matrix values.
            let mut expected_request = json!({ "expression": expected });
            for option in ["precision", "angle_unit"] {
                if let Some(value) = case.get(option) {
                    expected_request[option] = value.clone();
                }
            }
            let expected_response = server.calc(expected_request);
            if expected_response.status != 200 {
                failures.push(format!(
                    "{category}: invalid expected literal {expected}: {}",
                    expected_response.json()
                ));
                continue;
            }
            let expected_response = expected_response.success();
            if let Err(error) =
                compare_calculator_values(actual, &expected_response["result"]["value"])
            {
                failures.push(format!("{category}: {expression}: {error}"));
            }
        }
        if response["result"]["formatted"]
            .as_str()
            .is_none_or(|formatted| formatted.is_empty())
        {
            failures.push(format!(
                "{category}: {expression}: missing formatted result"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "documented capability failures:\n{}",
        failures.join("\n")
    );
}
