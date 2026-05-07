# NiaLang

Minimal compiled language that lowers to LLVM IR and runs via `clang`.

The project currently focuses on a compact but expressive core:
- fixed-size arrays (`[T; N]`) with indexing and mutation,
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

### Expressions and Operators

- Arithmetic: `+`, `-`, `*`, `/`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Indexing: `arr[i]`
- Field access: `obj.x`, `tuple.0`
- Function calls: `foo(a, b)`

### Builtins

- `println(x)` — prints values (including arrays, structs, enums, pointers)
- `len(arr)` — returns compile-time array length as `i32`
- `alloc(v)` / `realloc(ptr, v)` / `dealloc(ptr)`

## Large Working Examples

All examples below are mirrored in fixtures and validated by `cargo test`.

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

