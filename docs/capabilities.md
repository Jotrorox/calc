# Calculator language and capabilities

The API uses the project-owned Rust engine in [`src/engine`](../src/engine), with GMP/MPFR/MPC-backed `rug` arithmetic. Its language preserves the established calculation syntax, complex numbers, collections, numerical methods, and function catalog. It is not a symbolic computer algebra system. See [engine architecture](engine.md) for the compatibility tests and module responsibilities.

Send the expressions below in the request's `expression` field. Definitions can precede a calculation, separated by semicolons or newlines. Every request has a fresh environment. To prepare a sequence of evaluations, provide `context`, an array of expressions evaluated in order before `expression`; this is also how to use the answer from an earlier evaluation:

```json
{"context":["2 + 3"],"expression":"ans * 4"}
```

The result is `20`. Definitions and `ans` last only for that request. Put reusable definitions or the contents of a file of definitions into `context` or `expression`; the server does not load a path from its filesystem. A final declaration has no calculated value.

## Numbers, arithmetic, and grouping

| Capability | Examples | Meaning |
| --- | --- | --- |
| Integers and decimals | `42`, `0.125`, `.5` | Real numbers |
| Scientific notation | `1.25E3`, `3E-2` | `1250`, `0.03`; uppercase `E` |
| Arithmetic | `2 + 3 * 4`, `7 / 2`, `2^5`, `2**5` | Addition, subtraction, multiplication, division, powers |
| Implied multiplication | `2(3 + 4)`, `x=2; y=3; xy` | `14`, `6` |
| Factorial | `5!`, `0!` | `120`, `1`; implemented using the gamma function |
| Remainder | `17 % 5`, `17 mod 5` | `2` |
| Percent | `50%`, `200 + 10%`, `200 - 10%`, `200 * 10%` | `0.5`, `220`, `180`, `20` |
| Parentheses | `(2 + 3) * 4` | Explicit grouping |
| Absolute value | `|-3|`, `abs(-3)` | `3` |
| Ceiling/floor | `⌈4.2⌉`, `⌊2.6⌋` | `5`, `2` |
| Unicode operators | `2 × 3`, `2 ⋅ 3`, `8 ÷ 2` | Multiplication and division |

Powers associate to the right: `2^3^2` means `2^(3^2)`. The parser applies unary minus before powers: **`-2^2` evaluates to `4`**; write `-(2^2)` for `-4`. Its permissive implied multiplication and calls without parentheses can be ambiguous, so parentheses are useful in generated expressions. `%` is a suffix percentage or binary remainder depending on context. A percentage on the right of addition/subtraction is relative to the left operand, as the examples above show.

## Variables, functions, and conditional expressions

```text
x = 2; y = 3; xy + x
f(x, y) = xy + 1; f(3, 4)
f(x) = 2x; g(x) = f(x) + 3; g(4)
f(x) = { -x if x < 0; x otherwise }; f(-5)
f(x) = { f(x - 1) if x >= 1; x otherwise }; f(5)
```

These yield `8`, `13`, `11`, `5`, and `0`. Functions support multiple arguments, other user functions, local parameters, recursion, and multiple piecewise branches. Variables/functions can be redefined. Variable definitions are lazy: `x=2; y=x+1; x=4; y` gives `5`. Self-referential variables are rejected. Names can contain underscores and subscripts, for example `long_name` and `x₂₃`. Built-in constants cannot be overwritten. `sqrt4` and `f3` are accepted calls when the function is known.

A bare `x = 2` is a definition. Use `(x = 2)` or a conditional expression when you intend equality. An equation with an unknown variable, such as `x^2 = 64`, invokes the numerical solver.

## Constants and complex arithmetic

| Constant | Aliases | Meaning |
| --- | --- | --- |
| `pi` | `π` | Circle constant |
| `tau` | `τ` | Twice pi |
| `e` | — | Euler's number |
| `phi` | `ϕ` | Golden ratio |
| `i` | — | Square root of minus one |
| `ans` | — | Previous completed evaluation in the current request's context |

Complex values use `a + bi`. Arithmetic, exponentials, logarithms, square roots, trigonometric functions, and inverse functions support complex values where the engine implements them. For example:

```text
(2 + 3i) * (4 - i)
sqrt(-4)
(1 + i) / (1 - i)
abs(3 + 4i)
Re(3 + 4i)
Im(3 + 4i)
ln(-1)
```

The first six values are `11 + 10i`, `2i`, `i`, `5`, `3`, and `4`; the last is approximately `pi*i`. `arg(z)` gives the complex argument, and `sgn(z)` gives `z/abs(z)` for a nonzero complex value. Complex functions use principal branches. Support is function-specific: `gamma`, factorial, and `cbrt` use the real component for compatibility, so they are not general complex gamma/cube-root implementations.

## Complete built-in function catalog

Names are case-sensitive. `Re`, `Im`, `nCr`, and `nPr` use the capitalization shown. This catalog includes the supported aliases.

| Family | Functions | Notes/examples |
| --- | --- | --- |
| Trigonometric | `sin`, `cos`, `tan`, `csc`, `sec`, `cot` | One argument; `sin(pi/6)` ≈ `0.5` |
| Inverse trigonometric | `asin`, `acos`, `atan`, `acsc`, `asec`, `acot` | One argument; `asin(0)` = `0` |
| Hyperbolic | `sinh`, `cosh`, `tanh`, `csch`, `sech`, `coth` | One argument |
| Inverse hyperbolic | `asinh`, `acosh`, `atanh`, `acsch`, `asech`, `acoth` | One argument |
| Roots | `sqrt(x)`, `√(x)`, `cbrt(x)`, `∛(x)`, `root(x,n)` | `root(81,4)` = `3`; `cbrt(-27)` = `-3` |
| Exponentials/logs | `exp(x)`, `ln(x)`, `log(x)`, `log(x,b)` | Natural exponential/log; unary `log` is base 10; `log(8,2)` = `3`; `log₂(32)` = `5` |
| Rounding/sign | `abs`, `ceil`, `floor`, `round`, `trunc`, `frac`, `sgn` | `trunc(-2.6)` = `-2`; `frac(-2.25)` = `-0.25`; rounding functions operate on complex components |
| Complex parts | `Re`, `Im`, `arg` | Real part, imaginary part, argument |
| Gamma | `gamma`, `Γ` | `gamma(5)` = `24` |
| Magnitude | `hypot(x,y)` | `hypot(3,4)` = `5` |
| Common factors | `gcd(x,y)`, `lcm(x,y)` | `gcd(18,24)` = `6`; includes Gaussian-integer handling in `gcd` |
| Combinations | `nCr(n,r)`, `comb(n,r)` | `nCr(5,2)` = `10` |
| Permutation count | `nPr(n,r)`, `perm(n,r)` | `nPr(5,2)` = `20` |
| Integer bits | `bitcmp(x)`, `bitand(x,y)`, `bitor(x,y)`, `bitxor(x,y)`, `bitshift(x,n)` | Positive shift is left, negative is right; also `<<` and `>>` operators |
| Boolean indicator | `iverson(x)` | `iverson(2>1)` = `1`; `iverson(2<1)` = `0` |
| Collection reductions | `average`, `min`, `max`, `sum`, `prod` | Accept a vector or arguments: `sum((1,2,3))`, `average(1,2,3)` |
| Collection operations | `length(x)`, `sort(x)`, `append(v,x)` | `length` counts vector elements or matrix rows; `append` adds one item |
| Permutation enumeration | `perms`, `permutations` | Return distinct permutations as matrix rows; `length(perms((1,2,3)))` = `6` |
| Matrix creation | `matrix`, `diag` | `matrix(((1,2),(3,4)))`; `diag((2,3))` |
| Matrix operations | `transpose(x)`, `trace(x)`, `det(x)`, `determinant(x)` | `det` and `determinant` are aliases; transpose also uses `ᵀ` or `^T` |
| Numerical calculus | `integrate`, `integral`, `∫`; primes on function names | See below |
| Bounded reductions | `sum`, `Σ`, `∑`, `prod`, `∏` | Use explicit iteration variable, for example `sum(k=1,3,k^2)` |

The lexer also accepts inverse-function notation such as `sin⁻¹(0)`, `cos⁻¹(1)`, and `tan⁻¹(0)`. Use the catalog's ASCII names when generating calls.

## Angles and custom units

The request's angle setting selects radians (`rad`, default) or degrees (`deg`). Explicit angle suffixes are supported: `sin(90 deg)` = `1`, and `cos(180 deg)` ≈ `-1`. `°` is an alias for `deg`. Group a compound angle before its suffix: use `sin((pi/2) rad)`, because `sin(pi/2 rad)` attaches `rad` to the denominator. Inverse trigonometric results in degree mode carry a `deg` unit. Only radians and degrees are built in; the engine does not ship an SI/imperial conversion database.

Custom units define a conversion formula and its inverse:

```text
unit cm = 100m; 250 cm to m
unit F = C*9/5 + 32; 100C to F
unit F = C*9/5 + 32; 32F to C
```

These give `2.5 m`, `212 F`, and `0 C`. Arithmetic can convert compatible operands: `unit cm=100m; 250cm + 1m` gives `350 cm`. The formula describes the numeric value in the newly defined unit in terms of the base unit; `unit cm = 100m` means 100 centimeters per meter. Definitions can use nonlinear expressions when the engine can invert them. This is a conversion-formula system, not a complete dimensional-analysis system: do not infer physical-unit cancellation, compound-unit algebra, or an arbitrary conversion graph.

Conversion targets can be multi-letter unit names. Angle suffixes used directly in trig functions override the request angle mode. Degree mode also applies angle conversion to the hyperbolic families for compatibility; use radians for conventional hyperbolic calculations.

## Booleans and comparisons

Use `true`, `false`, `=`, `!=`/`≠`, `<`, `>`, `<=`/`≤`, and `>=`/`≥`. Logical operators are `and`/`∧`, `or`/`∨`, and `not`/`¬`. Comparisons can be chained: `1 < 2 < 3` is true. Boolean values can be returned or used in piecewise functions and comprehensions. `iverson` converts false to zero and true to one; for compatibility non-boolean inputs produce one, including `iverson(0)`.

Equality uses the engine's numerical comparison tolerance, rather than an exact arbitrary-precision proof.

## Vectors and matrices

Vectors use `(x,y,z)` or `[x,y,z]`. Matrices use bracketed rows separated by semicolons or newlines: `[1,2;3,4]`. A parenthesized single expression is grouping; a bracketed single expression is a vector.

| Operation | Example | Result |
| --- | --- | --- |
| Vector addition | `(2,3,5) + (7,11,13)` | `(9,14,18)` |
| Vector subtraction | `(2,3,5) - (7,11,13)` | `(-5,-8,-8)` |
| Dot product | `(2,3,5) * (7,11,13)` | `112` |
| Vector component powers | `[1,2,3]^2` | `(1,4,9)` |
| Component division | `(8,9,25) / (2,3,5)` | `(4,3,5)` |
| Scalar broadcast | `(2,3,5) + 2`, `(2,3,5) * 2` | `(4,5,7)`, `(4,6,10)` |
| Unary function mapping | `sqrt((4,9,16))` | `(2,3,4)` |
| Matrix product | `[1,2;3,4] * [5,6;7,8]` | `[19,22;43,50]` |
| Matrix/vector product | `[1,2;3,4] * (2,3)` | `(8,18)` |
| Positive integer matrix powers | `[1,2;3,4]^2` | `[7,10;15,22]` |
| Scalar raised to matrix entries | `2^[1,2;3,4]` | `[2,4;8,16]` |
| Matrix component division | `[4,9;12,3] / [2,3;4,3]` | `[2,3;3,1]` |
| Transpose | `[1,2;3,4]ᵀ` | `[1,3;2,4]` |
| Diagonal matrix | `diag((2,3))` | `[2,0;0,3]` |
| Trace/determinant | `trace([1,2;3,4])`, `det([1,2;3,4])` | `5`, `-2` |
| Vector indexing | `(10,20,30)[[2]]` | `20` |
| Matrix indexing | `[1,2;3,4][[2,1]]` | `3` |
| Matrix row indexing | `[1,2;3,4][[2]]` | `(3,4)` |

Indexes are **one-based**, with `⟦…⟧` as an alternative to `[[…]]`. Matrix/vector addition, subtraction, and division broadcast one vector value per matrix row; matrix/scalar operations broadcast the scalar. Collection multiplication has the linear-algebra behavior shown above, and division is componentwise rather than matrix inversion. Many unary numeric functions map over vectors/matrices. Provide matching sizes and rectangular rows; trace/determinant are defined for square matrices. Ragged matrices are rejected. Matrix powers accept nonnegative real integers; the zero power returns the scalar `1` for compatibility, and negative powers are unsupported. No general matrix inverse, eigenvalue, or cross-product function is registered.

## Comprehensions, sums, and products

```text
[x : 0 ≤ x and 5 > x]
[x^2 : x >= 1 and x <= 4]
[(x,y) : x > 0 and x <= 2, y > 0 and y <= 2]
sum(k=1, 5, 2k)
prod(k=1, 4, k)
sum(a=1, 3, Σ(b=1, 3, a+b))
```

Results are `(0,1,2,3,4)`, `(1,4,9,16)`, `((1,1),(1,2),(2,1),(2,2))`, `30`, `24`, and `36`. Comprehensions enumerate bounded integer ranges inferred from the conditions. Multiple ranges generate combinations. Bounded sums/products include both endpoints, support nesting, and restore existing iteration-variable values.

Use the explicit `k=1` form. For example, `sum(n=1,3,2n+1)` iterates over `n`, whereas `sum(1,3,2n+1)` is an ordinary reduction of three arguments. Ordinary vector reductions `sum((1,2,3))` and `prod((2,3,4))` remain available.

## Differentiation, integration, and equations

| Capability | Expression | Approximate result |
| --- | --- | --- |
| First derivative | `f(x)=2x^2+x; f'(2)` | `9` |
| Built-in derivative | `sin'(0)` | `1` |
| Higher derivative | `f(x)=x^3; f''(2)` | Near `12` |
| Definite integral | `integrate(0,1,x dx)` | `0.5` |
| Integral aliases | `integral(0,1,x^2 dx)` | `1/3` |
| Unicode integral | `∫(0,pi,sin(x) dx)` | `2` |
| Separate differential | `integrate(0,pi,sinx,dx)` | `2` |
| Root finding | `x^2 = 64` | One root, normally `8` |
| Nonlinear root | `3x^3 - 2x = x^2 + 2` | `1.2707763267` |
| Equation system | `{x+y+z=9;z*x-y=10;4y+12x-2=42}` | `(3,2,4)` for `(x,y,z)` |

These operations evaluate numerically. Derivatives use finite differences; higher derivatives and noisy/discontinuous functions can be inaccurate. Integrals use numerical quadrature over finite limits. Root finding uses Newton iteration, can fail to converge, and returns one solution rather than all roots. Equation-system results follow sorted variable-name order. Neither symbolic differentiation/integration nor a complete algebraic equation solver is provided. Increasing arithmetic precision does not remove these algorithmic limitations.

## Number bases

Binary, octal, and hexadecimal prefixes support fractional parts:

| Expression | Decimal value |
| --- | --- |
| `0b1101` | `13` |
| `0o17` | `15` |
| `0xff` | `255` |
| `0b1101.101` | `13.625` |
| `0xb.5` | `11.3125` |
| `1101_2`, `1101₂` | `13` |
| `13.5_8` | `11.625` |

The underscore/subscript form only uses decimal digit characters; letters are interpreted as variables. The API's numeric strings use decimal representation. Radix literals are accumulated directly at the requested arithmetic precision; output is always decimal.

## Precision and practical limits

The API accepts a binary precision setting; it does not promise that every returned digit is mathematically accurate. Literals, arithmetic, and built-in `pi`/`e`/`tau`/`phi` use the requested precision. Numerical algorithms still use finite tolerances and some fixed-precision intermediates. The `bit*` functions coerce to signed 32-bit integers; they are not arbitrary-size integer bit operations. The separate `<<`/`>>` operators scale a real number by a power of two, so `5 >> 1` gives `2.5`, whereas `bitshift(5,-1)` gives `2`. Use small integer counts for `bitshift`, and explicit whole-number indexes.

Undefined names, malformed expressions, incompatible collection dimensions, unsupported argument types, missing differentials, and equations without a found root produce errors. Some numerical domains produce NaN or infinity instead of a mathematical value. Function arguments, collection shapes, and indexes are validated; not every function accepts every value type. Request-size, runtime, recursion, output-size, and process limits apply to prevent an expensive expression from taking over the API. The HTTP documentation describes the configured limits and error representations.

This service exposes the calculation engine through HTTP, with request-local definitions and context in place of interactive sessions. It does not execute shell commands or load server files from calculator expressions.

## Executable examples

[`capability-cases.json`](capability-cases.json) is the machine-readable capability corpus used by the integration suite. Each of the 227 entries contains a category, an expression, and an `expected` reference expression. Some also select `context`, `precision`, or `angle_unit`. Reference expressions with custom units declare those units independently before returning the expected value. Independent fixed results from the preceding release also live in [`tests/fixtures/compatibility.json`](../tests/fixtures/compatibility.json). Numerical methods require tolerances, and structured real/imaginary/vector/matrix results are preferable to matching the engine's terminal formatting.
