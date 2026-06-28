# Atomics Plan

Status: Phase 0 through Phase 5 are implemented. Phase 6 is next.

## Goal

Add CPU atomics to Nia without introducing general-purpose generics.

Nia does not have user-level generics today, so the language surface must not be:

```nia
Atomic[T]
```

Instead, atomics are concrete built-in types for integers and booleans, plus one
special built-in pointer form:

```nia
AtomicBool
AtomicI8
AtomicU8
AtomicI16
AtomicU16
AtomicI32
AtomicU32
AtomicI64
AtomicU64
AtomicI128
AtomicU128
AtomicIsize
AtomicUsize
AtomicPtr[T]
```

`AtomicPtr[T]` is not evidence of general generics. It is a compiler-known type
constructor, like `List[T]`, but restricted to atomic storage of `&T` values.

## Design Principles

- No generic `Atomic[T]`.
- No user-definable atomic types.
- Atomic values are address-based storage cells, not ordinary scalar rvalues.
- Atomic operations must be explicit.
- Ordinary reads/writes of atomic values are rejected.
- Pointer atomics are separate from integer atomics.
- Atomic operations lower directly to LLVM atomic instructions.
- QIR atomics are out of scope.
- GPU atomics are out of scope until `gpu {}` has a real backend model.

## Source Surface

Construction:

```nia
let ready: AtomicBool = atomic_bool(false);
let slot: AtomicPtr[i32] = atomic_ptr(&value);
let counter: AtomicI32 = atomic_i32(0);
```

Operations are methods on atomic lvalues:

```nia
let was_ready = ready.swap(true, Ordering::AcqRel);
let old_bits = ready.fetch_xor(true, Ordering::AcqRel);

let current = slot.load(Ordering::Acquire);
slot.store(&other, Ordering::Release);

let x = counter.load(Ordering::Acquire);
counter.store(1, Ordering::Release);
let old = counter.fetch_add(1, Ordering::AcqRel);
```

Compare-exchange returns whether the exchange happened:

```nia
let ok = ready.compare_exchange(
    false,
    true,
    Ordering::AcqRel,
    Ordering::Acquire,
);
```

This MVP does not return the observed old value. That can be added later as a
small enum/tuple result once the language has a settled ergonomic result shape.

Fence:

```nia
atomic_fence(Ordering::SeqCst);
```

## Ordering

Add a built-in enum:

```nia
enum Ordering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}
```

The ordering argument must be a compile-time literal variant such as
`Ordering::Acquire`. Storing an ordering in a variable and passing it to an
atomic operation is rejected for now, because LLVM IR needs the ordering in the
instruction text.

LLVM mapping:

| Nia | LLVM |
| --- | --- |
| `Ordering::Relaxed` | `monotonic` |
| `Ordering::Acquire` | `acquire` |
| `Ordering::Release` | `release` |
| `Ordering::AcqRel` | `acq_rel` |
| `Ordering::SeqCst` | `seq_cst` |

Allowed orderings:

| Operation | Allowed |
| --- | --- |
| `load` | `Relaxed`, `Acquire`, `SeqCst` |
| `store` | `Relaxed`, `Release`, `SeqCst` |
| `swap` | all |
| `fetch_add`, `fetch_sub` | all |
| `fetch_and`, `fetch_or`, `fetch_xor` | all |
| `compare_exchange` success | all |
| `compare_exchange` failure | `Relaxed`, `Acquire`, `SeqCst`; not stronger than success |
| `atomic_fence` | `Acquire`, `Release`, `AcqRel`, `SeqCst` |

## Type Rules

Atomic types are not normal copyable values:

```nia
let a: AtomicBool = atomic_bool(false);
let b = a;          // error
println(a);         // error
a = atomic_bool(true);  // error
```

Allowed:

```nia
let a: AtomicBool = atomic_bool(false);
a.store(true, Ordering::Release);
let x = a.load(Ordering::Acquire);
```

MVP receiver rule:

- `load`, `store`, `swap`, `fetch_*`, and `compare_exchange` require an atomic
  lvalue receiver.
- Initially support local variables and `*p` where `p: &AtomicBool` or similar.
- Atomic struct fields can be a second step after lvalue-address extraction is
  shared with field assignment.

Atomic abilities:

- no `copy`
- no `clone`
- no `deref`
- no owning `drop` behavior; dropping an atomic cell is a no-op
- `send`/`sync` are treated as built-in properties once the language has real
  threads/shared ownership

## Supported Operations By Type

`AtomicBool`:

- `load(order) -> bool`
- `store(value: bool, order) -> ()`
- `swap(value: bool, order) -> bool`
- `compare_exchange(current: bool, new: bool, success, failure) -> bool`
- `fetch_and(value: bool, order) -> bool`
- `fetch_or(value: bool, order) -> bool`
- `fetch_xor(value: bool, order) -> bool`

`AtomicPtr[T]`:

- `load(order) -> &T`
- `store(value: &T, order) -> ()`
- `swap(value: &T, order) -> &T`
- `compare_exchange(current: &T, new: &T, success, failure) -> bool`

Pointer arithmetic atomics are intentionally excluded from the MVP.

Integer atomics:

- `load(order) -> T`
- `store(value: T, order) -> ()`
- `swap(value: T, order) -> T`
- `compare_exchange(current: T, new: T, success, failure) -> bool`
- `fetch_add(value: T, order) -> T`
- `fetch_sub(value: T, order) -> T`
- `fetch_and(value: T, order) -> T`
- `fetch_or(value: T, order) -> T`
- `fetch_xor(value: T, order) -> T`

## Compiler Changes

### AST

Add concrete variants to `Ty`:

```rust
AtomicBool,
AtomicI8,
AtomicU8,
AtomicI16,
AtomicU16,
AtomicI32,
AtomicU32,
AtomicI64,
AtomicU64,
AtomicI128,
AtomicU128,
AtomicIsize,
AtomicUsize,
AtomicPtr(Box<Ty>),
```

This is intentionally verbose in the semantic type model. It keeps the source
language honest: there is no hidden general `Atomic<T>`.

### Parser

Recognize concrete atomic names as reserved built-in types.

Recognize only this bracketed atomic form:

```nia
AtomicPtr[T]
```

Do not parse `Atomic[T]`.

### Built-in Names

Reserve:

- atomic type names
- `Ordering`
- `atomic_bool`
- `atomic_ptr`
- `atomic_i8`, `atomic_u8`
- `atomic_i16`, `atomic_u16`
- `atomic_i32`, `atomic_u32`
- `atomic_i64`, `atomic_u64`
- `atomic_i128`, `atomic_u128`
- `atomic_isize`, `atomic_usize`
- `atomic_fence`

### Typechecker

Add helpers:

- `is_atomic_ty`
- `atomic_value_ty`
- `is_atomic_int_ty`
- `is_atomic_ptr_ty`
- `parse_ordering_literal`
- `check_atomic_ordering_for_op`
- `check_atomic_lvalue_receiver`

Reject:

- ordinary rvalue reads of atomic cells
- assignment to an atomic cell with `=`
- passing atomics by value
- returning atomics by value
- atomic operations with invalid ordering
- `AtomicPtr[()]`
- `AtomicPtr[qubit]` / `AtomicPtr[result]`

Constructor type rules:

- `atomic_bool(bool) -> AtomicBool`
- `atomic_ptr(&T) -> AtomicPtr[T]`
- `atomic_i32(i32) -> AtomicI32`, etc.

### Codegen

Represent atomic storage with the underlying LLVM type:

| Nia | LLVM storage |
| --- | --- |
| `AtomicBool` | `i1` |
| `AtomicPtr[T]` | `ptr` |
| `AtomicI8` / `AtomicU8` | `i8` |
| `AtomicI16` / `AtomicU16` | `i16` |
| `AtomicI32` / `AtomicU32` | `i32` |
| `AtomicI64` / `AtomicU64` / `AtomicIsize` / `AtomicUsize` | `i64` |
| `AtomicI128` / `AtomicU128` | `i128` |

Initialization may use a normal store because the cell is not shared yet.

Operations lower to LLVM:

```llvm
%v = load atomic i32, ptr %cell acquire, align 4
store atomic i32 %v, ptr %cell release, align 4
%old = atomicrmw add ptr %cell, i32 %v acq_rel
%pair = cmpxchg ptr %cell, i32 %current, i32 %new acq_rel acquire
%ok = extractvalue { i32, i1 } %pair, 1
fence seq_cst
```

Pointer `swap` uses a `cmpxchg` retry loop instead of `atomicrmw xchg`,
because LLVM does not accept `atomicrmw xchg` over opaque pointer values.

Implementation helpers:

- compute atomic storage type
- compute alignment
- emit atomic load/store/swap/fetch/cmpxchg/fence
- extract lvalue pointer for atomic method receivers

### LLVM Target Notes

`i128` atomics may need target/runtime support. The implementation should either:

- support them when LLVM accepts the emitted IR for the target, or
- gate them behind a clear diagnostic and keep `AtomicI128`/`AtomicU128` as
  Phase 5.

Do not silently lower 128-bit atomics to non-atomic operations.

## Phases

Phase 0 is only shared groundwork. The first user-visible atomic type is
`AtomicBool`, and the second is `AtomicPtr[T]`.

### Phase 0: Shared Groundwork

Status: implemented.

Scope:

- reserve atomic type/function names
- add `Ordering` as a built-in enum-like type
- parse `Ordering::Relaxed`, `Ordering::Acquire`, `Ordering::Release`,
  `Ordering::AcqRel`, and `Ordering::SeqCst` as compile-time ordering literals
- add ordering validation helpers
- add codegen helpers for LLVM ordering text and atomic alignment
- add lvalue-address extraction helper for atomic method receivers

No public atomic type needs to work at the end of this phase.

### Phase 1: `AtomicBool`

Status: implemented.

Goal: make the smallest useful atomic cell work end to end.

Source surface:

```nia
let ready: AtomicBool = atomic_bool(false);
let old = ready.swap(true, Ordering::AcqRel);
let now = ready.load(Ordering::Acquire);
ready.store(false, Ordering::Release);
let changed = ready.compare_exchange(
    false,
    true,
    Ordering::AcqRel,
    Ordering::Acquire,
);
let bits = ready.fetch_xor(true, Ordering::AcqRel);
```

Compiler work:

- add `Ty::AtomicBool`
- parse `AtomicBool`
- add `atomic_bool(bool) -> AtomicBool`
- support methods `load`, `store`, `swap`, `compare_exchange`, `fetch_and`,
  `fetch_or`, and `fetch_xor`
- reject ordinary reads, assignment, pass-by-value, and return-by-value
- lower storage as `i1`
- emit `load atomic`, `store atomic`, `atomicrmw xchg`, boolean
  `atomicrmw and/or/xor`, and `cmpxchg`
- emit `atomic_fence`

Tests:

- parser accepts `AtomicBool`
- typechecker accepts valid bool atomic operations
- typechecker rejects ordinary `println(ready)`, `let x = ready`, and
  `ready = atomic_bool(false)`
- codegen contains `load atomic i1`, `store atomic i1`, `atomicrmw`, `cmpxchg`,
  and `fence`

### Phase 2: `AtomicPtr[T]`

Status: implemented.

Goal: add atomic storage for raw Nia pointers without introducing general
generics.

`AtomicPtr[T]` is a compiler-known special form. It is not `Atomic[T]`.

Source surface:

```nia
let a: i32 = 1;
let b: i32 = 2;
let slot: AtomicPtr[i32] = atomic_ptr(&a);

let current = slot.load(Ordering::Acquire);
slot.store(&b, Ordering::Release);
let old = slot.swap(&a, Ordering::AcqRel);
let changed = slot.compare_exchange(
    &a,
    &b,
    Ordering::AcqRel,
    Ordering::Acquire,
);
```

Compiler work:

- add `Ty::AtomicPtr(Box<Ty>)`
- parse exactly `AtomicPtr[T]`
- keep rejecting `Atomic[T]`
- add `atomic_ptr(&T) -> AtomicPtr[T]`
- support methods `load`, `store`, `swap`, and `compare_exchange`
- reject `AtomicPtr[()]`, `AtomicPtr[qubit]`, and `AtomicPtr[result]`
- lower storage as `ptr`
- emit `load atomic ptr`, `store atomic ptr`, a `cmpxchg` loop for `swap`,
  and `cmpxchg`

Tests:

- parser accepts `AtomicPtr[i32]`
- parser rejects `Atomic[i32]`
- typechecker accepts pointer load/store/swap/compare-exchange
- typechecker rejects pointer arithmetic atomics
- codegen contains pointer atomic LLVM instructions

### Phase 3: Small Integer Atomics

Status: implemented.

Goal: add the most useful integer atomics after bool and pointer atomics are
already stable.

Initial integer set:

- `AtomicI32`
- `AtomicU32`
- `AtomicI64`
- `AtomicIsize`
- `AtomicUsize`

Compiler work:

- add type variants and constructors
- support `load`, `store`, `swap`, `compare_exchange`
- support `fetch_add`, `fetch_sub`, `fetch_and`, `fetch_or`, `fetch_xor`
- lower to `i32` or `i64`
- sample: `examples/sample_atomic_int.nia`

### Phase 4: Narrow Integer Atomics

Status: implemented.

Add:

- `AtomicI8`, `AtomicU8`
- `AtomicI16`, `AtomicU16`
- `AtomicU64`

This phase should mostly reuse Phase 3 machinery, but needs explicit alignment
and signed/unsigned test coverage.

Sample: `examples/sample_atomic_narrow_int.nia`.

### Phase 5: 128-Bit Integer Atomics

Status: implemented.

Add:

- `AtomicI128`
- `AtomicU128`

Only ship these if LLVM and the target toolchain preserve real atomic behavior.
Otherwise keep the types reserved and emit a clear diagnostic when used.

Sample: `examples/sample_atomic_i128.nia`.

### Phase 6: Ergonomics And Documentation

- support atomic struct fields once field-address extraction is shared and
  tested
- document the stable subset in `spec.txt`
- add the stable subset to `README.md`
- keep QIR/GPU atomics explicitly out of scope

## Tests

Parser tests:

- concrete atomic type names parse
- `AtomicPtr[i32]` parses
- `Atomic[i32]` fails
- `AtomicU32` is reserved as a concrete atomic type name

Typecheck ok fixtures:

- `ok_atomic_bool.nia`
- `ok_atomic_ptr.nia`
- `ok_atomic_compare_exchange.nia`
- `ok_atomic_fence.nia`
- `ok_atomic_int.nia`
- `ok_atomic_narrow_int.nia`
- `ok_atomic_i128.nia`

Typecheck error fixtures:

- ordinary atomic read
- ordinary atomic assignment
- pass atomic by value
- return atomic by value
- invalid load ordering
- invalid store ordering
- invalid compare-exchange failure ordering
- `AtomicPtr[()]`

Codegen tests:

- `load atomic`
- `store atomic`
- `atomicrmw add/sub/and/or/xor`
- `atomicrmw xchg`
- `cmpxchg`
- `fence`
- alignment for each supported storage width

## Non-Goals

- General-purpose generics.
- Generic `Atomic[T]`.
- Atomic floats.
- Atomic structs/enums.
- Pointer arithmetic atomics.
- Thread spawning API.
- Shared ownership API.
- QIR atomics.
- GPU atomics.
