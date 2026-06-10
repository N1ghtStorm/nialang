# Задачи: переделка компилятора NiaLang

**Ветка:** `deptypes`  
**Статус:** активная разработка  
**Режим:** vibe-coding (AI + человек держит архитектуру)  
**Архитектурный reference:** [dependent-types-design.md](./dependent-types-design.md)

## Цель

Полностью переписать компилятор под зависимые типы (F*-style Core), **сохранив
базовый surface-синтаксис** из [spec.txt](../spec.txt). Старый typechecker и
прямой путь `AST → LLVM` — disposable. Контракт — lexer/parser + `examples/`.

## Инварианты (не нарушать при вайбе)

1. **Surface syntax не ломаем** — существующие `.nia` файлы должны парситься.
2. **Backend не видит surface AST** — только `CheckedModule` / HIR после erasure.
3. **Единый `Term`** — тип = терм; не добавлять `Ty::Dependent(Box<Expr>)`.
4. **Conversion детерминирован** — NbE/definitional equality; Z3 только для VC/refinements.
5. **Один milestone = один PR** — не расползаться на effects + quantum + SMT одновременно.
6. **Negative tests обязательны** — ill-typed программы должны падать так же предсказуемо, как typed.

## Целевая структура `src/`

```text
src/
  frontend/
    lexer/           # сохранить, минимальные правки
    parser/          # сохранить, Ty → SurfaceTy
    surface/         # SurfaceExpr, SurfaceTy, SurfaceItem
    resolve/         # DefId, LocalId, name → id
  core/
    term.rs          # единый Term
    env.rs           # typing context, levels
    checker.rs       # bidirectional infer/check
    nbe.rs           # eval + quote
    unify.rs         # metavariables, first-order unification
    inductive.rs     # ADT, families, positivity (фаза 4+)
    diagnostics.rs
  elab/              # surface → Core
  verify/            # guards, VC, SMT (фаза 6+)
  erase/             # Core → ErasedModule
  hir/
    classical.rs
    quantum.rs
  backend/
    llvm/            # HIR → LLVM text
    qir/             # QuantumHir → QIR
  prelude/           # desugared builtins (matrix, gates, …)
  driver/
    pipeline.rs      # orchestration, dual-path на время миграции
  lib.rs
```

## Tier-контракт (examples как oracle)

Прогресс измеряем прохождением tier'ов через **новый** pipeline.

### Tier 0 — минимальный язык

Файлы (`examples/tests/`):

- `ok_minimal.nia`
- `ok_if_return.nia`
- `ok_struct_named.nia`
- `ok_tuple_struct.nia`
- `ok_tuple_named_mix.nia`
- `ok_nested_if.nia`
- `ok_print_primitives.nia`
- `ok_enum_match.nia`
- `ok_enum_payload_match.nia`
- `ok_print_enum.nia`

**Критерий:** parse → elab → Core check → (пока без codegen) OK.

### Tier 1 — control flow, arrays, pointers

- `ok_for_range.nia`, `ok_while.nia`, `ok_loop.nia`
- `ok_array.nia`, `ok_array_index.nia`, `ok_array_len.nia`, `ok_array_index_store.nia`, `ok_array_reverse.nia`
- `ok_readme_arrays.nia`, `ok_readme_enums.nia`
- `ok_pointers.nia`, `ok_ptr_write.nia`, `ok_ptr_array_write.nia`, `ok_readme_pointers.nia`
- `ok_alloc_heap.nia`, `ok_compound_assign.nia`, `ok_bitwise.nia`

**Критерий:** + erase → ClassicalHir → LLVM → `clang` → run OK.

### Tier 2 — floats, strings, impl, vectors, matrices

- `ok_floats.nia`, `ok_string.nia`
- `ok_impl_methods.nia`, `ok_print_structs.nia`, `ok_print_array.nia`
- `ok_vector_to_array.nia`, `ok_array_to_vec.nia`, `ok_array_matrix_conversions.nia`
- + sample-файлы: `sample_linalg_commented.nia`, `sample_vector.nia`, `sample_matrix_arith.nia`

**Критерий:** feature parity со старым compiler на tier 2.

### Tier 3 — extern, scopes, quantum

- `ok_gpu_scope.nia`, `ok_quant_scope.nia`
- `sample_extern_lib.nia`, `sample_extern_fn.nia`
- `examples/quantum/*.nia` (по одному, начиная с `qubit_create.nia`)

**Критерий:** QIR backend через QuantumHir.

### Tier D — dependent types milestone

Не из текущих examples — **новые** тесты в `examples/tests/core/`:

```nia
// ok_vec_append.nia — целевой демо-milestone
data Nat: Type { Zero, Succ: Nat -> Nat }
data Vec(#a: Type): Nat -> Type { Nil, Cons: ... }
fn append(...) Vec[a, add(n, m)] { ... }
```

**Критерий:** dependent match + index unification + termination на `append`.

---

## Фазы и задачи

Отмечай `[x]` по мере выполнения. Каждая задача — один vibe-session (1–4 часа).

---

### Фаза 0 — Зафиксировать контракт

**Цель:** ничего не переписываем, но фиксируем baseline и правила игры.

- [x] **0.1** Добавить `cargo test` harness: прогон всех `examples/tests/ok_*.nia` через **старый** pipeline (snapshot текущего поведения).
- [x] **0.2** Разделить тесты: `driver/tests/parse_only.rs`, `typecheck_only.rs`, `baseline.rs` (full pipeline).
- [x] **0.3** Задокументировать tier-0..3 списки (этот файл + `src/driver/fixtures.rs`).
- [x] **0.4** Добавить CLI flag `--core-only`: parse → print surface AST (для отладки нового пути).
- [x] **0.5** Зафиксировать решения (заполнить секцию «Решения» ниже).

**Done when:** `cargo test` green на старом compiler; tier lists согласованы.

---

### Фаза 1 — Surface layer

**Цель:** отделить пользовательское представление от семантики.

- [x] **1.1** Создать `src/frontend/surface/` — перенести `Expr`, `Stmt`, item defs из `ast/mod.rs`.
- [x] **1.2** Переименовать `Ty` → `SurfaceTy` (parser + surface + tests).
- [x] **1.3** `ast/mod.rs` → thin re-export (`pub type Ty = SurfaceTy` для legacy TC/codegen).
- [x] **1.4** Parser возвращает `SurfaceModule`, не tuple.
- [x] **1.5** Все существующие parse-тесты green.

**Vibe prompt:** «Перенеси AST в `frontend/surface`, переименуй Ty в SurfaceTy, поведение parser не меняй.»

**Done when:** старый TC работает через SurfaceTy alias/adapter; parse tests green.

---

### Фаза 2 — Name resolution

**Цель:** строки → стабильные id до Core.

- [x] **2.1** `DefId`, `LocalId`, `ConstructorId`, `EffectId` (newtype u32).
- [x] **2.2** `resolve/module.rs`: resolve top-level items, struct/enum/vector/fn names.
- [x] **2.3** `ResolvedModule` — surface + id tables.
- [x] **2.4** Duplicate name errors (spans — когда parser получит location info).
- [x] **2.5** Unit tests: collision struct/enum/vector, duplicate fn/variant, reserved names.

**Done when:** `resolve(parse(src))` для tier-0 файлов без ошибок.

---

### Фаза 3 — Core skeleton

**Цель:** минимальный dependently-typed calculus без surface sugar.

- [x] **3.1** `core/term.rs`: `Var`, `Global`, `Universe`, `Pi`, `Lam`, `App`, `Let`, literals (`I32`, `Bool`, …), `Error`.
- [x] **3.2** `Binder`: name_hint, explicitness, relevance (Runtime/Erased).
- [x] **3.3** `core/env.rs`: context с de Bruijn levels / named lookup.
- [x] **3.4** `core/nbe.rs`: eval + quote для Pi/App/Lam/Let/Var; whnf.
- [x] **3.5** `core/checker.rs`: bidirectional check для Core-only programs.
- [x] **3.6** `is_def_eq(t1, t2)` через NbE + syntactic compare.
- [x] **3.7** 35 unit tests: alpha equivalence, Pi typing, app typing, let, universe mismatch → error.

**Vibe prompt:** «Реализуй core/term + nbe + checker по dependent-types-design.md §5. Без surface, без unification.»

**Done when:** hand-written Core terms typecheck in unit tests.

---

### Фаза 4 — Elaborator (tier 0)

**Цель:** surface tier 0 → Core.

- [x] **4.1** `elab/expr.rs`: literals, var, let, fn call, binary ops (+−*/ on i32).
- [x] **4.2** `elab/stmt.rs`: let, return, expr stmt, blocks.
- [x] **4.3** `elab/item.rs`: fn defs, struct → inductive desugar (простые record types).
- [x] **4.4** `elab/enum.rs`: enum → inductive с constructors.
- [x] **4.5** `elab/match.rs`: non-dependent match (indices фиксированы).
- [x] **4.6** `if` expression/statement elaboration.
- [x] **4.7** Pipeline mode: `--elab-only` печатает elaborated Core.
- [x] **4.8** Integration: tier-0 `ok_*.nia` → elab → check OK.

**Done when:** все Tier 0 examples проходят elab + Core check.

---

### Фаза 5 — Metavariables + implicits

**Цель:** inference как в настоящем dependent checker.

- [x] **5.1** `MetaId`, `MetaEnv`, assign/solve.
- [x] **5.2** `core/unify.rs`: first-order unification на head-normal forms.
- [x] **5.3** Implicit `#a: Type` в surface (parser) + insertion в elab.
- [x] **5.4** Diagnostics: «failed to infer implicit argument» с span.
- [x] **5.5** Negative tests: ambiguous implicit → error.

**Done when:** можно написать `fn id(#a: Type, x: a) a` и вызвать `id(42)`.

---

### Фаза 6 — Dependent ADT + match

**Цель:** milestone `Vec[a, n]` + `append`.

- [x] **6.1** `core/inductive.rs`: declare/check positivity.
- [x] **6.2** Indexed families: `Vec : Type → Nat → Type` (seed в prelude).
- [x] **6.3** Constructor typing с index constraints (checker `infer_inductive_ctor`).
- [x] **6.4** Dependent pattern match + coverage check.
- [x] **6.5** Index unification в match branches.
- [x] **6.6** Prelude: `Nat`, `add`, `Vec`, `append` (surface или Core seed).
- [x] **6.7** `examples/tests/core/ok_vec_append.nia` green.

**Done when:** append typechecks с return type `Vec[a, add(n, m)]`.

---

### Фаза 7 — Termination

**Цель:** безопасная рекурсия в типах и программах.

- [x] **7.1** Structural recursion checker на `match`.
- [x] **7.2** `decreases` clause в surface (parser + elab).
- [x] **7.3** Mutual recursion groups (minimal).
- [x] **7.4** Mark partial/`Div` effect (stub if effects not ready).
- [x] **7.5** Negative test: non-structural recurse → error.

**Done when:** `append` passes termination; obvious infinite loop rejected.

---

### Фаза 8 — Erasure + ClassicalHir

**Цель:** tier 1 codegen через новый путь.

- [x] **8.1** `erase/mod.rs`: strip proofs, ghost, implicits; keep runtime data.
- [x] **8.2** `hir/classical.rs`: typed MIR (Let, Call, Load, Store, Branch, …).
- [x] **8.3** Lower tier-0 surface constructs to HIR.
- [x] **8.4** `backend/llvm/`: HIR → LLVM (tier 0).
- [x] **8.5** Dual pipeline в driver: `--new` flag для нового пути.
- [x] **8.6** Tier 1 `ok_*.nia` compile + run через `--new`.

**Done when:** tier 1 green на `--new`; старый pipeline ещё доступен для diff.

---

### Фаза 9 — Tier 2 parity

**Цель:** vectors, matrices, impl, strings, floats.

- [ ] **9.1** Desugar `vector` decls → prelude inductive + ops.
- [x] **9.2** Desugar `i32<N>`, `T<>` anon vectors. *(как fixed arrays + to_vec/to_array)*
- [x] **9.3** Desugar `T[]` matrices + `@`, `det`, `outer`. *(to_matrix/to_array/matrix_drop/println — да; `@`/det/outer — нет)*
- [x] **9.4** Desugar `impl` methods → `Type__method` functions.
- [x] **9.5** String, f16/f32/f64, Complex builtins в prelude. *(floats/string — да; Complex — нет)*
- [x] **9.6** Tier 2 examples green на `--new`.
- [x] **9.7** Удалить старый `semantics/typecheck/` (если tier 2 green). *(перенесён в `backend/legacy_typecheck.rs`, default = new pipeline)*

**Done when:** tier 2 green; old typechecker deleted.

---

### Фаза 10 — Refinements + VC (без SMT сначала)

**Цель:** `{ y != 0 }`, `requires`/`ensures`.

- [x] **10.1** Surface syntax: refinement types, requires/ensures.
- [x] **10.2** `core/term.rs`: `Refinement` variant.
- [x] **10.3** Guard generation при subtyping.
- [x] **10.4** VC datatype + pretty print for debugging.
- [x] **10.5** Manual proof stubs (`admit` with warning).
- [x] **10.6** `safe_div` example typechecks (admit or manual).

**Done when:** refinement typing works; VCs visible in `--dump-vc`.

---

### Фаза 11 — SMT discharge

**Цель:** автоматические доказательства arithmetic/guards.

- [x] **11.1** VC → SMT-LIB encoding (minimal: Int, Bool, =, <, +).
- [x] **11.2** Z3 subprocess manager (timeout, cache, logging).
- [x] **11.3** `safe_div` discharged automatically.
- [x] **11.4** Solver failure diagnostics с assumptions list.

**Done when:** `safe_div` without `admit`; solver errors readable.

---

### Фаза 12 — Effects

**Цель:** Tot / Ghost / IO / Div + quantum prep.

- [x] **12.1** `Computation` wrapper in Core (`Tot t`, `Ghost t`, …).
- [x] **12.2** Effect checking in elab for fn declarations.
- [x] **12.3** `quant { }` / `gpu { }` as effect scopes.
- [x] **12.4** Subeffect relation.

**Done when:** IO fn cannot be called from Tot context; quant scope enforced.

---

### Фаза 13 — QuantumHir + QIR

**Цель:** tier 3 quantum examples.

- [x] **13.1** Affine qubit typing in Core/HIR.
- [x] **13.2** `QuantumHir` with measure, gates, allocation.
- [x] **13.3** Port/adapt `backend/qir/mod.rs` to QuantumHir input.
- [x] **13.4** `examples/quantum/qubit_create.nia` → QIR → runner.
- [x] **13.5** Remaining quantum examples one by one.

**Done when:** tier 3 quantum subset green.

---

### Фаза 14 — Cleanup + default new pipeline

**Цель:** новый compiler = default.

- [x] **14.1** Remove `--new` flag; old pipeline deleted.
- [x] **14.2** Remove dead `ast/`, old codegen entry points.
- [x] **14.3** Update README (architecture section, not full tutorial).
- [x] **14.4** `--core-only`, `--dump-hir`, `--dump-vc` dev flags documented.
- [x] **14.5** CI runs tier 0..2 on every PR.

**Done when:** `cargo test` uses only new pipeline; README updated.

---

## Решения (заполнить до фазы 3)

| Вопрос | Решение | Дата |
|--------|---------|------|
| Universe hierarchy | Predicative `Universe(0..n)`; `Type:Type` запрещён | фаза 3 |
| Erasure of known indices | Compile-time константы стираются в erase (tier 0) | фаза 8 |
| Prelude location | Rust `elab/prelude.rs` + `nia_std::llvm_prelude` | фаза 4/8 |
| de Bruijn vs named in Core | **Levels** в Core; surface names только в `name_hint` | фаза 3 |
| Struct/enum в Core | Номинальные `DataCtor`/`DataMatch` + `DataEnv` (не full inductive families) | фаза 4 |
| Old pipeline removal gate | Tier 2 green; legacy driver path removed (фаза 14) | фаза 14 |
| SMT solver | Z3 subprocess в `verify/` only | TBD |

---

## Антипаттерны для vibe-sessions

**Не просить AI:**

- «Добавь dependent types в enum Ty» — только SurfaceTy на surface.
- «Сравни типы через to_string()» — только NbE + definitional equality.
- «Вызови Z3 из types_equal» — SMT только в verify/.
- «Сразу перепиши codegen» — сначала `--core-only`, потом HIR.
- «Сделай quantum + refinements + matrices в одном PR» — один tier/milestone.

**Всегда просить:**

- unit tests для нового модуля;
- negative test на каждый новый typing rule;
- «не трогай FStar/ и QIR_REPOS_FOR_AI_CONTEXT/» (AGENTS.md).

---

## Vibe-session шаблон

```text
Контекст: ветка deptypes, задача X.Y из docs/rewrite-tasks.md.
Reference: docs/dependent-types-design.md §N.

Сделай:
1. [конкретный модуль]
2. unit tests
3. не ломай существующие cargo test

Не делай:
- [anti-patterns для этой фазы]
```

---

## Порядок vibe-работы (рекомендуемый)

```text
Неделя 1–2:  фазы 0 → 1 → 2
Неделя 3–5:  фаза 3 (Core + NbE + tests)
Неделя 6–8:  фазы 4 → 5 (tier 0 + implicits)
Неделя 9–12: фазы 6 → 7 (Vec/append milestone)
Месяц 4+:    фазы 8 → 9 (codegen parity)
Месяц 6+:    фазы 10–13 по необходимости
```

---

## Связанные документы

- [dependent-types-design.md](./dependent-types-design.md) — архитектура и rationale
- [spec.txt](../spec.txt) — surface syntax contract
- [vector-limitations.md](./vector-limitations.md) — текущие semantic rules (reference до tier 2)
- [quantum-instructions.md](./quantum-instructions.md) — quantum surface → QIR mapping

---

## Changelog

| Дата | Что |
|------|-----|
| 2026-06-09 | Создан документ на ветке `deptypes` |
| 2026-06-09 | Фаза 0 (0.1–0.4): fixtures, staged pipeline API, tier tests, `--core-only` |
| 2026-06-09 | Фаза 1: `frontend/surface`, `SurfaceTy`, `SurfaceModule`, `ast` re-exports |
| 2026-06-09 | Фаза 2: `frontend/resolve`, `ResolvedModule`, `--resolve-only`, tier resolve tests |
| 2026-06-09 | Фаза 3: `core/` Term + NbE + Checker, 35 unit tests |
| 2026-06-09 | Фаза 4: `elab/` surface→Core, tier-0 elab+check, `--elab-only` |
| 2026-06-09 | Фаза 8 (tier 0): `erase/`, `hir/`, `backend/llvm/`, `--new`, 403 tests |
| 2026-06-09 | Фаза 5: metavariables, implicit `#a: Type`, `id(42)`, implicit diagnostics |
| 2026-06-09 | Фаза 8.6: tier 1 на `--new` (arrays, ptrs, loops, heap), 408 tests |
| 2026-06-09 | Фаза 6 (начало): `core/inductive`, Nat/Vec seed, `ok_nat_add.nia`, 413 tests |
| 2026-06-09 | Фаза 6: dependent match, `append`, `ok_vec_append.nia`, 415 tests |
| 2026-06-09 | Фаза 7: `core/termination`, `decreases`/`partial`, seed + `err_nonstructural_recursion`, 419 tests |
| 2026-06-09 | Фаза 9 (частично): floats/string/impl/anonvec/println/gpu/quant на `--new`, 421 tests |
