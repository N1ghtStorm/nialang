# Current ownership model

Status: current implementation note for the abilities rollout.

Nia now has language-level ownership checks for non-copy values and automatic
cleanup for values with `drop`. The low-level runtime helpers are still present
for compatibility and compiler lowering, but ordinary user code should prefer
abilities syntax.

## Summary

Preferred user-facing forms:

```nia
let y = x.clone();
drop(x);
```

`drop(x)` moves `x`, so using `x` afterwards is a type error. If a live
droppable local reaches an ordinary scope exit, the compiler emits cleanup.

## Plain values

Scalar primitives, strings, fixed arrays, fixed anonymous vectors, and `Complex`
support language-level `copy`, `clone`, and `drop` behavior where appropriate.
Copy values can be reused after assignment or argument passing. Non-copy owners
must be moved, cloned, or explicitly dropped.

User-defined structs, enums, and named vectors declare abilities with
`has copy, clone, drop, deref`. A `copy` declaration requires `clone`.

## Matrix handles

`Matrix` is a built-in heap-backed runtime handle. It is not `copy`, because a
bitwise copy would duplicate ownership of the same heap allocation. It supports:

- `m.clone()` - allocate an independent matrix and clone/copy its cells
- `drop(m)` - free the owned matrix storage
- automatic cleanup at scope exit for live matrix owners

Shape and element helpers remain available:

- `matrix(...)`
- `matrix_get`, `matrix_set`, `matrix_rows`, `matrix_cols`, `matrix_len`

Compatibility helpers `matrix_clone(m)` and `matrix_drop(m)` still compile, but
new user code should prefer `m.clone()` and `drop(m)`.

## Heap anonymous vectors

`T<>` is a unique heap-owned anonymous vector with dynamic length. It is
distinct from fixed-size anonymous vectors `T<N>`.

Preferred ownership operations:

- `v.clone()` - allocate an independent vector and clone/copy its elements
- `drop(v)` - free the owned vector storage
- automatic cleanup at scope exit for live vector owners

Other public helpers:

- `vector_get`, `vector_set`, `vector_len`
- `len(v)`

Compatibility helpers `vector_clone(v)` and `vector_drop(v)` still compile, but
new user code should prefer `v.clone()` and `drop(v)`.

## Dynamic lists

`List[T]` is a growable heap-backed list value:

- `list_new[T]()`
- `list_with_capacity[T](capacity)`
- `xs.len()`
- `xs.capacity()`
- `xs.push(value)`
- `xs.get(index)`
- `xs.clone()`
- `drop(xs)`

Lists do not expose public low-level `list_clone` or `list_drop` helpers.

## Function values

Function values are lowered as `{ code, env, drop, clone }`.

Top-level function values and non-capturing closures behave like cheap function
pointers. Capturing closures own their environment:

- capturing closures are not shallow-copyable
- cloneable captured environments can be duplicated with `.clone()`
- non-cloneable captured environments reject `.clone()`
- `drop(f)` releases the captured environment

## Raw allocation and dereference

Nia still has low-level pointer allocation helpers:

- `alloc(value)` - allocate storage and return a pointer/reference value
- `realloc(ptr, value)` - reallocate storage for a new value
- `dealloc(ptr)` - explicitly free storage

Pointer/reference values can be dereferenced with `*p`, and assignment through a
pointer is supported with `*p = value`. This surface remains low-level; automatic
smart-pointer ownership is a later design step.

## Low-Level Runtime Helpers

The following helpers are retained for compatibility, tests, and internal
lowering:

- `matrix_clone`
- `matrix_drop`
- `vector_clone`
- `vector_drop`

They are not the preferred public style. New examples and docs should use
`.clone()`, `drop(x)`, and automatic scope cleanup unless they are explicitly
demonstrating runtime internals.
