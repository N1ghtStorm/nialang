# NiaLang

**NiaLang** is a small experimental compiled language for numeric, linear
algebra, and quantum-programming experiments. It treats vectors and matrices as
first-class values, has a compact Rust-like syntax, lowers native programs
through LLVM IR and `clang`, and can emit QIR for the bundled quantum runner.

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

The goal is a language that feels natural for dense numeric and quantum code
while staying simple enough to understand and hack on.

## Quick Start

Requirements:

- a recent Rust toolchain;
- `clang` for native executables, assembly, and shared libraries;
- pthread-compatible native threading support for `spawn`, `Arc`, `Mutex`, `RwLock`, and `Condvar`
  examples; the current MVP targets macOS/Linux-style pthread platforms;
- the optional `qir-runner` feature for quantum programs.

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

Emit textual LLVM IR:

```bash
cargo run -- examples/sample_all.nia --emit-ll build/sample_all.ll
```

Compile and run a QIR quantum sample:

```bash
cargo run -r --features qir-runner -- examples/quantum/qubit_create.nia -q
```

Run the fixed `N = 15` Shor demonstration:

```bash
cargo run -r --features qir-runner -- examples/quantum/qubit_shore.nia -q
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

Modules follow Rust-style item paths:

```nia
mod math {
    fn add(a: i32, b: i32) i32 {
        a + b
    }
}

fn main() i32 {
    math::add(40, 2)
}
```

When compiling from a file, `mod math;` loads `math.nia` or `math/mod.nia`.
Nested modules can use `self::`, `super::`, and `crate::` paths. Privacy is not
implemented yet; `pub` is accepted as a no-op.

CLI modes:

| Command | Behavior |
| --- | --- |
| `nialang file.nia` | compile with `clang` and run |
| `nialang file.nia -o out.ll` | run and also keep generated LLVM IR |
| `nialang file.nia --emit-ll [out.ll]` | write LLVM IR without running |
| `nialang file.nia --emit-asm [out.s]` | write native assembly |
| `nialang file.nia --lib -o library` | build a shared library from `extern fn` exports |
| `nialang file.nia -q [-o out.ll]` | lower to QIR and run through `qir-runner` |

## Integer And Bitwise Operators

All integer types support arithmetic, remainder, bitwise operations, and
compound assignment:

| Operator | Meaning |
| --- | --- |
| `+`, `-`, `*`, `/` | integer arithmetic |
| `a % b` | signed or unsigned remainder |
| `a & b` | bitwise AND |
| `a \| b` | bitwise OR |
| `a ^ b` | bitwise XOR |
| `~a` | bitwise complement |
| `a << n` | left shift |
| `a >> n` | arithmetic shift for signed integers, logical shift for unsigned integers |

The corresponding compound assignments are available: `+=`, `-=`, `*=`, `/=`,
`%=`, `&=`, `|=`, `^=`, `<<=`, and `>>=`.

Booleans support logical negation with `!value`.

Numeric literals may use `_` between digits for readability:

```nia
let population = 1_000_000;
let ratio = 3.141_592;
let scale = 1.0e1_0;
```

```nia
let flags: u8 = 12;
let masked = flags & 10;
let rotated_part = flags << 2;
let odd = (flags & 1) == 1;
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

Use `T<>` for a unique heap-owned anonymous vector:

```nia
let v: f64<> = <1.0, 2.0, 3.0>;
println(len(v));

let copied: f64<> = v.clone();
vector_set(copied, 1, 9.0);
println(vector_get(v, 1));
println(vector_get(copied, 1));
drop(copied);
drop(v);
```

`Matrix`, heap anonymous vectors `T<>`, and dynamic lists `List[T]` support
language-level `.clone()` and `drop(x)`. The older low-level helpers such as
`matrix_clone`, `matrix_drop`, `vector_clone`, and `vector_drop` remain
available for compatibility and internal lowering, but new user code should
prefer `.clone()`, `drop(x)`, and automatic scope cleanup.

### Dynamic Lists

`List[T]` is a growable heap-backed list for values of type `T`.
Constructors take the element type in brackets:

```nia
let bytes = list_new[u8]();
let zs: List[Complex] = list_with_capacity[Complex](2);

bytes.push(10);
bytes.push(20);

println(bytes.len());
println(bytes.capacity());
println(bytes.get(1));
```

The first list surface is intentionally small: `len`, `capacity`, `push`, and
`get`. Index syntax and explicit list cleanup are not implemented yet.

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

### Crypto And Merkle Tree Primitives

Merkle trees are a first-class language primitive in Nia. SHA-256 and Merkle
tree helpers are exposed as compiler builtins, so ordinary `.nia` programs can
build roots, proofs, and verification checks without `import`, `include`, or a
manually copied source prelude:

```nia
let data: [[u8; 3]; 2] = [[1, 2, 3], [4, 5, 6]];
let root = merkle_root_from_data(data);
let left = merkle_leaf_hash(data[0]);
let right = merkle_leaf_hash(data[1]);
let proof: [[u8; 32]; 1] = [right];

println(digest_eq(root, merkle_node_hash(left, right)));
println(merkle_verify(root, left, 0, proof));
```

The digest type is `[u8; 32]`. `sha256` and `merkle_leaf_hash` accept fixed
byte arrays `[u8; N]`; `merkle_root` accepts `[[u8; 32]; N]`;
`merkle_root_from_data` accepts fixed-size leaves `[[u8; M]; N]`; and
`merkle_verify` accepts `(root, leaf, index, proof)` where `proof` is
`[[u8; 32]; D]`.

Merkle hashing uses domain separation:

- leaf hash: `SHA256(0x00 || data)`
- internal node hash: `SHA256(0x01 || left || right)`

For odd leaf counts, the last node is duplicated at that level.

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

    drop(a);
    drop(b);
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

    drop(c);
    drop(b);
    drop(a);
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

    drop(a);
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

drop(m);
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

    drop(m);
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

`Matrix` values are unique heap-owned runtime handles. They support
language-level deep clone/drop:

```nia
let m = matrix([
    [1, 2],
    [3, 4],
]);

let copied = m.clone();
println(m.det());
drop(copied);
drop(m);
```

If a live `Matrix`, `T<>`, or `List[T]` local is left in scope, the compiler
inserts cleanup at ordinary scope exits. Use explicit `drop(x)` when you want to
release an owner earlier. The low-level `matrix_clone`, `matrix_drop`,
`vector_clone`, and `vector_drop` helpers are kept for compatibility and runtime
tests, not as the preferred user-facing API.

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
let p: &i32 = alloc(123);
println(*p);
*p = 456;
let moved: &i32 = realloc(p, 789);
dealloc(moved);
```

### Control Flow

```nia
fn main() i32 {
    let n = 5;

    if n > 0 {
        println("positive");
    } else {
        println("zero or negative");
    }

    let acc = 0;
    let i = 0;
    while i < 5 {
        acc = acc + i;
        i = i + 1;
    }

    for value in 0..3 {
        println(value);
    }

    loop {
        println("once");
        break;
    }

    println(acc);
    0
}
```

`if` supports `else` blocks and `else if` chains. `break` is supported by
`loop`; breaking from `while` or `for` is not implemented yet.

### Scoped Blocks

`gpu { ... }` is currently a normal scoped block reserved for future specialized
behavior: bindings declared inside do not escape, while assignments to outer
variables still work.

```nia
fn main() i32 {
    let x = 1;
    let y = 0;

    gpu {
        let local = 41;
        y = x + local;
    }

    y
}
```

## Concurrency

Nia has a small pthread-backed native concurrency surface. It is intentionally
Rust-like in shape but compiler-known rather than implemented through user-level
generics.

### Threads

Top-level functions can be spawned directly, and `move` closures can capture
values that are `send`:

```nia
fn worker() {
    println(1);
}

fn main() i32 {
    let direct: Thread = spawn worker;
    join(direct);

    let shared: Arc[Mutex[i32]] = arc_new(mutex_new(0));
    let t: Thread = spawn move || {
        let guard: MutexGuard[i32] = (*shared).lock();
        *guard = *guard + 1;
        drop(guard);
    };
    join(t);

    0
}
```

`Thread` is move-only. `join(t)` consumes the handle; `drop(t)` detaches it.
Non-`move` closure spawn is rejected, and every captured value must be `send`.

### Shared Ownership

`Arc[T]` is an atomic reference-counted shared owner:

```nia
let shared: Arc[i32] = arc_new(42);
let cloned = shared.clone();
println(*cloned);
drop(cloned);
drop(shared);
```

`Arc[T]` requires `T: send + sync`. It is not `copy`; `.clone()` bumps the
reference count and `*arc` gives read-only access to the inner value.

### Locks

`Mutex[T]` provides exclusive access and `RwLock[T]` provides many-readers or
one-writer access. Both require `T: send`; their guards are move-only RAII
tokens and are deliberately not `send`.

```nia
let m: Mutex[i32] = mutex_new(0);
let guard: MutexGuard[i32] = m.lock();
*guard = *guard + 1;
drop(guard);
drop(m);

let rw: RwLock[i32] = rwlock_new(41);
let r: RwLockReadGuard[i32] = rw.read();
println(*r);
drop(r);

let w: RwLockWriteGuard[i32] = rw.write();
*w = *w + 1;
drop(w);
drop(rw);
```

Try-lock methods return options:

```nia
let maybe_m: Option[MutexGuard[i32]] = m.try_lock();
let maybe_r: Option[RwLockReadGuard[i32]] = rw.try_read();
let maybe_w: Option[RwLockWriteGuard[i32]] = rw.try_write();
```

`RwLockReadGuard[T]` is read-only through `*guard`; `RwLockWriteGuard[T]` allows
read/write access.

### Condition Variables

`Condvar` pairs with `MutexGuard[T]`. `wait` consumes a guard, atomically
unlocks while sleeping, then returns a guard after wakeup and re-locking.
Spurious wakeups are allowed, so wait in a predicate loop:

```nia
let state: Arc[Mutex[i32]] = arc_new(mutex_new(0));
let cv: Arc[Condvar] = arc_new(condvar_new());

let waiter_state = state.clone();
let waiter_cv = cv.clone();
let waiter: Thread = spawn move || {
    let guard: MutexGuard[i32] = (*waiter_state).lock();
    while *guard == 0 {
        let next: MutexGuard[i32] = (*waiter_cv).wait(guard);
        guard = next;
    }
    println(*guard);
    drop(guard);
};

let notifier_state = state.clone();
let notifier_cv = cv.clone();
let notifier: Thread = spawn move || {
    let guard: MutexGuard[i32] = (*notifier_state).lock();
    *guard = 1;
    (*notifier_cv).notify_one();
    drop(guard);
};

join(notifier);
join(waiter);
drop(state);
drop(cv);
```

The native backend links generated programs with pthread support on non-Windows
targets. The current synchronization runtime is a Unix/macOS MVP; a portable
Windows synchronization backend is future work.

## Quantum Computing

NiaLang has a QIR backend for small static quantum programs. Quantum code is
written inside `quant { ... }` blocks or `quant fn` functions. The current
surface includes qubit registers, single-, controlled-, and three-qubit gates,
constant-angle rotations, Z-basis measurement, classical result reads, and QIR
output recording.

```nia
quant fn bell(control: qubit, target: qubit) {
    H(control);
    CNOT(control, target);
}

quant fn echo_h(q: qubit) {
    for i in 0..2 {
        H(q);
    }
}

quant fn flip(q: qubit) {
    X(q);
}

quant fn phase_like(y: qubit, z: qubit, s: qubit, t: qubit) {
    Y(y);
    Z(z);
    S(s);
    T(t);
}

quant fn controlled_phase(control: qubit, target: qubit) {
    CZ(control, target);
}

quant fn swap_pair(left: qubit, right: qubit) {
    SWAP(left, right);
}

quant fn rotate_like(rx: qubit, ry: qubit, rz: qubit, r1: qubit) {
    Rx(PI / 2.0, rx);
    Ry(PI / 4.0, ry);
    Rz(PI / 8.0, rz);
    R1(PI, r1);
}

quant fn identity_and_adjoint(i: qubit, s: qubit, t: qubit) {
    I(i);
    Sdg(s);
    Tdg(t);
}

quant fn controlled_more(control: qubit, h: qubit, y: qubit, s: qubit, t: qubit) {
    CH(control, h);
    CY(control, y);
    CS(control, s);
    CSdg(control, s);
    CT(control, t);
    CTdg(control, t);
}

quant fn three_qubit_like(control_a: qubit, control_b: qubit, target: qubit) {
    CCNOT(control_a, control_b, target);
    CCZ(control_a, control_b, target);
    CSWAP(control_a, control_b, target);
}

quant fn controlled_rotations(control: qubit, x: qubit, y: qubit, z: qubit, p: qubit) {
    CRx(PI / 2.0, control, x);
    CRy(PI / 4.0, control, y);
    CRz(PI / 8.0, control, z);
    CR1(PI, control, p);
}

fn main() i32 {
    quant {
        let a = qubit();
        let b = qubit();
        let x = qubit();
        let y = qubit();
        let z = qubit();
        let s = qubit();
        let t = qubit();
        bell(a, b);
        echo_h(a);
        controlled_phase(a, b);
        flip(x);
        phase_like(y, z, s, t);
        rotate_like(y, z, s, t);
        identity_and_adjoint(x, s, t);
        controlled_more(a, y, z, s, t);
        three_qubit_like(a, b, x);
        controlled_rotations(a, x, y, z, t);
        swap_pair(s, t);
        let ar = q_measure(a);
        let br = q_measure(b);
        let xr = q_measure(x);
        let yr = q_measure(y);
        let zr = q_measure(z);
        let sr = q_measure(s);
        let tr = q_measure(t);
        q_record(ar);
        q_record(br);
        q_record(xr);
        q_record(yr);
        q_record(zr);
        q_record(sr);
        q_record(tr);
    }

    0
}
```

The quantum surface is intentionally small:

| Syntax | Meaning |
| --- | --- |
| `quant { ... }` | quantum scope; quantum resources cannot escape it |
| `quant fn Name(...) { ... }` | quantum function; callable only from `quant` scopes |
| `for i in A..B { ... }` | static quantum loop; QIR lowering unrolls compile-time integer ranges |
| `qubit()` | create a qubit resource inside `quant` |
| `I(q)` | identity gate; leaves a qubit unchanged |
| `H(q)` | apply the Hadamard gate to a qubit |
| `X(q)` | apply the Pauli-X gate; flips `|0>` and `|1>` |
| `Y(q)` | apply the Pauli-Y gate; bit flip with phase |
| `Z(q)` | apply the Pauli-Z gate; phase flip on `|1>` |
| `S(q)` | apply the phase gate, a `pi/2` Z-axis phase rotation |
| `Sdg(q)` | apply the inverse of `S(q)` |
| `T(q)` | apply the T gate, a `pi/4` Z-axis phase rotation |
| `Tdg(q)` | apply the inverse of `T(q)` |
| `CNOT(c, t)` | controlled-X: flips target `t` when control `c` is `|1>` |
| `CZ(c, t)` | controlled-Z: applies a phase flip to `t` when control `c` is `|1>` |
| `SWAP(a, b)` | swap the quantum states of two qubits |
| `CH(c, t)` | controlled-H: applies `H(t)` when control `c` is `|1>` |
| `CY(c, t)` | controlled-Y: applies `Y(t)` when control `c` is `|1>` |
| `CS(c, t)` | controlled-S: applies `S(t)` when control `c` is `|1>` |
| `CSdg(c, t)` | controlled inverse-S: applies `Sdg(t)` when control `c` is `|1>` |
| `CT(c, t)` | controlled-T: applies `T(t)` when control `c` is `|1>` |
| `CTdg(c, t)` | controlled inverse-T: applies `Tdg(t)` when control `c` is `|1>` |
| `CCNOT(a, b, t)` | Toffoli gate: applies `X(t)` when both controls are `|1>` |
| `CCZ(a, b, t)` | controlled-controlled-Z phase flip |
| `CSWAP(c, a, b)` | Fredkin gate: swaps `a` and `b` when control `c` is `|1>` |
| `Rx(theta, q)` | rotate a qubit around the X axis by a constant `f64` angle |
| `Ry(theta, q)` | rotate a qubit around the Y axis by a constant `f64` angle |
| `Rz(theta, q)` | rotate a qubit around the Z axis by a constant `f64` angle |
| `R1(theta, q)` | apply a constant phase rotation |
| `CRx(theta, c, t)` | controlled X-axis rotation by a constant `f64` angle |
| `CRy(theta, c, t)` | controlled Y-axis rotation by a constant `f64` angle |
| `CRz(theta, c, t)` | controlled Z-axis rotation by a constant `f64` angle |
| `CR1(theta, c, t)` | controlled phase rotation by a constant `f64` angle |
| `q_measure(q)` | measure a qubit in the Z basis and return `result` |
| `q_read(r)` | read a `result` as a classical `bool` in QIR |
| `q_record(x)` | record a `result` or `bool` as QIR output |

`qubit` and `result` are quantum-only types. They cannot be returned from a
`quant` expression or printed with `println`; use `q_record(r)` to expose raw
measurement output to the QIR runner, or `q_read(r)` to turn a measurement
result into a classical `bool` before recording it. `quant fn` bodies are
checked as quantum scopes, so they can create qubits directly. Calls to
`quant fn` are rejected outside `quant { ... }`.

The current QIR lowering supports void `quant fn` calls with `qubit`,
`[qubit; N]`, and `result` parameters. Returning values from quantum functions
is reserved for future work. Rotation angles must be compile-time expressions
such as `PI`, `PI / 2.0`, or `0.125 + 0.125`. Quantum register sizes and
quantum `for` ranges are static. Measurement results can be converted to
classical `bool` values with `q_read` and used by ordinary control flow.

Run the current sample:

```bash
cargo run -r --features qir-runner -- examples/quantum/qubit_create.nia -q
```

The runner output includes QIR metadata and recorded measurement results:

```text
START
METADATA	entry_point
METADATA	qir_profiles	adaptive_profile
METADATA	required_num_qubits	0
METADATA	required_num_results	0
OUTPUT	RESULT	1
OUTPUT	RESULT	0
OUTPUT	RESULT	1
OUTPUT	RESULT	1
OUTPUT	RESULT	1
OUTPUT	RESULT	0
OUTPUT	RESULT	0
END	0
```

Because `H(q)` creates a superposition and `CNOT(c, t)` entangles the two
qubits, the recorded results can vary between runs. The same sample also lowers
`CZ(c, t)`, `SWAP(a, b)`, adjoint phase gates, controlled gates, three-qubit
gates, constant-angle rotations, and controlled rotations. You can also write
the generated QIR IR to a file:

```bash
cargo run -r --features qir-runner -- examples/quantum/qubit_create.nia -q -o build/qubit_create.ll
```

More complete examples include QFT and inverse QFT, Deutsch-Jozsa, a measured
random bit, and a specialized Shor factorization circuit for `N = 15`. The Shor
sample can produce an inconclusive phase and request another run, as expected
for a probabilistic order-finding procedure.

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
| `examples/sample_dft4.nia` | discrete Fourier transform for a 4-value signal |
| `examples/tests/ok_bitwise.nia` | remainder, bitwise operators, shifts, and compound assignment |
| `examples/sample_list.nia` | dynamic `List[T]` constructors and methods |
| `examples/sample_dft_list.nia` | list-backed discrete Fourier transform |
| `examples/sample_matrix_rc.nia` | low-level matrix RC compatibility helpers |
| `examples/sample_impl_methods.nia` | `impl`, `self`, and `&self` |
| `examples/sample_closures.nia` | non-capturing closure/function-value smoke test |
| `examples/abilities/copy_move_basics.nia` | `has copy, clone` and copy-preserving moves |
| `examples/abilities/custom_clone.nia` | ability-backed `x.clone()` and custom clone glue |
| `examples/abilities/custom_deref.nia` | custom `deref` ability through `*x` |
| `examples/abilities/explicit_drop.nia` | custom `drop(self)` and language-level `drop(x)` |
| `examples/abilities/auto_drop_scope.nia` | automatic custom-drop at scope exit |
| `examples/abilities/drop_flags.nia` | uninitialized locals, overwrites, and drop flags |
| `examples/abilities/aggregate_drop.nia` | derived struct/enum drop for user-defined values |
| `examples/abilities/closure_captures.nia` | copy-safe closure captures and function values |
| `examples/abilities/primitive_abilities.nia` | clone/drop for runtime primitives and aggregates |
| `examples/abilities/move_closure_captures.nia` | `move ||` captures and closure environment cleanup |
| `examples/abilities/function_value_abilities.nia` | copy/clone/drop behavior for `fn(...) -> ...` values |
| `examples/sample_send_sync.nia` | `send` / `sync` ability checks |
| `examples/sample_arc.nia` | `Arc[T]` shallow clone and read-only deref |
| `examples/sample_threads.nia` | top-level function thread spawn and join |
| `examples/sample_threads_closure.nia` | `spawn move ||` closure captures |
| `examples/sample_mutex.nia` | `Arc[Mutex[i32]]` shared counter across threads |
| `examples/sample_rwlock.nia` | `Arc[RwLock[i32]]` readers and writer |
| `examples/sample_condvar.nia` | `Condvar` predicate loop with `MutexGuard[T]` |
| `examples/sample_extern_lib.nia` | C ABI exports and shared-library mode |
| `examples/quantum/qubit_create.nia` | QIR gates, rotations, measurement, and result recording |
| `examples/quantum/qubit_read.nia` | read a QIR measurement result as `bool` with `q_read` |
| `examples/quantum/measure_qubits_as_byte.nia` | sample a biased 8-qubit cat state and collect byte statistics |
| `examples/quantum/random_bit.nia` | measurement-driven classical control |
| `examples/quantum/deutsch_jozsa_1bit.nia` | one-bit Deutsch-Jozsa circuits |
| `examples/quantum/qft4.nia` | 4-qubit quantum Fourier transform over a qubit register |
| `examples/quantum/iqft4.nia` | inverse 4-qubit QFT, composed with QFT as a round-trip check |
| `examples/quantum/qubit_shore.nia` | specialized Shor factorization demo for `N = 15`, `a = 2` |
| `examples/sample_all.nia` | broad language feature sample |

## Project Status

NiaLang is an experimental compiler and language playground.

Currently available:

- signed and unsigned integer types, floating-point types, strings, and booleans
- scalar arithmetic, `%`, bitwise operators, shifts, logical `!`, and compound assignment
- functions, `if`, `while`, static range `for`, and `loop` with `break`
- fixed arrays with indexing and mutation
- structs, tuple structs, enums, pattern matching, pointers, and heap allocation
- `extern fn` C ABI exports and shared-library builds
- named and anonymous vectors
- dynamic `List[T]` values with `len`, `capacity`, `push`, and `get`
- complex numbers, `sin`, `cos`, `PI`, and `cis`
- dense heap-owned matrices
- vector arithmetic, dot products, scalar multiplication, and outer products
- matrix arithmetic, matrix multiplication, and array conversions
- matrix-vector and vector-matrix multiplication
- determinant as a `Matrix` method
- Rust-style `impl` method syntax
- QIR quantum blocks/functions with qubit arrays, controlled and three-qubit
  gates, rotations, measurement, result reads, and recording

Still intentionally small or unfinished:

- `break` works only with `loop`, not `while` or `for`
- no sparse matrices
- no eigenvalues, QR, SVD, or advanced decomposition APIs
- no list index syntax or explicit list cleanup yet
- quantum register sizes, quantum loops, and rotation angles are static
- quantum functions are currently void and do not have general
  `controlled`/`adjoint` generation
- no generic register-size parameters or universal `shor(N)` implementation
- explicit matrix lifetime management
- limited diagnostics compared with production languages
- experimental syntax and type inference

The sweet spot today is compact compiler experiments, dense numeric programs,
and small quantum circuits that should read close to the underlying math.
