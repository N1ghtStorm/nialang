# Concurrency Plan: Send, Sync, Arc, Mutex, RwLock, Condvar, and Threads

Status: Phases 0-3 are implemented. Phase 4 is next.

Depends on:

- [atomics-plan.md](atomics-plan.md) — CPU atomics (Phases 0–5 implemented).
- [abilities-plan.md](abilities-plan.md) — ownership, `move ||` closures, function-value drop/clone.

## Goal

Add a practical multi-threading toolkit to Nia:

- formal `send` and `sync` ability rules enforced at compile time;
- `Arc[T]` — atomic reference-counted shared ownership across threads;
- `Mutex[T]` — mutual exclusion with scoped lock guards;
- `RwLock[T]` — many readers / one writer;
- `Condvar` — condition variables paired with `Mutex` guards;
- extended `spawn` so threads can run closures that **move** `send` captures and shared `sync` state.

Nia does not have user-level generics. Like `List[T]` and `AtomicPtr[T]`, sync primitives are
compiler-known type constructors:

```nia
Arc[i32]
Mutex[Counter]
RwLock[State]
```

`Condvar` is a single built-in type with no type parameter.

## Current State

Threads today ([`examples/sample_threads.nia`](../examples/sample_threads.nia)):

```nia
fn worker() {
    println(1);
}

let t: Thread = spawn worker;
join(t);
```

Constraints in the compiler today:

- `spawn` accepts only a **top-level** function name.
- the target must have type `fn() -> ()`.
- no closure threads, no moved payload, no `send` checking.
- `Thread` is move-only (`drop` detaches via `pthread_detach`, `join` consumes the handle).
- lowering uses `pthread_create` / `pthread_join` / `pthread_detach` and
  `@nialang.thread.entry` ([`src/nia_std/mod.rs`](../src/nia_std/mod.rs)).

`send` and `sync` are parsed and can be declared on structs/enums/vectors, but they are **not
enforced** for cross-thread operations yet. Struct validation intentionally skips structural
checks for `send`/`sync` ([`src/semantics/typecheck/mod.rs`](../src/semantics/typecheck/mod.rs)).

Atomics are implemented; the atomics plan lists thread/shared-ownership APIs as follow-up work.

## Design Principles

- No generic `SyncPrimitive[T]`. Only the concrete built-in forms above.
- Cross-thread boundaries are compile-time checked via `send` / `sync`.
- Runtime primitives wrap **pthread** on native targets (`-pthread` already linked in the driver).
- Poisoning (Rust-style `PoisonError`) is **out of scope** until Nia has unwinding or a panic model.
- Prefer RAII guards (`MutexGuard`, `RwLockReadGuard`, `RwLockWriteGuard`) over manual unlock calls.
- `Arc::clone` bumps a reference count; it does **not** deep-clone `T`.
- QIR / GPU shared memory is out of scope.
- Channels (`mpsc`, `select`) are a separate future plan; this document covers mutex-based sharing
  and OS-thread spawning only.

## Send and Sync

### Target Semantics

Mirror Rust’s auto-trait intuition, adapted to Nia’s explicit `has` syntax.

**`send`** — a value of this type may be transferred to another thread by move (ownership crosses
the thread boundary).

**`sync`** — a value of this type may be **shared** across threads through immutable references
(`&T`, `Arc[T]`, `&Mutex[T]`, atomic cells, etc.).

### Default Rules (auto traits)

A type **has `send`** unless opted out or a field blocks it:

| Type | `send` |
| --- | --- |
| scalars, `()`, `bool`, `string` | yes |
| `&T` | yes if `T: sync` |
| `&mut T` | yes if `T: sync` |
| `[T; N]`, `T<N>`, `T<>` | yes if `T: send` |
| `List[T]`, `Matrix`, `Option`, `Result` | yes if element/payload types are `send` |
| `fn(...) -> ...` (non-capturing) | yes |
| capturing `fn(...) -> ...` | yes if every captured value is `send` |
| `Atomic*` | yes |
| `Arc[T]` | yes if `T: send + sync` |
| `Mutex[T]`, `RwLock[T]` | yes if `T: send` |
| `Condvar` | yes |
| `Thread` | **no** |
| `qubit`, `result` | **no** |
| raw `alloc` pointers without lifetime story | **no** (conservative MVP) |

A type **has `sync`** unless opted out or a field blocks it:

| Type | `sync` |
| --- | --- |
| scalars, `()`, `bool`, `string` | yes |
| `&T` | yes if `T: sync` |
| `&mut T` | **no** |
| `[T; N]`, `T<N>`, `T<>` | yes if `T: sync` |
| `List[T]`, `Matrix`, `Option`, `Result` | yes if element/payload types are `sync` |
| non-capturing `fn(...) -> ...` | yes |
| capturing `fn(...) -> ...` | yes if every captured value is `sync` |
| `Atomic*` | yes |
| `Arc[T]` | yes if `T: send + sync` |
| `Mutex[T]`, `RwLock[T]` | yes if `T: send` |
| `Condvar` | yes |
| `Thread` | **no** |
| `qubit`, `result` | **no** |
| interior mutability without sync wrapper | **no** |

### Manual Opt-Out / Opt-In on User Types

User structs, enums, and named vectors keep the existing `has send, sync` surface:

```nia
struct NotSend has drop {
    rc: i32,
}

struct SyncCounter has sync, drop {
    value: AtomicI32,
}
```

Rules:

- if a type declares `send`, the compiler checks that every field/payload is `send` (same structural
  style as `drop`, not the current no-op).
- if a type declares `sync`, every field/payload must be `sync`.
- a type may omit `send`/`sync` from `has` and still receive them automatically when the auto rules
  say yes.
- a type may **withhold** an auto trait only by not declaring it **and** having a field that lacks
  it; there is no `!Send` syntax in MVP.
- `copy` types are always `send + sync`.

### Enforcement Sites

The typechecker must reject non-`send` values moved into:

- `spawn` closure captures (`move || ...`);
- `Arc::new(value)` when the arc may escape to another thread (always, for `Arc`);
- returning a value from a thread entry function once thread payloads are supported.

The typechecker must reject non-`sync` values behind:

- `Arc::new` inner type (needs `T: send + sync`);
- `static` / future global shared state (reserved);
- atomic loads that publish `&T` to other threads (already restricted for `AtomicPtr[T]`).

Diagnostics should name the offending field or capture, similar to existing move/ability errors.

## Source Surface

### Arc

```nia
let shared: Arc[i32] = arc_new(42);
let shared2 = shared.clone();   // refcount +1, shallow
let n = *shared;                // read through built-in deref

drop(shared);                   // refcount -1
drop(shared2);                  // destroys inner i32 when count hits 0
```

Methods:

| Method | Signature | Notes |
| --- | --- | --- |
| `arc_new` | `fn arc_new[T](value: T) -> Arc[T]` | requires `T: send + sync` |
| `.clone` | `fn clone(&self) -> Arc[T]` | refcount increment |
| deref | `*arc` | requires `Arc[T]: deref` built-in |

`Arc[T]` is not `copy`. It is `send` when `T: send + sync`, and `sync` under the same bound.

### Mutex

```nia
let m: Mutex[Counter] = mutex_new(Counter { hits: 0 });

let guard = m.lock();
guard.hits = guard.hits + 1;
drop(guard);                    // unlock

// or rely on scope exit auto-drop of MutexGuard
```

Methods:

| Method | Signature | Notes |
| --- | --- | --- |
| `mutex_new` | `fn mutex_new[T](value: T) -> Mutex[T]` | requires `T: send` |
| `.lock` | `fn lock(&self) -> MutexGuard[T]` | blocks; no poisoning in MVP |
| `.try_lock` | `fn try_lock(&self) -> Option[MutexGuard[T]]` | non-blocking |

`MutexGuard[T]`:

- move-only RAII token;
- has `deref` → `&T`;
- has `deref_mut` is **not** in MVP — use explicit pattern later or only expose `&T` first;
- dropping the guard unlocks;
- guard is **not** `send` (must not escape to another thread while held).

For interior mutation in MVP, store `Mutex[CellLike]` where `CellLike` is a struct users mutate
through `&T`, or add `MutexGuard::get_mut(&mut self) -> &mut T` as a method in Phase 2.

Recommended MVP for mutating primitives:

```nia
let m: Mutex[i32] = mutex_new(0);
let mut guard = m.lock();
*guard = *guard + 1;
```

This requires `MutexGuard` to support mutable deref (`&mut T`) when the guard binding is `mut`.
Start with `&mut T` through `mut guard` only.

### RwLock

```nia
let rw: RwLock[Config] = rwlock_new(Config { port: 8080 });

let r = rw.read();
println(r.port);
drop(r);

let mut w = rw.write();
w.port = 9090;
drop(w);
```

Methods:

| Method | Signature |
| --- | --- |
| `rwlock_new` | `fn rwlock_new[T](value: T) -> RwLock[T]` |
| `.read` | `fn read(&self) -> RwLockReadGuard[T]` |
| `.write` | `fn write(&self) -> RwLockWriteGuard[T]` |
| `.try_read` | `fn try_read(&self) -> Option[RwLockReadGuard[T]]` |
| `.try_write` | `fn try_write(&self) -> Option[RwLockWriteGuard[T]]` |

Guards follow the same RAII rules as `MutexGuard`. Read guard exposes shared `&T`; write guard
exposes `&mut T` through `mut` binding.

### Condvar

```nia
let m: Mutex[Queue] = mutex_new(queue);
let cv: Condvar = condvar_new();

// waiter
let mut guard = m.lock();
while !guard.ready {
    cv.wait(&guard);
}

// notifier
let mut guard = m.lock();
guard.ready = true;
cv.notify_one();
drop(guard);
```

Methods:

| Method | Signature |
| --- | --- |
| `condvar_new` | `fn condvar_new() -> Condvar` |
| `.wait` | `fn wait(&self, guard: MutexGuard[T]) -> MutexGuard[T]` |
| `.notify_one` | `fn notify_one(&self) -> ()` |
| `.notify_all` | `fn notify_all(&self) -> ()` |

`wait` atomically releases the mutex and blocks; on wakeup it re-acquires and returns the guard.
Spurious wakeups are allowed — callers should loop on a predicate, as in the example.

`Condvar` is `send + sync`. It is not tied to a specific `Mutex` at the type level (pthread model).

### Extended Threads

Keep the existing top-level spawn form and add closure spawn:

```nia
let data: Arc[Mutex[i32]] = arc_new(mutex_new(0));
let t: Thread = spawn move || {
    let mut guard = data.lock();
    *guard = *guard + 1;
};
join(t);
```

Surface:

```nia
let t: Thread = spawn worker;              // unchanged: top-level fn() -> ()
let t: Thread = spawn move || { ... };     // new: move closure, must be fn() -> ()
```

Rules:

- closure type must be `fn() -> ()` (unit-returning, no arguments).
- non-`move` closure capture in `spawn` is rejected — thread entry always takes ownership of the
  environment.
- every captured value must be `send`.
- the closure environment is dropped on the new thread after the body returns (reuse existing
  `{ code, env, drop, clone }` function-value lowering).
- `spawn` returns `Thread` as today; `join(t)` waits and frees the handle.

Optional later extension (not MVP):

```nia
spawn(move || ..., stack_size: i32)
```

## Type Rules Summary

| Type | `copy` | `clone` | `drop` | `send` | `sync` |
| --- | --- | --- | --- | --- | --- |
| `Arc[T]` | no | yes (shallow) | yes | if `T: send+sync` | if `T: send+sync` |
| `Mutex[T]` | no | no | yes | if `T: send` | if `T: send` |
| `RwLock[T]` | no | no | yes | if `T: send` | if `T: send` |
| `MutexGuard[T]` | no | no | yes | no | no |
| `RwLockReadGuard[T]` | no | no | yes | no | no |
| `RwLockWriteGuard[T]` | no | no | yes | no | no |
| `Condvar` | no | no | yes | yes | yes |
| `Thread` | no | no | yes | no | no |

Quantum types (`qubit`, `result`) cannot appear inside `Arc`, `Mutex`, `RwLock`, or thread captures.

## Runtime Layout (LLVM / pthread)

All sync objects are heap-allocated opaque handles (pointer-sized) unless noted.

### Arc[T]

```text
ArcInner[T] {
    refcount: AtomicUsize,   // strong count
    value: T,
}
Arc[T] handle -> ptr to ArcInner[T]
```

- `arc_new` allocates `ArcInner`, stores `value`, initializes refcount to 1.
- `clone` atomic-increments refcount.
- `drop` atomic-decrements; when previous count was 1, drop `T` and free `ArcInner`.
- use `AtomicUsize` with `Ordering::Relaxed` for refcount, `Ordering::Acquire` on last decrement
  before destroying `T`, `Ordering::Release` when publishing a new `Arc`.

### Mutex[T]

```text
MutexInner[T] {
    lock: pthread_mutex_t,
    value: T,
}
Mutex[T] handle -> ptr to MutexInner[T]
```

- `mutex_new` initializes mutex, stores `value`.
- `lock` calls `pthread_mutex_lock`, returns guard holding pointer to `MutexInner`.
- guard drop calls `pthread_mutex_unlock`.
- mutex drop (destructor) calls `pthread_mutex_destroy` then frees storage; **undefined** if still
  locked — document as user error, same as Rust.

### RwLock[T]

Use `pthread_rwlock_t` with the same inner layout pattern as `Mutex`.

### Condvar

```text
CondvarInner {
    cond: pthread_cond_t,
}
```

`wait(guard)`:

1. extract `pthread_mutex_t*` from the guard’s `MutexInner`;
2. `pthread_cond_wait(&cond, mutex)`;
3. return a new guard for the same mutex.

### Thread entry

Extend `@nialang.thread.entry` only if needed. The existing path already invokes
`{ code, env, drop }` on a malloc’d payload; closure `spawn` can reuse it without a new entry shim.

Declare in the LLVM prelude:

```llvm
declare i32 @pthread_mutex_init(ptr, ptr)
declare i32 @pthread_mutex_destroy(ptr)
declare i32 @pthread_mutex_lock(ptr)
declare i32 @pthread_mutex_unlock(ptr)
declare i32 @pthread_rwlock_init(ptr, ptr)
declare i32 @pthread_rwlock_destroy(ptr)
declare i32 @pthread_rwlock_rdlock(ptr)
declare i32 @pthread_rwlock_wrlock(ptr)
declare i32 @pthread_rwlock_unlock(ptr)
declare i32 @pthread_cond_init(ptr, ptr)
declare i32 @pthread_cond_destroy(ptr)
declare i32 @pthread_cond_wait(ptr, ptr)
declare i32 @pthread_cond_signal(ptr)
declare i32 @pthread_cond_broadcast(ptr)
```

On failure, follow the existing `emit_abort_if_pthread_error` pattern (abort on unexpected pthread
error). `try_lock` / `try_read` / `try_write` map `EBUSY` to `None`.

## Compiler Changes

### AST

Add type variants:

```rust
Arc(Box<Ty>),
Mutex(Box<Ty>),
RwLock(Box<Ty>),
MutexGuard(Box<Ty>),
RwLockReadGuard(Box<Ty>),
RwLockWriteGuard(Box<Ty>),
Condvar,
```

### Parser

Recognize built-in type names:

```text
Arc[T]
Mutex[T]
RwLock[T]
MutexGuard[T]
RwLockReadGuard[T]
RwLockWriteGuard[T]
Condvar
```

Recognize `spawn move || ...` (reuse closure parser; require `move` keyword).

### Built-in Names

Reserve constructors and methods:

```text
arc_new
mutex_new
rwlock_new
condvar_new
.lock .try_lock
.read .write .try_read .try_write
.wait .notify_one .notify_all
```

### Typechecker

New helpers:

```rust
fn has_send(ty, env) -> bool
fn has_sync(ty, env) -> bool
fn require_send(ty, site) -> Result<(), String>
fn require_sync(ty, site) -> Result<(), String>
```

Wire into:

- `validate_abilities` — structural checks for declared `send` / `sync`;
- `spawn` — accept `Expr::Closure` with `move`, check `fn() -> ()`, check captures;
- `arc_new`, mutex/rwlock constructors;
- built-in method signatures for guards (no escaping guard to outer thread).

Reject:

- `Arc[qubit]`, `Mutex[result]`, etc.;
- `spawn || ...` without `move`;
- `spawn` of capturing closure where a capture is not `send`;
- copying guards;
- `Condvar::wait` with a guard from a different mutex (dynamic check hard — MVP documents UB;
  optional pointer equality assert in debug runtime later).

### Codegen

- map handles to `ptr` in LLVM;
- emit pthread calls through private runtime helpers (`@nialang.sync.mutex.lock`, etc.) to keep IR
  readable, similar to list/matrix helpers;
- `Arc` refcount paths use existing atomic codegen;
- `MutexGuard` drop lowers to unlock + free guard struct;
- `spawn` on closure: emit function value as today, pass payload to `pthread_create`.

## Phased Rollout

### Phase 0: Send / Sync Semantics

Status: implemented.

Goal: make auto-trait rules real before any runtime types ship.

- implement `has_send` / `has_sync` queries;
- enforce structural validation when `send` / `sync` appear in `has` clauses;
- mark `Atomic*` as `send + sync`;
- mark `Thread` as `!send` / `!sync`;
- tests for struct/enum/vector send/sync derivation and opt-out via non-send fields.
- sample: `examples/sample_send_sync.nia`

No new runtime types yet.

### Phase 1: Arc[T]

Goal: shared ownership across threads.

- `Arc[T]` type, `arc_new`, `.clone`, deref, drop;
- refcount lowering with `AtomicUsize`;
- requires `T: send + sync`;
- sample: `examples/sample_arc.nia`;
- tests: parser/typecheck/codegen coverage for `Arc[T]`, move-only semantics, read-only deref,
  non-`send + sync` inner rejection, atomic refcount increment/decrement, and `Arc[AtomicI32]`
  method access through `*arc`.

### Phase 2: spawn move ||

Goal: thread closures with moved `send` captures.

- implemented: parser/typechecker support for `spawn move || ...`;
- implemented: closure env drop is reused on the worker thread;
- implemented: every capture must be `send`, including function values with closure envs;
- kept `spawn top_level_fn` working;
- added `examples/sample_threads_closure.nia` and `examples/tests/ok_spawn_move_closure.nia`.

### Phase 3: Mutex[T]

Goal: exclusive access to shared data.

- implemented: `Mutex[T]`, `MutexGuard[T]`, `mutex_new`, `.lock`, `.try_lock`;
- implemented: `MutexGuard` RAII unlock on `drop` / scope exit;
- implemented: `*guard` read/write access to protected value;
- added `examples/sample_mutex.nia`;
- added runtime stress sample: two threads increment shared `Arc[Mutex[i32]]` to 2000.

### Phase 4: RwLock[T]

Goal: read-heavy shared state.

- `rwlock_new`, read/write guards, try variants;
- sample: `examples/sample_rwlock.nia`;
- tests: concurrent readers, exclusive writer.

### Phase 5: Condvar

Goal: blocking until shared state changes.

- `condvar_new`, `wait`, `notify_one`, `notify_all`;
- sample producer/consumer with `Mutex` + `Condvar`;
- tests: waiter wakes after `notify_one`, predicate loop handles spurious wakeup.

### Phase 6: Documentation and spec

- document stable subset in `spec.txt`;
- update `README.md` concurrency section;
- cross-link from [atomics-plan.md](atomics-plan.md) non-goals into this plan;
- note pthread / platform requirements in driver docs.

## Tests

Parser:

- `Arc[i32]`, `Mutex[Counter]`, `RwLock[T]`, `Condvar` parse;
- `spawn move || ()` parses;
- `spawn || ()` rejected.

Typecheck ok fixtures:

- `ok_send_sync_struct.nia`
- `ok_arc_basic.nia`
- `ok_mutex_guard_mut.nia`
- `ok_rwlock_read_write.nia`
- `ok_condvar_wait_notify.nia`
- `ok_spawn_move_closure.nia`
- `ok_arc_mutex_threads.nia` — integration-style typecheck fixture

Typecheck error fixtures:

- non-`send` capture in `spawn move || ...`
- `arc_new` with non-`sync` inner type
- `mutex_new` with `qubit` inner type
- use of moved guard after `drop(guard)`
- `Thread` in `Arc[Thread]`

Codegen tests:

- `Arc` clone emits atomic increment
- `Arc` drop emits atomic decrement + conditional destroy
- `Mutex::lock` emits `pthread_mutex_lock`
- guard drop emits `pthread_mutex_unlock`
- `spawn move ||` emits `pthread_create` + `nialang.thread.entry`

Runtime integration (execute compiled binary):

- two threads increment `Arc[Mutex[i32]]` to 2000
- `Condvar` ping-pong between two threads

## Non-Goals

- `mpsc` / `sync_channel` / `select` (separate plan).
- `Once`, `Barrier`, `Semaphore`.
- Async/await and executor-based tasks.
- Cross-process mutexes / named IPC primitives.
- `Rc[T]` (single-thread refcount) — can be added later as a distinct non-`sync` type.
- Poisoning and panic-on-panic diagnostics.
- Fairness / priority inheritance configuration.
- Windows SRWLOCK / CRITICAL_SECTION backend (pthread-only MVP on Unix; portable abstraction later).
- Quantum circuit sharing across OS threads.
- Weak pointers (`Weak[T]`).
- `std::sync::atomic::fence` beyond existing `atomic_fence`.

## Open Questions

1. **`MutexGuard` and `mut`**: implement mutable access only through `let mut guard`, or add an
   explicit `.get_mut(&mut self) -> &mut T` method first?
2. **Guard `send`**: keep guards `!send` strictly (recommended), or allow sending an **unlocked**
   empty token (Rust does not — follow Rust).
3. **`Arc` strong-only MVP**: omit `Weak[T]` entirely in v1 (recommended).
4. **Try-lock errors**: return `None` only, or add `Result` with an error enum later?
5. **Thread naming / stack size**: expose optional `spawn` attributes later?
6. **Static globals**: when `static` with `sync` data exists, wire `require_sync` there too.

## Suggested Implementation Order

1. Phase 0 — `send` / `sync` queries and declared-ability validation.
2. Phase 1 — `Arc[T]` (needed by every later example).
3. Phase 2 — `spawn move ||` (can ship right after Arc; mutex integration tests want thread
   closures available).
4. Phase 3 — `Mutex[T]`.
5. Phase 4 — `RwLock[T]`.
6. Phase 5 — `Condvar`.
7. Phase 6 — docs and spec.

Practical parallel track: Phases 3–5 runtime code can be developed against placeholder `send`/`sync`
checks, but do not ship without Phase 0 enforcement.
