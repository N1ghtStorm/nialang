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

For reference-counted runtime values, clone glue increments the reference count.
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

Current Nia has reference-counted heap handles, but lifetime management is still
explicit in many places:

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

## Phase 6: custom struct deref methods

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

## Phase 7: custom struct drop methods

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

## Phase 8: explicit drop for custom-drop structs

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

## Phase 9: automatic drop at lexical scope exit for custom-drop structs

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

## Phase 10: drop flags and maybe-initialized custom-drop locals

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

## Phase 11: structs, enums, aggregate drop, and partial initialization

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

## Phase 12: ability constraints for generics

When Nia grows more generic syntax, add ability bounds:

```nia
fn dup[T: clone](x: T) T {
    x.clone()
}

fn ignore[T: drop](x: T) () {
    drop(x)
}

fn read_ref[T: deref](x: T) () {
    println(*x)
}
```

Possible spelling alternatives:

```nia
fn dup[T has clone](x: T) T
fn dup[T: clone](x: T) T
fn read_ref[T: deref](x: T) ()
```

Do not block the non-generic MVP on this phase.

Deliverables:

- parser support for ability bounds
- typechecker support for generic ability obligations
- diagnostics for missing bounds

## Phase 13: closures and captured environments

Abilities become important for closure captures.

Non-capturing closures:

```text
eventually copy, clone, drop after primitive/function-pointer ability integration
```

Capturing closures:

```text
copy: initially no
clone: if captured environment has clone
drop: if captured environment has drop
```

If closure environments are heap allocated and reference-counted, they should be
`clone, drop` but not `copy`.

Move closures should move captured non-copy values into the environment:

```nia
let h = FileHandle { fd: 3 };
let f = move || println(h.fd);
println(h.fd); // error: moved into closure
```

Runtime primitive captures such as `Matrix` and `T<>` should remain limited
until those primitives receive formal language-level `copy`, `clone`, and
`drop` rules in the final integration phase.

Deliverables:

- capture analysis integrated with move checking
- closure env drop glue
- closure clone glue if closure values are cloneable

## Phase 14: diagnostics and migration mode

Abilities affect common code paths, so diagnostics matter.

Good errors should say:

- which value was moved
- where it was moved
- why the type is not `copy`
- which ability is missing
- which field or enum variant prevents deriving an ability

During migration, consider a temporary compatibility mode:

- existing structs/enums without `has` keep current behavior if all fields are
  trivially copy/drop
- compiler warns that explicit `has` will be required later

Once examples and tests are migrated, strict mode can require explicit abilities
for user-defined resource-like types.

## Phase 15: primitive ability integration

Only after custom clone/deref/drop methods, auto-drop, drop flags, aggregate
drop, and closure environment cleanup are working, add formal language-level
abilities to built-in primitives.

This phase grants `copy` / `clone` / `drop` / `deref` where appropriate:

- scalar primitives receive formal `copy, clone, drop`
- raw pointers / references receive formal `deref`; their `copy`, `clone`, and
  `drop` rules depend on the pointer ownership model chosen for `alloc`,
  `realloc`, and `dealloc`
- fixed-size arrays receive `copy` / `clone` / `drop` ability `A` when their
  element type has `A`
- fixed-size anonymous vectors receive `copy` / `clone` / `drop` ability `A`
  when their element type has `A`
- non-capturing function pointers receive formal `copy, clone, drop`
- `Matrix` receives `clone, drop`, but not `copy`
- heap anonymous vectors `T<>` receive `clone, drop`, but not `copy`
- dynamic lists `List[T]` receive `clone` / `drop` according to their element
  ability constraints, but not `copy`
- future built-in smart pointer handles may receive `deref` here if they expose
  a well-defined target type
- any other future reference-counted runtime handle receives explicit ability
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
- closure capture drop tests for runtime primitives
- README update that marks primitive abilities as stable

## Phase 16: deprecate low-level manual RC builtins in user docs

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

- README ownership section update
- examples rewritten to rely on auto-drop where appropriate
- old manual-drop examples moved to low-level/runtime docs

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
14. Connect closure captures to abilities.
15. Integrate `copy`, `clone`, `drop`, and `deref` for built-in primitives and
    runtime handles: scalars, raw pointers/references, fixed arrays/vectors,
    `Matrix`, `T<>`, `List[T]`.
16. Update docs and examples.

The key design rule is simple: codegen should not guess ownership or access
semantics. Ownership, moves, clone permissions, deref permissions, and drop
obligations should be decided by typed semantic analysis before LLVM lowering.
