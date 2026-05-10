# NiaLang

Minimal compiled language that lowers to LLVM IR and runs via `clang`.

The project currently focuses on a compact but expressive core:
- fixed-size arrays (`[T; N]`) with indexing and mutation,
- fixed-size **vector** types (declared axes, one numeric type per axis) with component-wise and dot-product ops,
- structs (named and tuple), enums and `match`,
- loops (`for`, `while`, `loop` + `break`),
- pointers and simple heap builtins (`alloc`, `realloc`, `dealloc`),
- builtin `println` and `len`.

## Quick Start

Build and run a program:

```bash
cargo run -r -- examples/sample.nia
```

Save generated LLVM IR:

```bash
cargo run -r -- examples/sample.nia -o examples/sample.ll
```

Run tests:

```bash
cargo test
```

## Language Snapshot

### Types

- Integers: `i8`, `u8`, `i16`, `u16`, `i32`, `i64`, `u64`, `i128`, `isize`, `usize`, `u128`
- `bool`
- Arrays: `[T; N]`
- Pointers: `&T`
- Structs / Enums
- **Vectors** (user-defined fixed-size aggregates): see [Fixed-size vectors](#fixed-size-vectors) below.

### Expressions and Operators

- Arithmetic: `+`, `-`, `*`, `/`
- Vector-only: `@` (dot product of two values of the *same* `vector` type); component-wise `+`, `-`, `*` on two such vectors; `*` between a vector and a scalar of the **axis** type (either order). See [Fixed-size vectors](#fixed-size-vectors).
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Indexing: `arr[i]`
- Field access: `obj.x`, `tuple.0`
- Function calls: `foo(a, b)`

### Builtins

- `println(x)` — prints values (including arrays, structs, enums, pointers, **vectors**)
- `len(arr)` — returns compile-time array length as `i32`
- `alloc(v)` / `realloc(ptr, v)` / `dealloc(ptr)`

### Fixed-size vectors

Vectors are a small **linear-algebra-friendly** layer on top of the same value-type model as structs: a `vector` declaration introduces a named aggregate with **named axes** and a single **element** (axis) type. They are meant for 2D/3D-style math (positions, deltas, RGB-like triples) where you want field names (`X`, `Y`, …) and a fixed set of operations, not arbitrary struct logic.

#### What a vector is (and is not)

- **Is:** A value type known to the compiler by its **declared name** (e.g. `Vec2`). Two vectors are compatible only if they are the **same** declaration — nominal typing, not structural. Even if two `vector` types list the same axes and the same element type, if the names differ (`A` vs `B`), you cannot mix them with `+`, `*`, or `@`.
- **Is:** Lowered to LLVM as a plain struct with one field per axis (same layout idea as a named struct with those fields).
- **Is not:** A generic `Vec<T>` or a dynamically sized array; size is fixed by the axis list.
- **Is not:** SIMD or a matrix type; there is no `@` between matrices, only between two values of the **same** vector type.

#### Name space and declarations

- The name of a `vector` must be unique among **structs**, **enums**, and **vectors** in the program.
- Syntax: keyword `vector`, the type name, the **element type** (any primitive integer or float), then a comma-separated list of axis identifiers inside **`[ ... ]`** (preferred) or legacy **`{ ... }`**.

```nia
vector Vec2 i32 [
    X,
    Y,
]

vector Dir3 f64 { X, Y, Z }   // legacy brace form; same meaning as brackets
```

- Every axis name in the list must be a distinct identifier; the compiler expects a **complete** literal (all axes present, exactly once).

#### Literals and field access

Literals reuse the same **labeled record** shape as struct literals, using the vector’s type name:

```nia
let p = Vec2 [X: 10, Y: -3];
```

Field order in source may differ from the declaration; fields are matched **by name**. Each component expression must be assignable to the declared element type (`i32` here). You can annotate locals with the vector name like any other nominal type:

```nia
let q: Vec2 = Vec2 [X: 0, Y: 0];
```

Reading axes is ordinary field syntax: `p.X`, `p.Y`. That yields the element type (`i32` / `f64` / …).

#### Typed operations (summary)

All vector-vector operators require **the same vector type** on both sides. The element type must be **numeric** (integer or float); vectors whose element type is `bool` or another composite are not valid for arithmetic.

| Operator | Operands | Result type | Meaning |
|----------|-----------|---------------|---------|
| `+` | two same `vector` | that `vector` | Component-wise sum |
| `-` | two same `vector` | that `vector` | Component-wise difference |
| `*` | two same `vector` | that `vector` | Component-wise (Hadamard) product |
| `*` | `vector` and scalar, or scalar and `vector` | that `vector` | **Scale** every axis; scalar must have **exactly** the element type (e.g. `i32` for `vector V i32 [...]` — not `i64`, not `u32`, unless that is the declared element type) |
| `@` | two same `vector` | **element type** | Dot product: `u.X*v.X + u.Y*v.Y + …` |

Integer element types use LLVM `nsw` where applicable for add/sub/mul; floats use `fadd` / `fmul` / `fadd` for the dot-product reduction.

#### Dot product `@` (details)

- **Left** operand must already be a vector; `3 @ v` is invalid (there is no “scalar on the left” form).
- **Right** operand is inferred expecting the same vector type as the left, so literals and calls can be contextualized like other binary vector ops.
- **Result** is a **scalar** of the element type, so it composes with normal arithmetic: e.g. `(u @ v) * 2` multiplies the scalar dot value by `2` (with usual scalar typing rules).

#### Precedence and grouping

`*`, `/`, and `@` share one **multiplicative** precedence level and bind **tighter** than `+` and `-`. They group **left-to-right** among themselves.

Examples (with `u`, `v`, `w` of the same `vector` type, `s` of the element type):

- `u + v * w` means `u + (v * w)` — Hadamard product first, then component-wise sum.
- `u * v @ w` means `(u * v) @ w` — dot of two vectors, **not** `u * (v @ w)` (the latter would be a vector times a scalar, which is scaling).
- `u + v @ w` means `u + (v @ w)` only if that were legal; in practice `v @ w` is a scalar, so **`u + (scalar)` is a type error** — you must parenthesize vector sums before dotting, e.g. `(u + v) @ w`.

#### What is rejected (common mistakes)

- **Different vector names**, even with identical axes and element type: no `A + B`, `A * B`, or `A @ B`.
- **Wrong scalar width** for scaling: `vector V2 i64 [ X, Y ]` cannot scale with an `i32` literal without an explicit annotation matching `i64`.
- **Division** `vector / vector` or `vector / scalar` — `/` is not defined on vector values.
- **Unary `-`** on a whole vector value.
- **Comparisons** `==`, `<`, … on vector values (only integers, floats, bool, and pointers are comparable today).
- **Void and pointers** where vector ops expect values — same restrictions as scalar arithmetic.

#### `println` and passing vectors around

`println` accepts vector values; output is built from the element type and axis layout. You can pass vectors as **function arguments** and **return** them by value like any other value type, as long as signatures match nominally.

#### Compound assignment on vector variables

Statements like `u += v`, `u -= v`, and `u *= rhs` are lowered exactly like `u = u + v`, `u = u - v`, and `u = u * rhs`. So `*=` can mean **Hadamard** update (`u *= v` with `v` the same vector type) or **scaling** (`u *= s` with `s` the element type). There is **no** `@=` token; accumulate a dot product with an explicit scalar variable, e.g. `let acc = acc + (u @ v);`.

#### Further examples

- Extended demo (two types, `Vec2` and `Vec3`, more `println` cases): `examples/sample_vector_arith.nia`.
- Minimal walk-through (single `Vec2` type): see **§4** under [Large Working Examples](#large-working-examples) below.

## Large Working Examples

The first three examples below are mirrored under `examples/tests/ok_readme_*.nia` and validated by `cargo test`. The vector example matches the same rules as `examples/sample_vector_arith.nia` (second vector type and more `println` calls live there).

### 1) Arrays: reverse + bubble sort

```nia
fn reverse_array(arr: [u8; 8]) [u8; 8] {
    for i in 0..len(arr) / 2 {
        let temp = arr[i];
        arr[i] = arr[len(arr) - 1 - i];
        arr[len(arr) - 1 - i] = temp;
    }
    arr
}

fn bubble_sort(arr: [u8; 8]) [u8; 8] {
    for i in 0..len(arr) {
        for j in 0..len(arr) - 1 - i {
            if arr[j] > arr[j + 1] {
                let temp = arr[j];
                arr[j] = arr[j + 1];
                arr[j + 1] = temp;
            }
        }
    }
    arr
}

fn main() i32 {
    let src: [u8; 8] = [8, 3, 6, 1, 4, 7, 2, 5];
    let rev = reverse_array(src);
    let sorted = bubble_sort(src);
    println(src);
    println(rev);
    println(sorted);
    0
}
```

Fixture: `examples/tests/ok_readme_arrays.nia`

### 2) Enums with payload and `match`

```nia
enum Msg {
    Ping,
    Add(i32, i32),
    Move { x: i32, y: i32 },
}

fn eval(m: Msg) i32 {
    match m {
        Msg::Ping => 0,
        Msg::Add(a, b) => a + b,
        Msg::Move { x, y } => x + y,
    }
}

fn main() i32 {
    println(eval(Msg::Ping));
    println(eval(Msg::Add(7, 5)));
    println(eval(Msg::Move { x: 10, y: 20 }));
    0
}
```

Fixture: `examples/tests/ok_readme_enums.nia`

### 3) Pointers and heap operations

```nia
struct Pair(i32, i32)

fn write_mid(arr_ptr: &[u8; 4]) {
    (*arr_ptr)[1] = 9;
}

fn main() i32 {
    let p: &i32 = alloc(42);
    println(*p);

    let p2: &i32 = realloc(p, 100);
    println(*p2);
    dealloc(p2);

    let pair_ptr: &Pair = alloc(Pair(7, 9));
    println((*pair_ptr).0);
    dealloc(pair_ptr);

    let arr: [u8; 4] = [1, 2, 3, 4];
    write_mid(&arr);
    println(arr);
    0
}
```

Fixture: `examples/tests/ok_readme_pointers.nia`

### 4) Fixed-size vectors: components, scaling, dot product

```nia
// `vector Name ElementTy [ axes... ]` — fixed 2D integer vector (X,Y are i32 fields).
vector Vec2 i32 [ X, Y ]

fn main() i32 {
    // Literals: same shape as struct literals, one value per axis.
    let u = Vec2 [X: 3, Y: 4];
    let v = Vec2 [X: 1, Y: 2];
    let w = Vec2 [X: 1, Y: 1];

    // `+` / `-` : component-wise (per-axis) sum / difference; result type is Vec2.
    let sum = u + v;       // [3+1, 4+2] = [4, 6]
    let diff = u - v;      // [3-1, 4-2] = [2, 2]

    // `*` with another Vec2: Hadamard (component-wise) product, still Vec2.
    let had = u * v;       // [3*1, 4*2] = [3, 8]

    // `*` with a scalar of the *axis* type (here i32): scale every component.
    let scaled = u * 2;    // [3*2, 4*2] = [6, 8]   (same as `2 * u`)

    // `@` : dot product — sum of (u.X*v.X + u.Y*v.Y); result is scalar i32, not Vec2.
    let dot = u @ v;       // 3*1 + 4*2 = 11

    // `@` binds like `*`; parentheses group the vector sum before dotting with w.
    let mixed = (u + v) @ w;   // (u+v) is Vec2, then dot with w → scalar i32

    println(sum);    // whole-vector print
    println(diff);
    println(had);
    println(scaled);
    println(dot);    // scalar
    println(mixed);  // scalar
    0
}
```

See also: `examples/sample_vector_arith.nia` (includes a second vector type `Vec3`).

## Project Layout

```text
src/
  ast/
  lexer/
  parser/
  semantics/typecheck/
  backend/codegen/
  driver/
```

Tests for each component live in separate files alongside modules:
- `src/lexer/tests.rs`
- `src/parser/tests.rs`
- `src/semantics/typecheck/tests.rs`
- `src/backend/codegen/tests.rs`
- `src/driver/tests.rs`

