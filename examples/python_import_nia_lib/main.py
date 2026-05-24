from __future__ import annotations

import ctypes
import platform
import subprocess
from pathlib import Path

def main() -> None:
    lib_path = build_nia_library()
    nia = ctypes.CDLL(str(lib_path))

    nia.nia_add.argtypes = [ctypes.c_int32, ctypes.c_int32]
    nia.nia_add.restype = ctypes.c_int32
    nia.nia_double.argtypes = [ctypes.c_int32]
    nia.nia_double.restype = ctypes.c_int32
    nia.something.argtypes = []
    nia.something.restype = ctypes.c_int32
    vec4 = ctypes.c_double * 4
    nia.vector_mul.argtypes = [ctypes.POINTER(vec4), ctypes.POINTER(vec4)]
    nia.vector_mul.restype = ctypes.c_double

    sum_value = nia.nia_add(20, 22)
    doubled = nia.nia_double(21)
    smth = nia.something()
    a = vec4(1.2, 2.0, 3.0, 4.0)
    b = vec4(10.0, 20.0, 30.0, 40.0)
    dot = nia.vector_mul(ctypes.byref(a), ctypes.byref(b))
    flush_c_stdout()

    print(f"nia_add(20, 22) = {sum_value}")
    print(f"nia_double(21) = {doubled}")
    print(f"something() = {smth}")
    print(f"vector_mul(a, b) = {dot}")

    assert sum_value == 42
    assert doubled == 42
    assert smth == 666

def build_nia_library() -> Path:
    example_dir = Path(__file__).resolve().parent
    repo_root = example_dir.parents[1]
    nia_src = example_dir / "nia_lib.nia"
    lib_path = example_dir / "build" / dynamic_lib_filename("nia_sample")

    cmd = [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(repo_root / "Cargo.toml"),
        "--",
        str(nia_src),
        "--lib",
        "-o",
        str(lib_path),
    ]

    try:
        subprocess.run(cmd, check=True)
    except FileNotFoundError as exc:
        raise SystemExit("failed to run `cargo`; install Rust and ensure Cargo is on PATH") from exc
    except subprocess.CalledProcessError as exc:
        raise SystemExit(f"nialang failed to build {lib_path}") from exc

    return lib_path


def dynamic_lib_filename(stem: str) -> str:
    system = platform.system()
    if system == "Darwin":
        return f"lib{stem}.dylib"
    if system == "Windows":
        return f"{stem}.dll"
    return f"lib{stem}.so"


def flush_c_stdout() -> None:
    try:
        fflush = ctypes.CDLL(None).fflush
    except (AttributeError, OSError):
        return

    fflush.argtypes = [ctypes.c_void_p]
    fflush.restype = ctypes.c_int
    fflush(None)


if __name__ == "__main__":
    main()
