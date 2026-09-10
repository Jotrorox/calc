# Calculator API

A Rust JSON API powered by the actual [Kalker calculation engine](https://github.com/PaddiM8/kalker), pinned and vendored at revision `a756ffc76ef083032c2972d73bc3104919fb1f4c`. It exposes two routes: `GET /health` and `POST /calc`.

The engine supports arithmetic, complex numbers, functions and variables, piecewise expressions, vectors, matrices, numerical calculus, equations, number bases, and user-defined units. See the [complete capability reference](docs/capabilities.md), [OpenAPI contract](docs/openapi.json), and [executable capability examples](docs/capability-cases.json). Upstream numerical limitations and syntax quirks are documented there.

## Quick start

Install Rust (the repository pins 1.95.0), a C compiler, `make`, `m4`, and `diffutils`. GMP/MPFR are built from source by the engine; the first build takes several minutes.

```sh
cargo run --release --locked
curl http://localhost:8080/health
curl http://localhost:8080/calc \
  -H 'Content-Type: application/json' \
  -d '{"expression":"2 + 3 * 4"}'
```

The health response is `{"status":"ok"}`. The calculation response is:

```json
{
  "result": {
    "formatted": "14",
    "value": {"type":"number", "real":"14", "imaginary":"0", "unit":null}
  },
  "precision": 128,
  "angle_unit": "rad"
}
```

## Requests and results

`POST /calc` requires `Content-Type: application/json` (an optional charset parameter is accepted). Unknown fields and malformed JSON are rejected.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `expression` | string, required | — | Final expression or multiline/semicolon-separated program; returns its last result. |
| `context` | string array | `[]` | Up to 64 earlier programs evaluated in order before `expression`, for definitions or `ans`. |
| `precision` | integer | `128` | Binary floating-point precision, 32–4096 bits; this is **bits**, not decimal digits. |
| `angle_unit` | string | `"rad"` | `"rad"` or `"deg"` for implicit angular arguments. Explicit angle units are also supported. |

For example:

```json
{"context":["f(x)=x^2","f(5)"],"expression":"ans + f(3)","precision":256}
```

Requests are stateless. Context is discarded after each request, including failures; users cannot overwrite each other's definitions. To use a local Kalker definitions file, send its contents as one entry in `context`. There is no server filesystem loading, terminal UI, or persistent REPL session.

`result` is `null` for a program that only defines a variable/function/unit. Otherwise it contains `formatted` for display and a recursive `value` for programs:

| `value.type` | Additional fields |
| --- | --- |
| `number` | `real` and `imaginary`: decimal strings; `unit`: string or `null` |
| `boolean` | `value`: JSON boolean |
| `vector` | `values`: array of typed values |
| `matrix` | `rows`: array of arrays of typed values |

Decimal components preserve the engine's selected precision without JSON number/f64 conversion. Nonfinite values are represented as strings (`NaN`, `inf`, `-inf`); inspect both components for complex values. A higher precision does not fix upstream numerical algorithms or constants that use f64 internally. Display text is a convenience; use structured values for computation.

## Errors and limits

Errors have the shape `{"error":{"code":"calculation_error","message":"..."}}`. Messages explain the error and may change when the engine is updated.

| HTTP status | Meaning |
| --- | --- |
| 400 | Invalid JSON, unknown fields, empty input, invalid precision, or input limit exceeded |
| 404 / 405 | Unknown endpoint / unsupported HTTP method |
| 408 | Calculation deadline or request-body read deadline exceeded |
| 413 | Request body exceeds 32 KiB |
| 415 | Content type is not `application/json` |
| 422 | Invalid calculation, recursion limit, worker failure, or result too large |
| 503 | All workers are busy; `Retry-After: 1` is included |
| 500 | Server could not start or communicate with a worker |

Combined expression/context input is limited to 16 KiB of UTF-8. The engine's recursion limit is 128. Each calculation runs in a fresh child process with a hard wall-clock deadline, a bounded 2 MiB response, and a 256 MiB virtual-address-space limit on Linux (including Railway). Workers receive no server environment variables. Expired or failed workers are killed and reaped; health checks run independently of calculator capacity. Non-Linux development platforms retain process/time/output isolation but do not apply the Linux memory limit.

| Environment variable | Default | Allowed |
| --- | --- | --- |
| `HOST` | `0.0.0.0` | IP bind address |
| `PORT` | `8080` | 0–65535; Railway provides this automatically |
| `CALC_MAX_WORKERS` | `4` | 1–32 concurrent calculations; excess requests receive 503 |
| `CALC_TIMEOUT_MS` | `2000` | 1–30000 ms hard worker deadline |

The engine also has a cooperative 1500 ms evaluation timeout; the process deadline covers parsing, native arithmetic, and serialization too. Body reads time out after 5 seconds. The service is a public, unauthenticated calculator; deployment has no database or stored request history. CPU-intensive traffic consumes Railway resources within these per-request/concurrency limits.

## Development and tests

```sh
cargo fmt -p calc-api --check
cargo clippy -p calc-api --all-targets --no-deps --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
```

The workspace includes the upstream engine tests and their original fixtures, adapter tests, and black-box HTTP tests that launch the actual server. The capability corpus exercises documented expressions. CI runs formatting, Clippy, tests, and a release build on pushes to `main` and pull requests. Format only `calc-api`; upstream source formatting is intentionally preserved.

There are five application dependencies: `axum`, `tokio`, `serde`, `serde_json`, and `kalk`, plus `libc` on Linux for worker memory limits (already present transitively). Axum has only HTTP/1, JSON, and Tokio features enabled. The engine retains its default arbitrary-precision dependencies and upstream WebAssembly bindings. No database, authentication framework, test HTTP client, or expression reimplementation is added. The sole engine patch is a read-only native result accessor; see [provenance](vendor/kalk/UPSTREAM.md).

## Deployment

The multi-stage Dockerfile builds a release binary with Rust 1.95.0 and runs it as UID 10001 in Debian slim. Railway uses `/health` for deployment health checks and restarts failed instances. Both SIGINT and SIGTERM trigger graceful shutdown.

Railway settings are persisted through the CLI. To reproduce them for another service, run `bash scripts/configure-railway.sh PROJECT_ID ENVIRONMENT_ID SERVICE_ID OWNER/REPO` after the initial deployment. This sets the Docker builder, health checks, restart policy, and GitHub source. The application requires no Railway SDK or deployment runtime dependencies. Legacy `railway.toml`/`railway.json` configuration is deliberately avoided because Railway is retiring it.

```sh
railway up -y --detach -m "Deploy calculator API"
railway deployment list --json
railway domain
railway service source connect --repo Jotrorox/calc --branch main
```

The production service is connected to the private GitHub repository's `main` branch for automatic deployments after GitHub CI succeeds. Its concrete URLs and deployment verification are recorded in [deployment notes](docs/deployment.md). GitHub access is controlled by the repository's private visibility; the API domain itself is public.
