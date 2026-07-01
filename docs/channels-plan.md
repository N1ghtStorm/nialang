# Channels Plan

Status: design plan.

Depends on:

- [concurrency-plan.md](concurrency-plan.md) - `send` / `sync`, `spawn move ||`, `Arc`, and
  pthread-backed blocking primitives.
- [abilities-plan.md](abilities-plan.md) - move checking, `clone`, `drop`, and derived ability
  rules.

## Goal

Add Go-style typed channels to Nia as a first-class synchronization primitive:

- `chan[T]` for a bidirectional channel handle;
- `<- chan[T]` for a receive-only channel handle;
- `chan[T] <-` for a send-only channel handle;
- `ch <- value` for blocking send;
- `<-ch` for blocking receive;
- `close(ch)` for sender-side close;
- buffered and unbuffered construction;
- later `select` over channel operations.

The receive-only spelling is intentionally:

```nia
<- chan[T]
```

It is visually close to Go's `<-chan T`, but keeps Nia's existing `Type[T]` type constructor style.
The visible source type should be lowercase `chan[T]`, not `Channel[T]`, unless the whole language
style changes later.

Nia does not have user-level generics. Like `List[T]`, `Arc[T]`, and `AtomicPtr[T]`, `chan[T]` is a
compiler-known type constructor.

## Design Principles

- Channels transfer ownership of values; sending a non-`copy` value moves it into the channel.
- Receiving owns the value that comes out of the channel.
- Channel direction is part of the static type and is enforced at compile time.
- Directional channel types are views over the same runtime handle, not different heap objects.
- `chan[T]` may coerce to `<- chan[T]` or `chan[T] <-`; the reverse direction is rejected.
- Channel handles are shallow-cloned reference-counted handles. Cloning does not clone queued
  values.
- Element type `T` must be `send`. This keeps the MVP focused on cross-thread communication.
- Blocking operations use pthread mutexes and condition variables on native targets.
- QIR, GPU kernels, async/await, cancellation, and deadlines are out of scope for the MVP.

## Source Surface

### Construction

```nia
let unbuffered: chan[i32] = chan_new[i32]();
let buffered: chan[i32] = chan_with_capacity[i32](64);
```

Constructors:

| Function | Signature | Notes |
| --- | --- | --- |
| `chan_new` | `fn chan_new[T]() -> chan[T]` | unbuffered rendezvous channel; requires `T: send` |
| `chan_with_capacity` | `fn chan_with_capacity[T](capacity: usize) -> chan[T]` | buffered FIFO; requires `T: send` |

`chan_with_capacity[T](0)` is equivalent to `chan_new[T]()`.

### Directional Handles

```nia
fn producer(out: chan[i32] <-) {
    out <- 1;
    out <- 2;
    close(out);
}

fn consumer(input: <- chan[i32]) {
    match <-input {
        Some(value) => println(value),
        None => println(0),
    };
}
```

Direction rules:

| Type | Can send | Can receive | Can close |
| --- | --- | --- | --- |
| `chan[T]` | yes | yes | yes |
| `chan[T] <-` | yes | no | yes |
| `<- chan[T]` | no | yes | no |

Implicit direction narrowing:

```nia
let ch: chan[i32] = chan_new[i32]();
let tx: chan[i32] <- = ch.clone();
let rx: <- chan[i32] = ch;
```

Allowed:

- `chan[T]` to `chan[T] <-`;
- `chan[T]` to `<- chan[T]`;
- exact direction-preserving assignment or parameter passing.

Rejected:

- `<- chan[T]` to `chan[T]`;
- `chan[T] <-` to `chan[T]`;
- `<- chan[T]` to `chan[T] <-`;
- `chan[T] <-` to `<- chan[T]`.

### Send

```nia
let ch: chan[string] = chan_new[string]();
let name = "nia";

ch <- name;      // sends a copy-like string value today; later follows formal copy/move rules
```

For non-`copy` values, send consumes the value:

```nia
let xs: List[i32] = list_new[i32]();
ch <- xs;        // moves xs into the channel
println(xs.len()); // error: use after move
```

Rules:

- `ch <- value` requires `ch: chan[T]` or `ch: chan[T] <-`.
- `value` must be assignable to `T`.
- if `T` is not `copy`, the value is moved.
- send blocks until a receiver accepts the value or a buffered channel has space.
- send on a closed channel aborts until Nia has a panic/result model for this operation.

### Receive

```nia
let maybe: Option[i32] = <-ch;

match <-ch {
    Some(value) => println(value),
    None => println(0),
};
```

Receive returns `Option[T]`:

- `Some(value)` means a value was received and ownership moved to the receiver.
- `None` means the channel is closed and empty.

Rules:

- `<-ch` requires `ch: chan[T]` or `ch: <- chan[T]`.
- receive blocks until a value is available or the channel is closed and drained.
- receiving from a closed but non-empty channel drains queued values first.
- receiving from a closed and empty channel returns `None` immediately.

This deliberately differs from Go's single-value receive, which returns the element zero value after
close. Nia should not invent zero values for arbitrary `T`; `Option[T]` makes close visible in the
type.

### Close

```nia
let tx: chan[i32] <- = chan_new[i32]();
close(tx);
```

`close(ch)`:

- requires a send-capable handle (`chan[T]` or `chan[T] <-`);
- consumes that handle;
- wakes blocked receivers;
- makes future receives drain buffered values and then return `None`;
- aborts on double close;
- aborts blocked or future sends.

Only the logical owner of the send side should close the channel. With cloned send handles, this is
a protocol rule rather than a fully static guarantee in the MVP.

### Clone And Drop

```nia
let ch: chan[i32] = chan_new[i32]();
let tx: chan[i32] <- = ch.clone();
let rx: <- chan[i32] = ch;
```

`.clone()` returns the same static direction:

| Receiver | Result |
| --- | --- |
| `chan[T]` | `chan[T]` |
| `chan[T] <-` | `chan[T] <-` |
| `<- chan[T]` | `<- chan[T]` |

Dropping a handle decrements the runtime reference count. Dropping the last handle frees channel
storage and drops any queued values. Drop does not close the channel; a receiver can still block
forever if the program drops or loses all send-capable handles without calling `close`.

## Select

`select` should be a later phase, after basic channels are implemented and tested.

Target syntax:

```nia
select {
    case out <- value => {
        println(1);
    }
    case msg = <-input => {
        match msg {
            Some(value) => println(value),
            None => println(0),
        };
    }
    case <-done => {
        println(2);
    }
    default => {
        println(3);
    }
}
```

Rules:

- send cases require send-capable channels;
- receive cases require receive-capable channels;
- `case name = <-ch` binds `name: Option[T]`;
- `case <-ch` is allowed when the received value is intentionally ignored;
- `default` runs only when no channel operation is ready;
- if multiple cases are ready, the runtime should choose pseudo-randomly or rotate fairly enough to
  avoid obvious starvation.

Move-sensitive send cases need careful implementation. Exactly one selected send arm may consume its
payload. The first `select` implementation may restrict send payloads to `copy` types until the
move checker can represent "moved only if this arm was selected".

## Type Rules Summary

| Type | `copy` | `clone` | `drop` | `send` | `sync` |
| --- | --- | --- | --- | --- | --- |
| `chan[T]` | no | yes | yes | if `T: send` | if `T: send` |
| `chan[T] <-` | no | yes | yes | if `T: send` | if `T: send` |
| `<- chan[T]` | no | yes | yes | if `T: send` | if `T: send` |

Element rules:

- `chan[T]` construction requires `T: send`;
- sending a value requires `T: send`;
- channel handles themselves are `send + sync` when `T: send`;
- `chan[Thread]`, `chan[qubit]`, and `chan[result]` are rejected under the current send rules.

Nested channels are allowed because channel handles are `send`:

```nia
let meta: chan[chan[i32]] = chan_new[chan[i32]]();
```

Directional nested channel types are also valid:

```nia
let rx_of_rx: chan[<- chan[i32]] = chan_new[<- chan[i32]]();
```

## Runtime Layout

All channel handle types use the same pointer-sized runtime representation:

```text
chan[T] handle -> ptr to ChanInner[T]
```

Sketch:

```text
ChanInner[T] {
    refcount: AtomicUsize,
    mutex: pthread_mutex_t,
    send_cv: pthread_cond_t,
    recv_cv: pthread_cond_t,
    closed: bool,
    capacity: usize,
    len: usize,
    head: usize,
    tail: usize,
    buffer: storage for T,
    rendezvous_slot: storage for T,
    rendezvous_full: bool,
}
```

Buffered channels use `buffer` as a FIFO ring. Unbuffered channels use the rendezvous slot and the
condition variables to pair one sender with one receiver.

Send algorithm:

1. lock `mutex`;
2. if `closed`, abort;
3. if a receiver can accept immediately, move the value into the receiver path and wake it;
4. otherwise, if the buffer has space, push the value and wake one receiver;
5. otherwise, block on `send_cv`;
6. retry after wakeup because pthread condition variables may wake spuriously.

Receive algorithm:

1. lock `mutex`;
2. if a value is buffered or in the rendezvous slot, move it out and wake one sender;
3. otherwise, if `closed`, return `None`;
4. otherwise, block on `recv_cv`;
5. retry after wakeup.

Close algorithm:

1. lock `mutex`;
2. if already `closed`, abort;
3. set `closed = true`;
4. wake all receivers and senders;
5. receivers drain buffered values, then return `None`;
6. senders abort when they observe closure.

The pthread mutex and condition variables provide the memory ordering guarantees for values sent
through the channel.

## Compiler Changes

### Lexer

Add:

- `chan` keyword;
- `<-` token for both channel type direction and expressions.

The parser can still treat `chan` as contextual in type positions if preserving old identifiers is
valuable, but the compiler is early enough that making it reserved is acceptable.

### AST

Add channel type support:

```rust
enum ChanDir {
    Both,
    SendOnly,
    RecvOnly,
}

Ty::Chan {
    elem: Box<Ty>,
    dir: ChanDir,
}
```

Add expression forms:

```rust
Expr::ChanSend {
    channel: Box<Expr>,
    value: Box<Expr>,
}

Expr::ChanRecv {
    channel: Box<Expr>,
}
```

Later:

```rust
Expr::Select {
    cases: Vec<SelectCase>,
    default: Option<Block>,
}
```

### Parser

Type grammar additions:

```text
Ty :=
  | "chan" "[" Ty "]"
  | "<-" "chan" "[" Ty "]"
  | "chan" "[" Ty "]" "<-"
```

Expression grammar additions:

```text
Expr :=
  | Expr "<-" Expr     // send, statement-like precedence
  | "<-" Expr          // receive, unary precedence
```

Precedence notes:

- `<-ch.method()` should parse as `<-(ch.method())`;
- `ch <- f(x)` should parse as `ch <- (f(x))`;
- send is valid only as a statement expression returning `()`;
- receive is a normal expression returning `Option[T]`.

### Typechecker

Add helpers:

```rust
fn is_channel_send_capable(ty: &Ty) -> Option<&Ty>
fn is_channel_recv_capable(ty: &Ty) -> Option<&Ty>
fn narrow_channel_direction(ty: &Ty, target: ChanDir) -> Result<Ty, String>
```

Wire into:

- construction of `chan_new[T]` and `chan_with_capacity[T]`;
- ability queries for `clone`, `drop`, `send`, and `sync`;
- assignment and parameter passing for direction narrowing;
- send expression checking;
- receive expression checking;
- move checking for values consumed by send;
- `close(ch)` as a consuming builtin.

Diagnostics should name both the operation and the direction mismatch:

```text
cannot send on receive-only channel `<- chan[i32]`
cannot receive from send-only channel `chan[i32] <-`
cannot close receive-only channel `<- chan[i32]`
channel element type `Thread` is not send
```

### Codegen

Lower all channel handles to `ptr`.

Emit or call runtime helpers:

```text
@nialang.chan.new(element_size, element_align, capacity, drop_fn) -> ptr
@nialang.chan.clone(ptr) -> ptr
@nialang.chan.drop(ptr)
@nialang.chan.send(ptr, value_ptr)
@nialang.chan.recv(ptr, out_ptr) -> i1
@nialang.chan.close(ptr)
```

`recv` returns `true` for `Some` and `false` for `None`; the caller constructs `Option[T]`.

For values with drop glue, the channel runtime must store a drop callback or use monomorphized
wrappers so queued values are destroyed correctly when the channel is freed.

Declare pthread functions in the LLVM prelude:

```llvm
declare i32 @pthread_mutex_init(ptr, ptr)
declare i32 @pthread_mutex_destroy(ptr)
declare i32 @pthread_mutex_lock(ptr)
declare i32 @pthread_mutex_unlock(ptr)
declare i32 @pthread_cond_init(ptr, ptr)
declare i32 @pthread_cond_destroy(ptr)
declare i32 @pthread_cond_wait(ptr, ptr)
declare i32 @pthread_cond_signal(ptr)
declare i32 @pthread_cond_broadcast(ptr)
```

Use the existing pthread error handling style from thread, mutex, rwlock, and condvar codegen.

## Phased Rollout

### Phase 0: Syntax And Documentation

Goal: reserve the surface shape before runtime work starts.

- add this plan;
- add `chan` and `<-` lexer tokens;
- parse channel types:
  - `chan[T]`;
  - `<- chan[T]`;
  - `chan[T] <-`;
- parse send and receive expressions;
- add parser tests for accepted syntax and ambiguity cases.

No runtime behavior yet.

### Phase 1: Typechecker Direction Rules

Goal: make channels meaningful in the type system.

- add `Ty::Chan`;
- implement direction narrowing;
- implement channel ability rules;
- typecheck `chan_new[T]` and `chan_with_capacity[T]`;
- typecheck `ch <- value`;
- typecheck `<-ch` as `Option[T]`;
- typecheck consuming `close(ch)`;
- reject non-`send` element types.

No LLVM lowering yet; driver tests can stop after typecheck for this phase.

### Phase 2: Blocking Runtime

Goal: ship usable channels without `select`.

- implement runtime layout;
- implement unbuffered send/receive;
- implement buffered FIFO send/receive;
- implement close semantics;
- implement clone/drop and queued value destruction;
- lower constructors, send, receive, close, clone, and drop.

Acceptance examples:

- single-thread buffered FIFO;
- unbuffered producer/consumer using `spawn move ||`;
- close then drain;
- receive from closed empty channel returns `None`;
- send on closed channel aborts.

### Phase 3: Directional API Polish

Goal: make producer/consumer APIs pleasant and hard to misuse.

- support directional channel types in function parameters and return types;
- ensure `.clone()` preserves direction;
- add examples with explicit `chan[T] <-` producer endpoints and `<- chan[T]` consumer endpoints;
- update README and spec once the behavior is implemented.

### Phase 4: Select

Goal: add Go-style waiting on multiple channel operations.

- parse `select { case ... => ... default => ... }`;
- implement runtime registration/wakeup for multiple candidate operations;
- implement fair-ish ready-case selection;
- decide the first move-checking rule for send payloads:
  - either restrict send cases to `copy` payloads at first;
  - or add conditional move state so only the selected arm consumes its value.

### Phase 5: Optional Extensions

These should wait until the core semantics are solid:

- non-blocking `try_recv` / `try_send` result enums;
- `len(ch)` and `cap(ch)` for buffered channels;
- timer channels or sleep integration;
- select timeouts;
- channel iteration sugar after `for` / iterator design exists;
- typed close protocols that make double close and multi-producer close safer.

## Tests

Parser tests:

- `chan[i32]`;
- `<- chan[i32]`;
- `chan[i32] <-`;
- nested directional channels;
- `ch <- value`;
- `<-ch`;
- `select` syntax once Phase 4 starts.

Typechecker accept tests:

- send/receive on `chan[T]`;
- send on `chan[T] <-`;
- receive on `<- chan[T]`;
- direction narrowing from bidirectional to send-only and receive-only;
- send moves non-`copy` values;
- receive returns `Option[T]`;
- close consumes a send-capable handle.

Typechecker reject tests:

- send on `<- chan[T]`;
- receive from `chan[T] <-`;
- close on `<- chan[T]`;
- channel element type without `send`;
- using a non-`copy` value after sending it;
- widening a directional channel back to `chan[T]`.

Runtime tests:

- unbuffered handoff between two threads;
- buffered FIFO order;
- blocking send wakes after receive;
- blocking receive wakes after send;
- close wakes blocked receivers;
- close drains buffered values before `None`;
- double close aborts;
- send after close aborts;
- queued values are dropped when the last handle drops.

## Open Questions

1. Should `close(ch)` consume the handle forever, as proposed here, or borrow it like Go?
2. Should dropping the last send-capable handle implicitly close the channel, or stay Go-like and
   require explicit close?
3. Should send-only type syntax remain postfix (`chan[T] <-`), or should the language introduce a
   more Go-like spelling if a better one appears?
4. Should the first `select` implementation support moving send payloads, or start with `copy`
   payloads only?
5. Should send on closed channel abort, or should Nia add a result-returning send API before
   channels stabilize?
