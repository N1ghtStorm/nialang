# Current ownership model

Status: implementation note for abilities Phase 0.

This document records the ownership and cleanup behavior that exists before
language-level abilities (`copy`, `clone`, `drop`, `deref`) are implemented.
It is descriptive only: Phase 0 does not change compiler behavior, syntax, or
runtime lowering.

## Summary

Nia currently has several heap-backed values, but lifetime management is mostly
manual and implemented through reserved builtins or methods. The compiler does
not yet run an ownership pass, does not insert automatic drops, and does not
track moved locals for ordinary native values.

The future abilities work should preserve this behavior until the relevant
staged phase explicitly changes it.

## Plain values

Primitive scalar values, fixed arrays, structs, enums, and fixed-size vectors
currently behave like ordinary by-value values in the existing typechecker and
codegen. There is no formal `copy` ability yet, and there is no use-after-move
diagnostic for non-copy user-defined values.

This means the abilities rollout needs a compatibility path: existing scalar
and aggregate examples should keep compiling until formal `copy` rules are
introduced.

## Matrix handles

`Matrix` is a built-in heap-backed runtime handle. Matrix values are reference
counted by the runtime representation, and user code currently manages sharing
and cleanup explicitly.

Reserved matrix helpers include:

- `matrix(...)` - allocate/build a matrix value
- `matrix_clone(m)` - increment the matrix reference count and return the same
  handle
- `matrix_refcount(m)` - inspect the current reference count
- `matrix_drop(m)` - decrement the reference count and free matrix storage when
  it reaches zero
- `matrix_get`, `matrix_set`, `matrix_rows`, `matrix_cols`, `matrix_len`

There is no automatic `matrix_drop` insertion at scope exit yet.

## Heap anonymous vectors

`T<>` is a reference-counted heap anonymous vector with dynamic length. It is
distinct from fixed-size anonymous vectors `T<N>`.

Reserved heap-vector helpers include:

- `vector_clone(v)` - increment the vector reference count and return the same
  handle
- `vector_refcount(v)` - inspect the current reference count
- `vector_drop(v)` - decrement the reference count and free vector storage when
  it reaches zero
- `vector_get`, `vector_set`, `vector_len`
- `len(v)` - length helper for heap vectors

There is no automatic `vector_drop` insertion at scope exit yet.

## Dynamic lists

`List[T]` is a growable heap-backed list value. The current public surface is
intentionally small:

- `list_new[T]()`
- `list_with_capacity[T](capacity)`
- `xs.len()`
- `xs.capacity()`
- `xs.push(value)`
- `xs.get(index)`

Unlike matrices and heap vectors, lists do not currently expose public
`list_clone` or `list_drop` helpers. Index syntax and explicit list cleanup are
also not part of the current surface.

This is important for the abilities rollout: list `clone` / `drop` glue should
not be assumed to exist until the primitive integration phase adds or defines
it.

## Raw allocation and dereference

Nia currently has low-level pointer allocation helpers:

- `alloc(value)` - allocate storage and return a pointer/reference value
- `realloc(ptr, value)` - reallocate storage for a new value
- `dealloc(ptr)` - explicitly free storage

Pointer/reference values can already be dereferenced with `*p`, and assignment
through a pointer is supported with `*p = value`.

This dereference behavior is built into the current expression/typechecking and
codegen paths. It does not go through a formal `deref` ability yet.

## What Phase 0 guarantees

Phase 0 is documentation only.

It does not:

- add `has`
- add `copy`, `clone`, `drop`, or `deref` keywords
- add move checking
- add automatic drop insertion
- change matrix, heap-vector, list, or pointer lowering
- replace existing helpers such as `matrix_drop`, `vector_drop`, or `dealloc`

Later phases can use this document as the compatibility baseline.
