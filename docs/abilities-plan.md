# Abilities in NiaLang: staged implementation plan

Status: design plan.

This document describes a staged path for adding Move-style abilities to Nia.
The initial language-level abilities are:

- `copy` - values may be implicitly duplicated.
- `clone` - values may be explicitly duplicated through clone glue.
- `drop` - values may be automatically destroyed or ignored at the end of a
  scope.
- `deref` - values may be explicitly dereferenced through deref glue.

The new declaration keyword is `has`; `copy`, `clone`, `drop`, and `deref`
become ability keywords:

```nia
struct Point has copy, clone, drop {
    x: f64,
    y: f64,
}

struct I32Box has deref, drop {
    ptr: &i32,
}
```

The goal is to make ownership and automatic cleanup explicit in the type system
without requiring a full Rust borrow checker as the first step.

## Target semantics

Abilities are properties of types.

`copy` means assignment, argument passing, and local reads may duplicate the
value without invalidating the original binding:

```nia
let a = 10;
let b = a;
println(a); // ok for copy types
```

`clone` means the value can be explicitly duplicated:

```nia
let b = a.clone();
```

For unique heap-owned runtime values, clone glue allocates independent storage.
For plain copy values, clone can lower to the same value.

`drop` means the compiler is allowed to destroy the value automatically when it
leaves scope:

```nia
fn main() i32 {
    let h = FileHandle { fd: 3 };
    println(1);
    0
} // compiler inserts FileHandle::drop(h)
```

Types without `drop` are resource-like. They cannot be silently ignored, leaked,
or overwritten without being consumed.

`deref` means the value can expose a referenced target through explicit
dereference syntax or deref glue:

```nia
let value = *box;
```

Dereferencing borrows through the owner. It must not implicitly clone, move, or
drop the owner. In the first implementation, prefer explicit dereference only;
implicit deref coercions and method forwarding can come later.

For the MVP, keep a conservative invariant:

```text
copy implies clone
```

Do not make `clone` imply `copy`. Built-in/runtime primitives should receive
their formal language-level `copy`, `clone`, `drop`, and `deref` abilities only
in the final primitive-integration phase. Until then, keep existing compiler
behavior and low-level helpers for those types.

## Declaration syntax

Structs:

```nia
struct S has clone, drop {
    data: f64<>,
}

struct RefCellI32 has deref {
    ptr: &i32,
}
```

Enums:

```nia
enum OptionI32 has copy, clone, drop {
    Some(i32),
    None,
}
```

Named vectors keep their current spelling and add `has` after the axis list:

```nia
vector Vec2 f64 [X, Y] has copy, clone, drop
```

Function types do not use `has` directly. Their abilities are derived from the
function value representation after primitive integration:

- non-capturing function pointers: eventually `copy, clone, drop`
- capturing closures: derived from captured environment; initially `clone, drop`
  only if every captured field supports the required ability

## Eventual built-in ability defaults

These are the target defaults after the full abilities rollout. During the
first implementation stages, built-in primitives keep their existing compiler
behavior and low-level helper functions. They do not receive formal
language-level `copy`, `clone`, `drop`, or `deref` through the abilities system
until the final primitive-integration phase.

Primitive scalar values:

```text
(), bool, integers, floats: eventually copy, clone, drop
raw pointers / references: eventually deref, with copy/clone/drop rules chosen
by the pointer ownership model
```

Scalar primitives may keep their current trivial by-value behavior internally
for compatibility, but the abilities system should not treat primitive
`copy`/`clone`/`deref` as the first milestone.

Fixed-size arrays:

```text
[T; N] eventually has copy/clone/drop ability A if T has A
```

Fixed-size anonymous vectors:

```text
T<N> eventually has copy/clone/drop ability A if T has A
```

`deref` is not structural for arrays or vectors; an array of dereferenceable
values is not itself dereferenceable unless the language later adds an explicit
array-deref design.

Heap anonymous vectors:

```text
T<> eventually has clone, drop
```

`T<>` is not `copy`, because implicit copies would duplicate the pointer without
updating the reference count. Its language-level `clone` and `drop` abilities
are intentionally delayed until custom struct clone/drop, drop flags, and
automatic scope cleanup are already working for user-defined types.

Matrices:

```text
Matrix eventually has clone, drop
```

`Matrix` keeps `matrix_clone` / `matrix_drop` as explicit low-level operations
until the final primitive-integration phase.

Dynamic lists:

```text
List[T] eventually has drop if T has drop
List[T] eventually has clone if T has clone
List[T] is not copy
```

Structs, enums, and named vectors have only the abilities declared with `has`.
The compiler must reject an ability declaration if any field or payload cannot
support it. `deref` is different from structural abilities: in the first
implementation, a struct supports it by providing a valid custom deref method.

## Phase 0: document the current ownership model

Status: complete in [current-ownership-model.md](current-ownership-model.md).

Current Nia has heap-owned runtime handles, but lifetime management is still
explicitly exposed through compatibility helpers in several places:

- `matrix_clone`, `matrix_drop`
- `vector_clone`, `vector_drop`
- list allocation/method helpers; public list clone/drop helpers are not exposed
  yet

Before changing semantics, add a short implementation note describing which
types are heap handles, which drop/clone builtin currently owns them, and which
pointer/reference forms currently support `*p` without going through abilities.

Deliverables:

- [current-ownership-model.md](current-ownership-model.md)
- no compiler behavior changes
- no syntax changes

## Phase 1: lexer, parser, and AST surface

Status: complete.

Add the keywords:

```text
has
copy
clone
drop
deref
```

Add an AST representation:

```rust
enum Ability {
    Copy,
    Clone,
    Drop,
    Deref,
}
```

Attach ability lists to:

- `StructDef`
- `EnumDef`
- named vector declarations

The parser should accept declarations but the compiler can initially ignore the
abilities after parsing.

Examples:

```nia
struct A has drop {
    x: i32,
}

struct BoxI32 has deref {
    ptr: &i32,
}

enum E has copy, clone, drop {
    A,
    B(i32),
}

vector V i32 [X, Y] has copy, clone, drop
```

Deliverables:

- lexer tests for the new keywords
- parser tests for struct, enum, and vector `has` clauses
- AST pretty/debug tests if available
- no ownership behavior changes yet

## Phase 2: ability database in type checking

Status: complete.

Build an ability environment during type checking:

```text
TypeName -> AbilitySet
```

Add helper queries:

```rust
has_ability(ty, Ability::Copy)
has_ability(ty, Ability::Clone)
has_ability(ty, Ability::Drop)
has_ability(ty, Ability::Deref)
```

These helpers must understand:

- primitive types
- arrays
- fixed anonymous vectors
- heap anonymous vectors
- matrices
- lists
- structs
- enums
- named vectors
- function values
- future closure environment types

For built-in primitives, the helper should know the types exist from the
beginning, but must not report formal language-level `copy`, `clone`, `drop`, or
`deref` for them until the final primitive-integration phase. Existing scalar
by-value and pointer-dereference behavior can be preserved as separate
compatibility rules while the ability model is being introduced.

Validate declarations:

- `struct S has copy` requires every field to have `copy`, except for the
  temporary legacy-scalar carve-out before primitive integration
- `struct S has clone` requires every field to have `clone`, except for the
  temporary legacy-scalar carve-out before primitive integration
- early `struct S has drop` is valid if `S` has a custom `fn drop(self) ()`;
  derived field-by-field drop is enabled later, once aggregate drop exists
- early `struct S has deref` is valid if `S` has a custom
  `fn deref(&self) &Target`
- enum abilities require every payload field in every variant to support the
  ability, with the same temporary legacy-scalar carve-out; enum `deref` is
  rejected until there is an explicit enum-deref design
- named vector abilities require the axis element type to support the ability;
  named vectors over primitive scalar axes can remain accepted as a compatibility
  case until primitive abilities are formalized in the final integration phase;
  named vector `deref` is rejected in the first implementation

Deliverables:

- typechecker tests for accepted ability declarations
- typechecker tests for rejected impossible declarations
- diagnostics that name the missing ability and offending field/payload

## Phase 3: move checking for non-copy values

Status: complete.

Add a local ownership state during type checking:

```text
local name -> initialized | moved | maybe_initialized
```

Rules:

- using a moved local is an error
- assigning a non-copy local into another local moves it
- passing a non-copy value by value into a function moves it
- returning a non-copy local moves it out of the function
- reading a copy value does not move it

Example:

```nia
let a: f64<> = <1.0, 2.0>;
let b = a;      // move
println(a);     // error: use after move
```

The first version can be intentionally conservative:

- reject moves out of indexed values
- reject partial moves out of structs/enums
- reject move-sensitive behavior inside closures until captures are represented
  explicitly

Deliverables:

- use-after-move tests
- function argument move tests
- return move tests
- existing scalar by-value behavior remains backwards-compatible while formal
  primitive `copy` ability waits for the final integration phase

Implemented notes:

- the first move checker tracks local states as `available` / `moved`;
  `maybe_initialized` remains a future extension for uninitialized locals and
  finer control-flow merges
- `println` and `len` are checked as read-only builtin operations
- `&self` methods read their receiver; by-value methods move non-copy
  receivers
- partial moves out of struct fields and moves out of indexed values are
  rejected for now
- scalar primitives, strings, references, function values, quantum handles,
  matrices, heap vectors, lists, Complex, and scalar named vectors keep
  backwards-compatible copy-like move behavior until primitive ability
  integration

## Phase 4: explicit clone glue

Status: complete.

Add language-level clone glue behind method-call syntax only:

```nia
let b = a.clone();
```

Do not add a separate global `clone(x)` builtin. Clone is an ability-backed
method operation, so the surface form should stay attached to the value whose
ability is being used.

Type checking:

- `x.clone()` requires `x` to have `clone`
- result type is the same as `x`
- clone does not move `x`

Lowering:

- user-defined structs/enums/named vectors with `clone` -> generate clone
  lowering for the value
- arrays and fixed anonymous vectors clone when their element type already has
  language-level `clone`
- custom `fn clone(&self) OwnerType` overrides are deliberately left to Phase 5
- built-in primitives, including scalar primitives, `Matrix`, `T<>`, and
  `List[T]`, are not lowered through ability `clone` until the final
  primitive-integration phase

Keep old explicit builtins (`matrix_clone`, `vector_clone`) working during the
migration. They are low-level runtime functions, not language-level ability
clone yet.

Deliverables:

- typechecker tests for `x.clone()`
- codegen tests for generated clone lowering
- recursive struct/array clone tests that exclude primitive clone until the
  final integration phase
- tests that `matrix_value.clone()`, `heap_vector.clone()`, and primitive
  `1.clone()` are rejected before primitive integration

## Phase 5: custom struct clone methods

Status: complete.

Allow structs with `clone` ability to override their clone logic.

Preferred surface spelling:

```nia
struct Token has clone {
    id: i32,
}

impl Token {
    fn clone(&self) Token {
        Token {
            id: self.id,
        }
    }
}
```

Rules:

- only structs with `clone` may define `fn clone(&self) SelfType`
- `clone` takes `&self`, not `self`, because cloning must not move the source
  value
- return type must be exactly the owner struct type
- `clone` is called through `x.clone()`; there is no global `clone(x)`
- `clone` methods cannot be `copy`, `drop`, `extern`, or quantum functions
- moving fields out of `&self` is rejected
- recursive `self.clone()` calls inside the same custom clone method should be
  diagnosed if they would obviously recurse forever

Generated clone glue for a struct should be:

```text
if S has custom clone:
    result = call S::clone(&value)
else:
    result = clone fields recursively
```

Example:

```nia
let a = Token { id: 7 };
let b = a.clone();
```

Lower roughly as:

```text
b = Token::clone(&a)
```

This keeps `clone` symmetric with custom `drop`, but with one crucial
difference: `clone` borrows the source, while `drop` consumes it.

Deliverables:

- parser/typechecker support for recognizing `fn clone(&self) OwnerType`
- rejection tests for wrong signatures
- codegen tests showing `x.clone()` calls custom clone glue
- tests that `x.clone()` does not move `x`
- recursive fallback tests for structs without custom clone

Implemented notes:

- custom clone is currently supported for structs only
- custom clone overrides structural clone validation for `clone`; `copy` still
  requires structurally copyable fields
- a direct `self.clone()` inside the custom clone body is rejected as obvious
  recursion

## Phase 6: custom struct deref methods

Status: complete.

Allow structs with `deref` ability to override dereference logic.

Preferred surface spelling:

```nia
struct BoxI32 has deref, drop {
    ptr: &i32,
}

impl BoxI32 {
    fn deref(&self) &i32 {
        self.ptr
    }
}
```

Rules:

- only structs with `deref` may define `fn deref(&self) &Target`
- `deref` takes `&self`, not `self`, because dereferencing must not move the
  owner
- return type must be a reference/pointer type in the first implementation
- `*x` should lower through deref glue when `x` has `deref`
- `deref(x)` can exist as an explicit builtin form if that is easier to stage,
  but `*x` is the desired user-facing operation
- deref does not imply `copy`, `clone`, or `drop`
- deref does not allow moving the target out through `&self`
- implicit deref coercions, method forwarding, and chained auto-deref should be
  postponed until explicit deref is stable
- `deref` methods cannot be `copy`, `clone`, `drop`, `extern`, or quantum
  functions

Generated deref glue for a struct should be:

```text
if S has custom deref:
    target_ptr = call S::deref(&value)
    result = load target_ptr only when the source expression asks for a value
```

Example:

```nia
let x = *boxed;
```

Lower roughly as:

```text
target = BoxI32::deref(&boxed)
x = *target
```

Deliverables:

- parser/typechecker support for recognizing `fn deref(&self) &Target`
- rejection tests for wrong signatures
- codegen tests showing `*x` calls custom deref glue
- tests that deref does not move `x`
- tests that `deref` does not grant `copy`, `clone`, or `drop`
- diagnostics for implicit auto-deref forms if they are not supported yet

Implemented notes:

- custom deref is currently supported for structs only
- `*x` calls `S::deref(&x)` when `x` has `deref`; raw pointer dereference keeps
  its existing behavior
- direct `.deref()` calls are rejected; use `*x`
- deref borrows the owner and does not move it
- implicit deref coercions, method forwarding, and chained auto-deref are still
  future work

## Phase 7: custom struct drop methods

Status: complete.

Allow structs with `drop` ability to override their destructor logic.

Preferred surface spelling:

```nia
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) () {
        close_fd(self.fd);
    }
}
```

Rules:

- only structs with `drop` may define `fn drop(self) ()`
- `drop` takes `self` by value and returns `()`
- in the first drop-focused milestone, a user-defined struct should get
  language-level `drop` only through a custom `fn drop(self) ()`; automatic
  recursive field drop comes later
- `drop` cannot be called directly as a normal method in the first
  implementation; use `drop(x)` to request destruction
- `drop(self)` runs at most once for each initialized value
- moving fields out of `self` inside `drop` is rejected in the first
  implementation
- `drop` methods cannot be `copy`, `clone`, `extern`, or quantum functions
- panics/unwinding are out of scope until the language has an exception model
- existing low-level cleanup functions such as `matrix_drop` and `vector_drop`
  may still be called manually inside a custom destructor

Early generated drop glue for a custom-drop struct should be:

```text
call S::drop(self)
```

Field cleanup policy is staged:

1. Early custom-drop phase: manual cleanup. `drop(self)` is responsible for any
   non-trivial field cleanup it wants to perform.
2. Later aggregate-drop phase: for fields that have language-level `drop`, the
   compiler can add Rust-like field cleanup after `drop(self)`.
3. Final primitive-integration phase: built-in primitives receive their formal
   language-level `copy`, `clone`, and `drop` rules. Runtime handles such as
   `Matrix`, `T<>`, and `List[T]` receive `clone`/`drop`, so custom destructors
   can stop calling low-level `matrix_drop` / `vector_drop` directly when
   appropriate.

This ordering intentionally lets custom destructors land before primitive
`copy`, primitive `clone`, or matrix/vector/list drop are part of the ability
system.

Example lowering:

```nia
let h = FileHandle { fd: 3 };
```

At scope exit:

```text
call FileHandle::drop(h)
```

Deliverables:

- parser/typechecker support for recognizing `fn drop(self) ()`
- rejection tests for wrong signatures
- codegen tests showing custom drop glue can call the destructor function
- tests that low-level runtime cleanup functions can still be used manually
  inside custom destructors
- diagnostics for direct `value.drop()` calls if they are not supported yet

Implemented notes:

- custom drop is currently supported for structs only
- a struct that defines `fn drop(self)` must declare `has drop`
- direct `.drop()` calls are rejected; use the language-level `drop(x)` form
- moving non-copy fields out of `self` inside `drop` is rejected by the existing
  partial-move diagnostics
- low-level cleanup helpers such as `matrix_drop` and `vector_drop` remain
  callable inside custom destructors

## Phase 8: explicit drop for custom-drop structs

Status: complete.

Add a language-level explicit drop operation for user-defined custom-drop
structs:

```nia
drop(v);
```

Type checking:

- `drop(x)` requires `x` to have `drop`
- `drop(x)` moves `x`
- using `x` after `drop(x)` is an error
- in this phase, `drop(x)` is accepted for custom-drop structs only
- `drop(primitive_scalar)`, `drop(matrix_value)`, `drop(heap_vector)`, and
  `drop(list)` are rejected until the final primitive-integration phase

Lowering:

- custom-drop struct -> call its `S::drop(self)` glue

Keep old explicit builtins (`matrix_drop`, `vector_drop`) working during the
migration. They are low-level runtime functions, not language-level ability
drop yet.

Deliverables:

- use-after-drop tests
- codegen tests showing `drop(x)` calls custom struct drop
- tests that matrix/vector/list values are still rejected by language-level
  `drop(x)` in this phase

Implemented notes:

- `drop(x)` is parsed as a language-level operation, not as a normal user
  function
- `drop(x)` is accepted only for custom-drop structs and moves `x`
- using a local after `drop(local)` is rejected by move checking
- `drop(primitive)`, `drop(matrix)`, `drop(heap_vector)`, and `drop(list)` are
  rejected until the final primitive-integration phase
- existing `matrix_drop` and `vector_drop` builtins are unchanged

## Phase 9: automatic drop at lexical scope exit for custom-drop structs

Status: complete.

Insert drop glue automatically for live custom-drop locals:

```nia
fn main() i32 {
    let h = FileHandle { fd: 3 };
    println(1);
    0
}
```

Lower roughly as:

```text
h = FileHandle { fd: 3 }
println(1)
FileHandle::drop(h)
return 0
```

Automatic drop must run before:

- normal block exit
- `return`
- `break`
- `continue`
- loop scope exit
- match/if branch exit

Drop order should be reverse declaration order within a scope, matching Rust's
practical intuition.

Runtime primitives are intentionally excluded from auto-drop in this phase.

Deliverables:

- codegen tests that generated LLVM includes custom drop calls
- tests for early `return`
- tests for `break` and `continue`
- tests that heap vectors and matrices still require their existing manual
  cleanup functions

Implemented notes:

- custom-drop locals are automatically dropped in reverse declaration order on
  normal function/block exit
- early `return` drops all currently live custom-drop locals before returning
- `break` from `loop` drops locals declared inside the loop body before
  branching to the loop exit
- `if`, `while`, `loop`, `for`, `quant`, and `gpu` statement bodies clean up
  locals declared inside the body on normal block exit
- `continue` is still future work because Nia does not currently expose a
  `continue` statement
- runtime primitives such as `Matrix`, `T<>`, and `List[T]` are still excluded
  from language-level auto-drop

## Phase 10: drop flags and maybe-initialized custom-drop locals

Status: complete.

Support conditional initialization:

```nia
let h: FileHandle;

if cond {
    h = FileHandle { fd: 3 };
}
```

The compiler needs a hidden drop flag:

```text
h_init = false
if cond {
    h = ...
    h_init = true
}
if h_init {
    drop(h)
}
```

Also handle overwrites:

```nia
let h = FileHandle { fd: 3 };
h = FileHandle { fd: 4 }; // old h must be dropped before overwrite
```

Deliverables:

- maybe-initialized diagnostics
- conditional custom-drop codegen tests
- overwrite custom-drop tests

Implemented notes:

- `let x: T;` is accepted for typed locals and starts as uninitialized
- reading an uninitialized local is rejected
- after conditional initialization, reading the local is rejected as
  maybe-initialized
- custom-drop locals carry a hidden `i1` drop flag in codegen
- assignments to custom-drop locals set the flag after storing the new value
- overwriting a live custom-drop local conditionally drops the old value first
- moving or explicitly dropping a custom-drop local clears its flag so automatic
  cleanup does not double-drop it

## Phase 11: structs, enums, aggregate drop, and partial initialization

Status: complete.

Once custom auto-drop works for simple locals, extend drop glue to aggregate
user-defined values whose fields or payloads already have language-level
`drop`.

Rules:

- derived struct drop is allowed only when every non-trivial field has `drop`
- struct drop drops fields in declaration order or reverse declaration order;
  choose one and document it
- enum drop switches on the active tag and drops the active payload only
- partial moves are rejected in the first implementation
- custom `drop(self)` may run before compiler-generated field cleanup, but only
  for fields whose types already have language-level `drop`
- fields of `Matrix`, `T<>`, and `List[T]` are not recursively dropped until the
  final primitive-integration phase
- derived clone for aggregates containing built-in primitives also waits for
  the final primitive-integration phase unless the struct provides a custom
  `fn clone(&self) Self`

Example rejected initially:

```nia
let p = Pair { a: handle_a, b: handle_b };
let only_a = p.a; // partial move rejected for now
```

Deliverables:

- struct drop glue tests for user-defined fields
- enum drop glue tests
- diagnostics for unsupported partial moves
- tests that aggregate drop does not silently drop runtime primitives before
  they are integrated

Implemented notes:

- structs can derive `drop` when every field is either trivial under the current
  scalar compatibility carve-out or already has language-level `drop`
- struct fields are dropped in reverse declaration order
- enum drop switches on the active tag and drops only the active payload
- custom `drop(self)` runs first, then compiler-generated cleanup drops any
  fields with language-level `drop`
- `Matrix`, `T<>`, `List[T]`, fixed arrays, fixed anonymous vectors, and named
  vectors remain outside language-level drop integration until the final
  primitive/runtime phase
- partial moves out of structs remain rejected

## Phase 12: closures and captured environments

Status: complete for copy-safe value captures.

This phase connects the existing `fn(...) -> ...` function value surface to
closure literals that capture ordinary copy-safe values from the surrounding
scope.

Non-capturing closures:

```text
use env = null
```

Capturing closures:

```text
capture copy-safe values by value
lower as { code_ptr, env_ptr }
```

Runtime primitive captures such as `Matrix` and `T<>`, plus custom-drop and
other non-copy resources, are handled by the later explicit `move || ...`
closure work in Phase 14.

Deliverables:

- capture analysis integrated with move checking for copy-safe captures
- function values initially lowered as fat values `{ code_ptr, env_ptr }`;
  Phase 14 extends this to `{ code_ptr, env_ptr, drop_ptr }`
- wrappers for top-level functions used as values
- tests for non-capturing closures, capturing closures, nested captures,
  top-level functions as values, and unit-returning function values
- sample program for copy-safe captures

Implemented notes:

- closure bodies can capture copy-safe outer values by value
- nested closure capture analysis is transitive, so outer closure environments
  contain values required by inner closures
- function values lower as a fat value containing code, env, and optional env
  drop pointers
- non-capturing closures and top-level functions use null env/drop pointers
- captured `Matrix`, `T<>`, `List[T]`, custom-drop resources, and other
  non-copy values use the later Phase 14 `move || ...` form
- assignments to captured variables are rejected for now
- closure env clone is intentionally not exposed as language-level `clone`
  until Phase 15 defines formal function-value abilities

## Phase 13: primitive ability integration

Status: complete for scalar/fixed aggregate/runtime `clone` and `drop`;
function-value abilities and pointer ownership remain future phases.

After custom clone/deref/drop methods, auto-drop, drop flags, aggregate drop,
and copy-safe closure captures are working, add formal language-level abilities
to built-in primitives and runtime handles.

This phase intentionally excludes `fn(...) -> ...` values and closure
environments. Function-value abilities wait until closure environment ownership
is explicit.

This phase grants `copy` / `clone` / `drop` / `deref` where appropriate:

- scalar primitives receive formal `copy, clone, drop`
- raw pointers / references receive formal `deref`; their `copy`, `clone`, and
  `drop` rules depend on the pointer ownership model chosen for `alloc`,
  `realloc`, and `dealloc`
- fixed-size arrays receive `copy` / `clone` / `drop` ability `A` when their
  element type has `A`
- fixed-size anonymous vectors receive `copy` / `clone` / `drop` ability `A`
  when their element type has `A`
- `Matrix` receives `clone, drop`, but not `copy`
- heap anonymous vectors `T<>` receive `clone, drop`, but not `copy`
- dynamic lists `List[T]` receive `clone` / `drop` when their element type
  supports the same ability, but not `copy`
- future built-in smart pointer handles may receive `deref` here if they expose
  a well-defined target type
- any future runtime handle receives explicit ability
  rules here

Lowering:

- scalar primitive `clone` -> identity
- raw pointer/reference deref -> existing pointer load/store lowering
- array/vector primitive `clone` -> element-wise clone when needed
- `Matrix` -> `matrix_drop`
- `T<>` -> `vector_drop`
- `List[T]` -> list drop glue
- `Matrix` clone -> `matrix_clone`
- `T<>` clone -> `vector_clone`
- `List[T]` clone -> list clone glue
- arrays/structs/enums containing these types may now derive recursive drop
- arrays/structs/enums containing these types may now derive recursive clone
- custom `drop(self)` methods can rely on compiler-generated field cleanup for
  these fields after the custom destructor runs

The existing low-level functions remain available for compatibility, but normal
user code should move toward:

```nia
drop(x);
```

and automatic scope cleanup.

Deliverables:

- primitive scalar `copy` / `clone` / `drop` tests
- raw pointer/reference `deref` tests
- fixed array and fixed vector derived ability tests
- `matrix_value.clone()` tests
- `heap_vector.clone()` tests
- `list.clone()` tests
- `drop(matrix_value)` and auto-drop tests
- `drop(heap_vector)` and auto-drop tests
- `drop(list)` and auto-drop tests
- aggregate clone/drop tests where structs contain matrices, heap vectors, and
  lists
- overwrite and conditional-initialization tests for runtime primitives
- README update that marks primitive abilities as stable

Implemented notes:

- scalar primitives support formal `copy`, `clone`, and no-op `drop`
- fixed arrays and fixed anonymous vectors derive `copy`, `clone`, and `drop`
  from their element type
- `Matrix` and heap anonymous vectors `T<>` support `clone` by allocating
  independent storage and `drop` through runtime cleanup
- `List[T]` supports `clone` / `drop` when `T` supports the same ability;
  list clone allocates a new list and clones elements
- structs, vectors, and enums can derive recursive clone/drop through runtime
  primitive fields
- `Matrix`, `T<>`, and `List[T]` are no longer treated as copy-like by move
  checking; use `.clone()` to duplicate them
- old low-level `matrix_clone`, `matrix_drop`, `vector_clone`, and
  `vector_drop` calls remain available and clear auto-drop state for
  compatibility

## Phase 14: closure move captures and environment cleanup

Status: complete for explicit `move` captures and environment drop cleanup.

This phase contains the remaining closure work that was intentionally left out
of the Phase 12 MVP. It now runs after primitive ability integration, so closure
environments can reuse the formal `clone`/`drop` rules for `Matrix`, `T<>`,
`List[T]`, fixed arrays, fixed vectors, and user-defined aggregates.

Capturing closures:

```text
copy: no
clone: not exposed yet
drop: yes, via generated closure environment drop glue
```

Closure environments are currently heap allocated and owned by the function
value. The lowered function value carries `{ code, env, drop }`, so moving a
function value transfers the environment cleanup responsibility. Non-capturing
closures and top-level function values use null `env`/`drop` pointers.

Move closures should move captured non-copy values into the environment:

```nia
let h = FileHandle { fd: 3 };
let f = move || println(h.fd);
println(h.fd); // error: moved into closure
```

Captured non-copy values are read-only inside the closure body for now. This
keeps the existing `fn(...) -> ...` call surface repeatable until a later
`FnOnce`-style model exists.

Deliverables:

- `move || ...` syntax and parser tests
- move-checking that marks non-copy captures as moved into the closure
- closure env drop glue through generated `__nia_closure_N_drop` functions
- capture drop tests for custom-drop structs
- diagnostics for use-after-move caused by move captures
- diagnostics for attempts to move captured env values out of repeatable
  closures

Open design questions:

- whether non-`move` closures should ever capture by reference
- whether closure environments should become cloneable through explicit
  function-value `clone`
- whether a later `FnOnce`-style type should allow consuming captured env values

## Phase 15: function-value abilities

Status: complete for value-level copy/clone/drop behavior.

After closure environment ownership is explicit, assign formal abilities to the
surface `fn(...) -> ...` type.

Non-capturing function values:

```text
copy, clone, drop
```

Capturing function values:

```text
copy: no
clone: if the environment is cloneable
drop: yes
```

This phase should prevent shallow-copying an environment pointer while still
allowing ordinary top-level functions and non-capturing closures to behave like
cheap function pointers.

Implementation notes:

- the runtime function value is `{ code, env, drop, clone }`
- top-level function values and non-capturing closures use null env/drop/clone
  pointers and are treated as copyable plain function pointers by move checking
- capturing closures are move-only by default
- capturing closures get generated `__nia_closure_N_clone` glue when every
  captured value is cloneable
- `fn(...) -> ...` supports language-level `drop` as a type
- structural `copy`/`clone` for aggregates containing `fn(...) -> ...` remains
  conservative because the type alone does not encode whether a particular
  function value has an environment

Deliverables:

- value-level typechecker/move-checker rules for `copy`, `clone`, and `drop`
  on function values
- clone/drop lowering for capturing function values
- copy tests for top-level function values and non-capturing closures
- rejection tests for copying capturing closure values
- tests for cloneable and non-cloneable captured environments

## Phase 16: deprecate low-level manual RC builtins in user docs

Status: complete.

Keep these builtins available for internal lowering and compatibility:

- `matrix_clone`
- `matrix_drop`
- `vector_clone`
- `vector_drop`

After primitive ability integration is stable, user-facing docs should prefer:

```nia
x.clone()
drop(x)
```

and automatic scope cleanup.

Deliverables:

- README ownership section updated to prefer `.clone()` / `drop(x)`
- user-facing examples rewritten to rely on abilities where appropriate
- low-level manual-RC examples kept only as compatibility/runtime docs

## Phase 17: diagnostics

Status: complete.

Abilities affect common code paths, so diagnostics matter.

Good errors should say:

- which value was moved
- where it was moved
- why the type is not `copy`
- which ability is missing
- which field or enum variant prevents deriving an ability

Implemented notes:

- move diagnostics keep the old `use of moved local` prefix, but now include the
  previous move category such as by-value argument passing, binding into another
  local, explicit `drop(...)`, return, or `move` closure capture
- moved-local diagnostics explain why the local's type is not `copy`
- declared ability validation now reports the missing ability reason for the
  offending struct field, enum variant payload, or vector element
- closure capture diagnostics explain why a non-`move` capture lacks `copy`, or
  why a `move ||` capture is not eligible for copy/drop/function ownership

## Suggested implementation order

1. Parse `has copy, clone, drop, deref`.
2. Add `AbilitySet` and typechecker queries.
3. Validate declared abilities structurally.
4. Add move checking for non-copy locals. (complete)
5. Add ability-backed `x.clone()` method calls.
6. Add custom struct clone method validation and lowering.
7. Add custom struct deref methods and explicit `*x` lowering through deref
   glue.
8. Add custom struct drop methods.
9. Add explicit `drop(x)` for custom-drop structs.
10. Add automatic custom-drop at simple scope exits.
11. Add early-return/break/continue custom-drop insertion.
12. Add drop flags for custom-drop locals.
13. Add aggregate drop glue for user-defined types.
14. Connect copy-safe closure captures to abilities. (complete)
15. Integrate `copy`, `clone`, `drop`, and `deref` for built-in primitives and
    runtime handles: scalars, raw pointers/references, fixed arrays/vectors,
    `Matrix`, `T<>`, `List[T]`.
16. Add closure move captures and closure environment drop cleanup. (complete)
17. Add formal `copy`, `clone`, and `drop` rules for `fn(...) -> ...` values.
    (complete)
18. Update docs and examples. (complete)
19. Improve diagnostics. (complete)

The key design rule is simple: codegen should not guess ownership or access
semantics. Ownership, moves, clone permissions, deref permissions, and drop
obligations should be decided by typed semantic analysis before LLVM lowering.
