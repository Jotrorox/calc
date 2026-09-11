# Calculation engine

The language implementation belongs to this project and lives under `src/engine`:

- `parser.rs` tokenizes source and builds the syntax tree. It handles declarations, operator precedence, implied multiplication, Unicode aliases, collections, conditionals, and mathematical notation.
- `runtime.rs` evaluates programs in a request-local environment. It owns definitions, function scopes, units, iteration, numerical calculus, equation solving, recursion limits, and the cooperative deadline.
- `value.rs` implements typed values, arithmetic, collection operations, and built-in functions.
- `src/calculator.rs` validates API requests and serializes lossless decimal components and recursive values.
- `src/main.rs` supervises isolated worker processes and implements the HTTP contract and resource limits.

These are original implementations; no Kalker source, crate, WebAssembly module, or executable is required. The former `vendor` workspace member, its fixtures, and its build configuration have been removed. The engine uses the general-purpose `rug` library and its GMP/MPFR/MPC numeric backend; HTTP and JSON also continue to use their existing Rust libraries. Those libraries remain external dependencies.

## Compatibility

The public request and response shapes, routes, angle settings, precision range, context replay, error categories, and worker isolation remain unchanged. Compatibility includes the established unary-minus precedence, relative percentages, lazy variable definitions, one-based collection indexes, and request-local `ans`.

`docs/capability-cases.json` retains all 227 documented examples. `tests/fixtures/compatibility.json` records their structured results from the previous release, captured before replacing the engine. `tests/compatibility.rs` compares the new implementation with those fixed values, including types, units, collection dimensions, and both complex components. Numerical comparisons permit a relative/absolute tolerance of `1e-7` because numerical algorithms and precision of constants can change; dedicated exact-value tests additionally protect large integer arithmetic and precision beyond `f64`. Expected snapshot values are never evaluated by the new engine.

The HTTP suite runs the same documented programs through actual worker processes and tests statelessness, deadlines, capacity, malformed input, and recovery. Parser and engine tests cover additional language cases and invalid inputs.

This is a numerical calculator. Numerical results are not promised to match every last digit of the previous implementation, and error message wording can differ. Malformed matrices and indexes are validated rather than relying on accidental behavior. Arithmetic uses the selected binary precision, while numerical differentiation, integration, and solving retain finite approximation tolerances. See [capabilities](capabilities.md) for syntax and limits.
