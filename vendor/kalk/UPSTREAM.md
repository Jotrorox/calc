# Upstream provenance

Source: https://github.com/PaddiM8/kalker

Revision: `a756ffc76ef083032c2972d73bc3104919fb1f4c`

This directory contains the upstream `kalk` crate. Its integration fixtures are
preserved in `../tests`, matching the directory layout expected by the upstream
test suite. `LICENSE` is the upstream MIT license.

One API-only source patch adds `CalculationResult::value() -> &KalkValue` outside
the `wasm-bindgen` implementation. Upstream exposes the result through formatted
strings and `f64` getters, while keeping its native value getter crate-private.
The accessor lets this service serialize numbers, complex components, units,
vectors, and matrices without discarding arbitrary precision. It does not change
parsing or calculation behavior.

The service uses upstream's default arbitrary-precision features. The lockfile
pins transitive dependencies. The adapter and resource limits live outside this
crate.
