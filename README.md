# NiaLang

Minimal compiled language that lowers to LLVM IR and runs via `clang`.

The project currently focuses on a compact but expressive core:
- fixed-size arrays (`[T; N]`) with indexing and mutation,
- fixed-size **vector** types (declared axes, one numeric type per axis) with component-wise and dot-product ops,
- structs (named and tuple), enums and `match`,
- loops (`for`, `while`, `loop` + `break`),
- pointers and simple heap builtins (`alloc`, `realloc`, `dealloc`),
- built-in reference-counted matrices,
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
- `Matrix` — built-in ref-counted heap matrix of one numeric cell type.

### Expressions and Operators

- Arithmetic: `+`, `-`, `*`, `/`
- Vector-only: `@` (dot product of two values of the *same* `vector` type); component-wise `+`, `-`, `*` on two such vectors; `*` between a vector and a scalar of the **axis** type (either order). See [Fixed-size vectors](#fixed-size-vectors).
- Matrix-only: `@` (matrix multiplication); component-wise `+`, `-`, `*`; `*` with a scalar of the exact cell type.
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Indexing: `arr[i]`
- Field access: `obj.x`, `tuple.0`
- Function calls: `foo(a, b)`

### Builtins

- `println(x)` — prints values (including arrays, structs, enums, pointers, **vectors**, `Matrix`)
- `len(arr)` — returns compile-time array length as `i32`
- `alloc(v)` / `realloc(ptr, v)` / `dealloc(ptr)`
- `matrix([[...], [...]])` — creates a `Matrix` from a rectangular array of numeric arrays
- `matrix_get(m, row, col)` / `matrix_set(m, row, col, value)`
- `matrix_rows(m)` / `matrix_cols(m)` / `matrix_len(m)`
- `matrix_clone(m)` / `matrix_refcount(m)` / `matrix_drop(m)`
- `outer(a, b)` — outer product of two vectors; returns a `Matrix`
- `def(m)` — determinant of a square `Matrix`; returns the matrix cell type
- `a + b` / `a - b` / `a * b` — component-wise matrix arithmetic with the same element type and shape
- `a @ b` — matrix multiplication; `matrix_cols(a)` must equal `matrix_rows(b)`
- `m * scalar` / `scalar * m` — matrix scaling; the scalar type must match the matrix cell type

`Matrix` is a compiler-known heap object with explicit reference counting.
See [Matrices](#matrices) for construction, printing, indexing, and lifetime
rules.

### Matrices

`Matrix` is a built-in reference-counted heap handle for a rectangular 2D block
of numeric cells. Source code writes the surface type simply as `Matrix`; inside
the compiler the handle still remembers the element type, such as `Matrix<i32>`
or `Matrix<f64>`.

#### Creating a matrix

Use `matrix([...])` with an array of rows:

```nia
let m: Matrix = matrix([
    [1, 2, 3],
    [4, 5, 6],
]);
```

Rules:

- The outer array must contain at least one row.
- Every row must contain at least one cell.
- Every row must have the same length.
- Every cell must be a numeric primitive: integer types (`i8`, `u8`, `i16`, ...)
  or float types (`f16`, `f32`, `f64`).
- All cells must have exactly one element type. Integer literals default to
  `i32`; float literals default to `f64`.

Valid:

```nia
let ints: Matrix = matrix([
    [1, 2],
    [3, 4],
]);

let floats: Matrix = matrix([
    [1.0, 2.0],
    [3.5, 4.5],
]);
```

Rejected:

```nia
let mixed: Matrix = matrix([
    [1, 2],
    [3.5, 4.5],
]);
```

The rejected example mixes `i32` and `f64`. If the matrix should be floating
point, write the integer-looking cells with a decimal point (`1.0`, `2.0`, ...).

#### Printing

`println(m)` prints the matrix contents as nested arrays:

```nia
let m: Matrix = matrix([
    [1, 2, 3],
    [4, 5, 6],
]);

println(m); // [[1, 2, 3], [4, 5, 6]]
```

Float cells use the same float formatting as normal `println`:

```nia
let f: Matrix = matrix([
    [1.0, 2.5],
    [3.0, 4.75],
]);

println(f); // [[1.000000, 2.500000], [3.000000, 4.750000]]
```

#### Shape and length

Use the shape helpers when you need dimensions at runtime:

```nia
println(matrix_rows(m)); // number of rows
println(matrix_cols(m)); // number of columns
println(matrix_len(m));  // rows * columns
```

Each helper returns `i32`.

#### Reading and writing cells

Cells are addressed by zero-based row and column indices:

```nia
let value = matrix_get(m, 0, 1); // row 0, column 1
matrix_set(m, 1, 2, 42);        // row 1, column 2
```

`matrix_get` returns the matrix element type. `matrix_set` requires a value of
that same type:

```nia
let ints: Matrix = matrix([
    [1, 2],
    [3, 4],
]);

matrix_set(ints, 0, 0, 99);   // ok: i32 matrix, i32 value
// matrix_set(ints, 0, 0, 1.5); // rejected: f64 value for i32 matrix
```

Current runtime note: indices are assumed valid. There is no bounds check yet,
so `matrix_get(m, 100, 100)` is invalid program behavior.

#### Matrix arithmetic

Use `+`, `-`, and `*` for component-wise matrix arithmetic:

```nia
let a: Matrix = matrix([
    [1, 2],
    [3, 4],
]);

let b: Matrix = matrix([
    [10, 20],
    [30, 40],
]);

let c: Matrix = a + b;
let d: Matrix = b - a;
let e: Matrix = a * b;
println(c); // [[11, 22], [33, 44]]
println(d); // [[9, 18], [27, 36]]
println(e); // [[10, 40], [90, 160]]
```

The result is a new `Matrix` allocation with reference count `1`; it does not
modify either operand. Both operands must have the same element type:

```nia
let ints: Matrix = matrix([
    [1, 2],
    [3, 4],
]);

let floats: Matrix = matrix([
    [1.0, 2.0],
    [3.0, 4.0],
]);

// let bad_sum: Matrix = ints + floats;  // rejected: Matrix<i32> + Matrix<f64>
// let bad_diff: Matrix = ints - floats; // rejected: Matrix<i32> - Matrix<f64>
// let bad_prod: Matrix = ints * floats; // rejected: Matrix<i32> * Matrix<f64>
```

Both operands must also have the same runtime shape (`rows` and `cols`). The
generated code checks the shape before doing arithmetic; a mismatch aborts the
program. Drop each result when it is no longer needed:

```nia
matrix_drop(e);
matrix_drop(d);
matrix_drop(c);
matrix_drop(b);
matrix_drop(a);
```

Use `@` for real matrix multiplication. The cell type must match exactly, and
the runtime shape must satisfy the usual rule:

```text
(n x m) @ (m x k) = (n x k)
```

For example, a `2 x 3` matrix multiplied by a `3 x 4` matrix produces a
`2 x 4` matrix:

```nia
let left: Matrix = matrix([
    [1, 2, 3],
    [4, 5, 6],
]);

let right: Matrix = matrix([
    [7, 8, 9, 10],
    [11, 12, 13, 14],
    [15, 16, 17, 18],
]);

let product: Matrix = left @ right;
println(product); // [[74, 80, 86, 92], [173, 188, 203, 218]]
println(matrix_rows(product)); // 2
println(matrix_cols(product)); // 4
```

Generated code checks `matrix_cols(left) == matrix_rows(right)` before
multiplying; a mismatch aborts the program. Like other matrix arithmetic, `@`
creates a new allocation with reference count `1`.

Use `outer(a, b)` to build a matrix from two vectors. The vector declarations may
have different lengths, but their element types must be the same numeric type:

```nia
vector Vec3i i32 [X, Y, Z]
vector Vec2i i32 [U, V]

let a = Vec3i [X: 1, Y: 2, Z: 3];
let b = Vec2i [U: 4, V: 5];

let product: Matrix = outer(a, b);
println(product); // [[4, 5], [8, 10], [12, 15]]
println(matrix_rows(product)); // 3
println(matrix_cols(product)); // 2
```

The rows come from the first vector and the columns come from the second vector.
Like matrix arithmetic, the result is a new `Matrix` allocation with reference
count `1`.

Use `def(m)` to compute the determinant of a square matrix. The return type is
the same as the matrix cell type:

```nia
let m: Matrix = matrix([
    [1, 2],
    [3, 4],
]);

let d: i32 = def(m);
println(d); // -2
```

Generated code checks `matrix_rows(m) == matrix_cols(m)` before computing the
determinant; a non-square matrix aborts the program.

Use `*` with a scalar to multiply every cell by one number:

```nia
let m: Matrix = matrix([
    [1, 2],
    [3, 4],
]);

let right: Matrix = m * 3;
let left: Matrix = 2 * m;
println(right); // [[3, 6], [9, 12]]
println(left);  // [[2, 4], [6, 8]]
```

The scalar type must match the matrix cell type exactly. Integer literals are
`i32`; float literals are `f64`:

```nia
let f: Matrix = matrix([
    [1.0, 2.0],
    [3.0, 4.0],
]);

let ok: Matrix = f * 2.0;
// let bad: Matrix = f * 2; // rejected: Matrix<f64> * i32
```

#### Reference counting

A `Matrix` handle owns a heap allocation with an explicit reference counter.
The compiler does not insert automatic clone/drop calls yet, so code must manage
sharing explicitly:

```nia
let m: Matrix = matrix([
    [1, 2, 3],
    [4, 5, 6],
]);

println(matrix_refcount(m)); // 1

let shared: Matrix = matrix_clone(m);
println(matrix_refcount(m)); // 2

println(shared);             // prints the same heap data

matrix_drop(shared);
println(matrix_refcount(m)); // 1

matrix_drop(m);              // frees when the counter reaches zero
```

Use `matrix_clone(m)` whenever another handle should share the same matrix data.
Use `matrix_drop(m)` exactly once for each live handle when it is no longer
needed. Do not use a handle after dropping it.

#### Complete example

See `examples/sample_matrix_rc.nia` for a runnable sample covering construction,
printing, shape queries, cell get/set, cloning, reference count inspection, and
dropping. See `examples/sample_matrix_arith.nia` for a separate arithmetic
sample focused on `+`, `-`, and `*`.

### Fixed-size vectors

Vectors are a small **linear-algebra-friendly** layer on top of the same value-type model as structs: a `vector` declaration introduces a named aggregate with **named axes** and a single **element** (axis) type. They are meant for 2D/3D-style math (positions, deltas, RGB-like triples) where you want field names (`X`, `Y`, …) and a fixed set of operations, not arbitrary struct logic.

#### What a vector is (and is not)

- **Is:** A value type known to the compiler by its **declared name** (e.g. `Vec2`). Binary vector operators (`+`, `-`, `*`, `@`) take two operands of that **same** declared type — nominal typing, not structural equivalence. Distinct declarations (e.g. `A` and `B`) are different types even if their axes and element type match.
- **Is:** Lowered to LLVM as a plain struct with one field per axis (same layout idea as a named struct with those fields).
- **Is not:** A generic `Vec<T>` or a dynamically sized array; size is fixed by the axis list.
- **Is not:** SIMD or a matrix type; **`@`** is the dot product of two values of the **same** vector type.

#### Name space and declarations

- The name of a `vector` is unique among **structs**, **enums**, and **vectors** in the program.
- Syntax: keyword `vector`, the type name, the **element type** (any primitive integer or float), then a comma-separated list of axis identifiers inside **`[ ... ]`** (preferred) or legacy **`{ ... }`**.

```nia
vector Vec2 i32 [
    X,
    Y,
]

vector Dir3 f64 { X, Y, Z }   // legacy brace form; same meaning as brackets
```

- A literal lists every axis from the declaration, each exactly once.

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

All vector-vector operators use **the same vector type** on both sides. The element type is a **numeric** primitive (integer or float); arithmetic follows that element type.

| Operator | Operands | Result type | Meaning |
|----------|-----------|---------------|---------|
| `+` | two same `vector` | that `vector` | Component-wise sum |
| `-` | two same `vector` | that `vector` | Component-wise difference |
| `*` | two same `vector` | that `vector` | Component-wise (Hadamard) product |
| `*` | `vector` and scalar, or scalar and `vector` | that `vector` | **Scale** every axis; the scalar has the **same** type as each axis (the declared element type, e.g. `i32` on `vector V i32 [...]`) |
| `@` | two same `vector` | **element type** | Dot product: `u.X*v.X + u.Y*v.Y + …` |

Integer element types use LLVM `nsw` where applicable for add/sub/mul; floats use `fadd` / `fmul` / `fadd` for the dot-product reduction.

#### Dot product `@` (details)

- The **left** operand is a vector (the usual form is `u @ v`).
- The **right** operand is inferred with the same vector type as the left, so literals and calls follow the same typing as other binary vector ops.
- The **result** is a **scalar** of the element type, so it composes with normal arithmetic, e.g. `(u @ v) * 2` scales the dot value by `2`.

#### Precedence and grouping

`*`, `/`, and `@` share one **multiplicative** precedence level and bind **tighter** than `+` and `-`. They group **left-to-right** among themselves.

Examples (with `u`, `v`, `w` of the same `vector` type, `s` of the element type):

- `u + v * w` is `u + (v * w)` — Hadamard product first, then component-wise sum.
- `u * v @ w` is `(u * v) @ w` — dot product of the Hadamard result with `w` (left-to-right among `*`, `/`, `@`).
- `u + v @ w` groups as `u + (v @ w)` under these rules; to dot a **sum** with another vector, parenthesize the sum: `(u + v) @ w`.

#### `println` and passing vectors around

`println` accepts vector values; output is built from the element type and axis layout. Vectors can be passed as **function arguments** and **returned** by value when the signature uses the same vector type name.

#### Compound assignment on vector variables

Statements like `u += v`, `u -= v`, and `u *= rhs` expand to `u = u + v`, `u = u - v`, and `u = u * rhs`. So `*=` can apply a **Hadamard** update (`u *= v` with `v` the same vector type) or **scaling** (`u *= s` with `s` the element type). For a running dot sum, use a scalar accumulator, e.g. `let acc = acc + (u @ v);`.

#### Further examples

- Extended demo (two types, `Vec2` and `Vec3`, more `println` cases): `examples/sample_vector_arith.nia`.
- Minimal walk-through (single `Vec2` type): see **§4** under [Large Working Examples](#large-working-examples) below.
- Stricter typing rules and rejected forms: [docs/vector-limitations.md](docs/vector-limitations.md).

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

    // `@` : dot product — u.X*v.X + u.Y*v.Y; result is scalar i32.
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
