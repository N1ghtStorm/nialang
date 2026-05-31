FILE ?= examples/sample_floats.nia
OUT ?=

pyex:
	python3 examples/python_import_nia_lib/main.py

ll:
	cargo run -- $(FILE) --emit-ll $(OUT)
