# Rust Importing A Nia Library

This example crate builds `nia_lib.nia` as a shared library during
`build.rs`, links it into a Rust binary, and calls the exported C ABI symbols.

Run it from the repository root:

```bash
cargo run --manifest-path examples/rust_import_nia_lib/Cargo.toml
```

The Nia exports used by Rust are:

```c
#include <stdint.h>

int32_t nia_add(int32_t a, int32_t b);
int32_t nia_double(int32_t x);
```
