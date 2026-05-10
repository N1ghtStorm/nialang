# Vector typing: compiler restrictions

This document lists forms the **typechecker rejects** for `vector` types, and related constraints. For the language overview and supported operations, see [README.md](../README.md) (section *Fixed-size vectors*).

## Declarations and names

- **Duplicate `vector` name** — two `vector` declarations with the same type name.
- **Name collision** — a `vector` cannot share its name with a `struct` or `enum` in the same program.
- **Non-numeric element type** — the axis type must be a primitive integer or float. Using `bool` or a composite type as the element type prevents vector arithmetic (`+`, `-`, `*`, `@`) and scaling.

## Literals

- **Missing axis** — a literal must supply every axis from the declaration exactly once.
- **Unknown or duplicate field label** — axis names must match the declaration; duplicates or typos are rejected.
- **Field expression type** — each component must match the declared element type (after usual literal / coercion rules).

## Binary operators: same nominal type

For `+`, `-`, `*`, and `@` on two vector operands, both sides must be the **same** declared vector type (same type name). The compiler does **not** treat two different `vector` declarations as compatible even if they have identical axes and element types (e.g. `A` and `B` with the same shape — `A + B` is rejected).

## Scaling (`*` with a scalar)

- **Scalar type must match the axis type exactly** — e.g. for `vector V i64 [ X, Y ]`, scaling with an unannotated `i32` literal is rejected unless the context fixes the literal to `i64`.
- **Two vectors with `*`** — component-wise product requires the same vector type; mixing different vector names is rejected.

## Dot product (`@`)

- **Left operand must be a vector** — there is no `scalar @ vector` form (e.g. `3 @ v` is rejected).
- **Right operand** — must be the same vector type as the left (after inference with that expectation).
- **Result** — scalar of the element type; mixing that result into `vector + scalar` without an intermediate `let` is rejected (see precedence in README: use `(u + v) @ w`).

## Operators not defined on vectors

- **`/`** — division is not defined for vector operands (neither `vector / vector` nor `vector / scalar` in the vector sense).
- **Unary `-`** — negation applies only to scalar numeric types, not to a whole vector value.
- **Comparisons** (`==`, `!=`, `<`, …) — vector values are not in the set of comparable types for these operators (the checker allows integers, floats, `bool`, and pointers for the supported comparison forms).

## Other expression rules

- **`void` / `()`** — vector operators cannot take `()`-typed subexpressions where a value is required.
- **Pointers** — `*` / `@` / component-wise ops do not accept raw pointer operands in place of scalars or vectors.

## Compound assignment

- **`+=` / `-=` / `*=` / `/=`** — `*=` on a vector follows the same rules as `v = v * rhs` (Hadamard or scaling). **`/=`** on a vector is rejected when the expanded division is invalid for vectors.
- **No `@=`** — there is no compound assignment token for dot product; use a scalar temporary, e.g. `let acc = acc + (u @ v);`.

## Implementation note

Exact diagnostic strings may change; this file describes **semantic** rules enforced by `src/semantics/typecheck/mod.rs` (and mirrored in tests under `src/semantics/typecheck/tests.rs`).
