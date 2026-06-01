# NiaLang

**NiaLang** is a small compiled programming language for linear algebra experiments.
It treats vectors and matrices as first-class values, keeps the syntax compact, and
lowers programs through LLVM IR and `clang`.

The language is still experimental, but the core idea is already visible:

```nia
let dot = u @ v;        // dot product
let sum = u + v;        // element-wise vector addition
let had = u * v;        // Hadamard product
let scaled = 3 * u;     // scalar-vector multiplication

let ab = A @ B;         // matrix product
let av = A @ v;         // matrix-vector product
let va = v @ A;         // vector-matrix product
let d = A.det();        // determinant
```

The goal is a language that feels natural for dense numeric code while staying
simple enough to hack on.

## Quick Start

Build and run an example:

```bash
cargo run -- examples/sample_linalg_commented.nia
```

Build a shared library from exported `extern fn` symbols:

```bash
cargo run -- examples/sample_extern_lib.nia --lib -o build/libnia_sample.dylib
```

Emit native assembly for inspection:

```bash
cargo run -- examples/sample_floats.nia --emit-asm build/sample_floats.s
```

Run the compiler test suite:

```bash
cargo test
```

Nia programs usually look like this:

```nia
fn main() i32 {
    let x = 40 + 2;
    println(x);

    0
}
```

## Linear Algebra First

### Named Vectors

Named vectors are fixed-size vectors whose coordinates have labels. The labels
make output easier to read and help keep examples close to math notation.

```nia
vector Vec2 i32 [X, Y]

fn main() i32 {
    let u = Vec2 [X: 1, Y: 2];
    let v = Vec2 [X: 3, Y: 4];

    println(u + v);     // (i32 {"X": 4, "Y": 6})
    println(v - u);     // (i32 {"X": 2, "Y": 2})
    println(u * v);     // (i32 {"X": 3, "Y": 8})
    println(u @ v);     // 11
    println(3 * u);     // (i32 {"X": 3, "Y": 6})

    0
}
```

Vector operators:

| Operator | Meaning | Result |
| --- | --- | --- |
| `u + v` | element-wise addition | vector |
| `u - v` | element-wise subtraction | vector |
| `u * v` | element-wise multiplication | vector |
| `u @ v` | dot product | scalar |
| `k * u`, `u * k` | scalar multiplication | vector |
| `-u` | element-wise negation | vector |

### Anonymous Vectors

For quick numeric code, vectors can be written directly without declaring a
named vector type:

```nia
fn main() i32 {
    let a: i32<4> = <1, 2, 3, 4>;
    let b: i32<4> = <10, 20, 30, 40>;

    println(a + b);     // [11, 22, 33, 44]
    println(a * b);     // [10, 40, 90, 160]
    println(a @ b);     // 300

    0
}
```

The explicit anonymous-vector type spelling is `T<N>`: element type `T`,
length `N`. The same arithmetic works for integer and float vectors.

Use `T<>` for a reference-counted heap anonymous vector:

```nia
let v: f64<> = <1.0, 2.0, 3.0>;
println(vector_len(v));
println(len(v));
println(vector_refcount(v));

let shared: f64<> = vector_clone(v);
vector_set(shared, 1, 9.0);
println(vector_get(v, 1));
vector_drop(shared);
vector_drop(v);
```

### Complex Numbers And Trig

`Complex` is a built-in struct-shaped type with `f64` fields:

```nia
let z = complex(1.0, 2.0);
let w = Complex { re: 3.0, im: 4.0 };

let sum = complex_add(z, w);
let product = complex_mul(sum, cis(PI));
let scaled = complex_scale(product, 0.5);
let ratio = complex_div(scaled, complex(1.0, -1.0));

println(ratio);
println(sin(PI) + cos(0.0));
```

Available helpers: `complex`, `complex_add`, `complex_sub`, `complex_mul`,
`complex_scale`, `complex_div`, `sin`, `cos`, `PI`, and `cis`.

## Matrices

Matrices are built with `matrix([...])`:

```nia
let A = matrix([
    [1, 2, 3],
    [4, 5, 6],
]);

let B = matrix([
    [7, 8, 9],
    [10, 11, 12],
]);
```

A matrix is a built-in heap-backed value. Internally the compiler tracks the
element type and dimensions when it can. User code annotates the type as `T[]`
where `T` is the element type (e.g. `i32[]`, `f64[]`).

### Matrix Arithmetic

Matrices support element-wise addition, subtraction, and multiplication:

```nia
fn main() i32 {
    let a = matrix([
        [1, 2],
        [3, 4],
    ]);

    let b = matrix([
        [10, 20],
        [30, 40],
    ]);

    println(a + b);     // [[11, 22], [33, 44]]
    println(b - a);     // [[9, 18], [27, 36]]
    println(a * b);     // [[10, 40], [90, 160]]
    println(a * 3);     // [[3, 6], [9, 12]]

    matrix_drop(a);
    matrix_drop(b);
    0
}
```

### Matrix Multiplication

Use `@` for the linear algebra matrix product:

```nia
fn main() i32 {
    let a = matrix([
        [1, 2, 3],
        [4, 5, 6],
    ]);

    let b = matrix([
        [7, 8],
        [9, 10],
        [11, 12],
    ]);

    let c = a @ b;
    println(c);         // [[58, 64], [139, 154]]

    matrix_drop(a);
    matrix_drop(b);
    matrix_drop(c);
    0
}
```

The dimensions follow the usual rule:

```text
(m x n) @ (n x p) -> (m x p)
```

### Matrix-Vector and Vector-Matrix Products

`@` also works between matrices and vectors:

```nia
vector Vec3 i32 [X, Y, Z]
vector Vec2 i32 [R, S]

fn main() i32 {
    let a = matrix([
        [1, 2, 3],
        [4, 5, 6],
    ]);

    let x = Vec3 [X: 10, Y: 20, Z: 30];
    let y = Vec2 [R: 7, S: 8];

    let ax: Vec2 = a @ x;
    let ya: Vec3 = y @ a;

    println(ax);        // (i32 {"R": 140, "S": 320})
    println(ya);        // (i32 {"X": 39, "Y": 54, "Z": 69})

    matrix_drop(a);
    0
}
```

Rules:

```text
Matrix(m x n) @ Vector(n) -> Vector(m)
Vector(m) @ Matrix(m x n) -> Vector(n)
```

Anonymous vectors use the same operator:

```nia
let left = matrix([
    [1, 2, 3],
    [4, 5, 6],
]) @ <10, 20, 30>;

let right = <7, 8> @ matrix([
    [1, 2, 3],
    [4, 5, 6],
]);

println(left);          // [140, 320]
println(right);         // [39, 54, 69]
```

### Outer Product

The `outer` builtin builds a matrix from two vectors:

```nia
let u = <1, 2, 3>;
let v = <10, 20>;

let m = outer(u, v);
println(m);             // [[10, 20], [20, 40], [30, 60]]

matrix_drop(m);
```

### Determinant

The determinant is exposed as a `Matrix` method:

```nia
fn main() i32 {
    let m = matrix([
        [1, 2, 0, 1, 3],
        [2, 5, 1, 0, 4],
        [0, 1, 3, 2, 1],
        [1, 0, 2, 4, 2],
        [3, 1, 0, 2, 5],
    ]);

    println(m.det());

    matrix_drop(m);
    0
}
```

Only square matrices have determinants.

### Larger Matrix/Vector Sample

The project includes a larger matrix-vector example with roughly a thousand
elements and non-uniform generated values:

```bash
cargo run -- examples/sample_matrix_vector_large.nia
```

It is useful as a smoke test for generated loops and larger dense values.

## Matrix Ownership

`Matrix` values are reference-counted heap handles in the runtime. For now,
matrix lifetime management is explicit:

```nia
let m = matrix([
    [1, 2],
    [3, 4],
]);

println(m.det());
matrix_drop(m);
```

This is intentionally simple while the language is young. Long term, this is
one of the areas where the compiler can grow more ownership and lifetime help.

## The Rest of the Language

NiaLang is not only a matrix calculator. It has a small general-purpose core
around the linear algebra features.

### Types

Primitive types:

```nia
let i: i32 = 42;
let f: f64 = 3.14;
let ok: bool = true;
let msg: string = "hello";
```

Arrays:

```nia
let xs = [1, 2, 3, 4];
println(xs[0]);
```

Pointers and heap allocation:

```nia
let p = box(123);
println(*p);
free(p);
```

### Control Flow

```nia
fn main() i32 {
    let n = 5;

    if n > 0 {
        println("positive");
    } else {
        println("not positive");
    }

    let acc = 0;
    let i = 0;
    while i < 5 {
        acc = acc + i;
        i = i + 1;
    }

    println(acc);
    0
}
```

### Scoped Blocks

`quant { ... }` and `gpu { ... }` are reserved syntax for future specialized
behavior. Today they act like normal block scopes: bindings declared inside do
not escape, while assignments to outer variables still work.

```nia
fn main() i32 {
    let x = 1;
    let y = 0;

    quant {
        let local = 41;
        y = x + local;
    }

    gpu {
        y = y + 1;
    }

    y
}
```

### Structs, Enums, Match

```nia
struct Point {
    x: i32,
    y: i32,
}

enum Shape {
    Dot,
    Circle(i32),
    Rect(i32, i32),
}

fn area(shape: Shape) i32 {
    match shape {
        Shape::Dot => 0,
        Shape::Circle(r) => r * r,
        Shape::Rect(w, h) => w * h,
    }
}
```

### Impl Methods

NiaLang has Rust-style `impl` blocks as syntax sugar over normal functions:

```nia
struct Counter {
    value: i32,
}

impl Counter {
    fn new(value: i32) Counter {
        Counter { value: value }
    }

    fn inc(self) Counter {
        Counter { value: self.value + 1 }
    }

    fn get(&self) i32 {
        self.value
    }
}

fn main() i32 {
    let c = Counter::new(10).inc();
    println(c.get());

    0
}
```

Both `self` and `&self` are supported in method syntax. Mutable self is not part
of the language yet.

## Example Map

Good places to start:

| File | What it shows |
| --- | --- |
| `examples/sample_linalg_commented.nia` | guided linear algebra tour |
| `examples/sample_vector.nia` | named vector basics |
| `examples/sample_vector_arith.nia` | vector arithmetic |
| `examples/sample_anon_vector.nia` | anonymous vectors |
| `examples/sample_matrix_arith.nia` | matrix arithmetic and multiplication |
| `examples/sample_matrix_vector.nia` | matrix-vector and vector-matrix products |
| `examples/sample_matrix_vector_large.nia` | larger dense matrix-vector smoke test |
| `examples/sample_matrix_det.nia` | determinant as `m.det()` |
| `examples/sample_complex.nia` | complex numbers, trig, and `cis` |
| `examples/sample_matrix_rc.nia` | explicit matrix lifetime management |
| `examples/sample_impl_methods.nia` | `impl`, `self`, and `&self` |
| `examples/sample_all.nia` | broad language feature sample |

## Project Status

NiaLang is an experimental compiler and language playground.

Currently available:

- scalar arithmetic, variables, functions, and control flow
- arrays, structs, enums, pattern matching, pointers, and heap allocation
- named and anonymous vectors
- complex numbers, `sin`, `cos`, `PI`, and `cis`
- dense matrices
- vector arithmetic, dot products, outer products
- matrix arithmetic and matrix multiplication
- matrix-vector and vector-matrix multiplication
- determinant as a `Matrix` method
- Rust-style `impl` method syntax

Still intentionally small or unfinished:

- no sparse matrices
- no eigenvalues, QR, SVD, or advanced decomposition APIs
- explicit matrix lifetime management
- limited diagnostics compared with production languages
- experimental syntax and type inference

The sweet spot today is compact examples, compiler experiments, and dense linear
algebra programs that should read close to the math.
