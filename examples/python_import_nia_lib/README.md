# Python Importing A Nia Library

This example builds `nia_lib.nia` as a shared library, loads it from Python
with `ctypes`, and calls the exported C ABI symbols.

Run it from the repository root:

```bash
python3 examples/python_import_nia_lib/main.py
```

The Nia exports used by Python are:

```c
#include <stdint.h>

int32_t nia_add(int32_t a, int32_t b);
int32_t nia_double(int32_t x);
int32_t something(void);
```
