# Полноценные зависимые типы в NiaLang

Статус: проектный документ.

Этот документ описывает архитектуру полноценной системы зависимых типов для
NiaLang, вдохновлённой F*. Речь идёт не только о массивах с длиной в типе, а о
системе, в которой:

- типы являются термами языка;
- результат функции может зависеть от значения аргумента;
- доступны зависимые функции и зависимые пары;
- определения участвуют в вычислительном равенстве типов;
- поддерживаются индуктивные семейства и зависимый pattern matching;
- refinement-типы порождают логические обязательства;
- эффекты являются частью типа вычисления;
- доказательные и ghost-компоненты стираются перед генерацией LLVM IR или QIR;
- оставшиеся логические обязательства могут автоматически решаться SMT-солвером.

Цель документа - определить целевую архитектуру и поэтапный путь миграции от
нынешнего компилятора NiaLang.

## 1. Что означает "полноценные зависимые типы"

В обычной системе типов тип функции не зависит от конкретного значения
аргумента:

```nia
fn id(x: i32) i32
```

В зависимой системе тип результата может содержать значение аргумента:

```nia
fn replicate(
    #a: Type,
    n: Nat,
    x: a,
) Vec[a, n]
```

Здесь `n` является одновременно обычным значением и частью типа результата.
Однако длина вектора - только простейший пример. Полноценная система должна
позволять зависимость от произвольных чистых термов:

```nia
fn lookup(
    #a: Type,
    schema: Schema,
    row: Row[schema],
    field: Field[schema],
) Value[field_type(schema, field)]
```

Или:

```nia
fn decode(
    format: Format,
    bytes: Bytes,
) Result[DecodedType[format], DecodeError[format]]
```

Главное свойство такой системы: между "типом" и "выражением" больше нет
фундаментальной синтаксической стены. Тип проверяется тем же ядром термов, что и
программа.

### 1.1. Обязательные возможности

Чтобы система действительно была полноценной, а не набором специальных
случаев, нужны:

1. Иерархия универсумов `Type 0`, `Type 1`, ...
2. Зависимые функции, то есть Pi-типы.
3. Зависимые пары, то есть Sigma-типы.
4. Лямбды, application и `let` внутри типов.
5. Индуктивные типы и индуктивные семейства.
6. Зависимый pattern matching.
7. Вычислительное, или definitionally equal, равенство.
8. Неявные аргументы и metavariables.
9. Унификация термов с учётом вычисления.
10. Проверка positivity индуктивных определений.
11. Проверка завершимости рекурсивных определений.
12. Refinement-типы и подтипирование.
13. Типизированные эффекты и спецификации вычислений.
14. Генерация verification conditions.
15. Автоматическое доказательство части условий.
16. Стирание доказательств и вычислительно незначимых индексов.

Если отсутствуют пункты 4, 5, 6 или 7, пользователь быстро упрётся в
ограничения, характерные для GADT или размерных типов, а не для полноценного
зависимого языка.

## 2. Что полезно перенять у F*

F* разделяет работу на несколько крупных слоёв:

```text
surface syntax
    -> desugaring и resolution
    -> единый зависимый Syntax term
    -> elaboration и type checking
    -> guards и verification conditions
    -> normalizer, tactics и SMT
    -> checked module
    -> extraction
```

Для NiaLang особенно важны следующие идеи.

### 2.1. Единое представление термов и типов

Во внутреннем представлении F* `typ` является псевдонимом `term`. Стрелки,
refinement-типы, universe instantiation, metavariables и вычисления находятся в
одном языке термов.

NiaLang должен прийти к той же модели. Нынешний `Ty` нельзя постепенно
превратить в полноценное зависимое ядро, просто добавляя варианты вроде
`Ty::DependentArray` или `Ty::Refinement`. Такое расширение оставит два
несогласованных языка: `Expr` для вычислений и `Ty` для типов.

### 2.2. Разделение surface AST и Core

Пользовательский синтаксис может быть богатым и удобным, но Core должен быть
маленьким и однозначным. Перегрузка операторов, методы, сокращённые записи
типов, implicit arguments и синтаксический сахар раскрываются до проверки Core.

### 2.3. Guards вместо немедленного ответа true/false

При сравнении зависимых типов не всегда можно сразу вернуть `true` или `false`.
Проверка может породить условие:

```text
n + 1 = m
i < n
pre_state satisfies owns(q)
```

F* собирает такие условия в guard, решает вычислимую часть унификацией и
нормализацией, а логический остаток превращает в VC.

### 2.4. Эффект входит в тип вычисления

Функция описывает не только результат, но и вид вычисления:

```text
Tot A
Ghost A
Lemma P
STATE A pre post
```

Для Nia это особенно ценно, потому что классический код, GPU-код и квантовый код
могут иметь разные эффекты и разные правила исполнения.

### 2.5. Extraction отделена от проверки

После проверки доказательства, refinements, ghost-аргументы и часть индексов
стираются. Backend получает уже проверенное вычислительное представление и не
должен повторно угадывать семантику типов.

## 3. Текущее состояние NiaLang

Сейчас типовая модель централизована в `src/ast/mod.rs`:

```rust
pub enum Ty {
    I32,
    Bool,
    Qubit,
    Result,
    Array(Box<Ty>, usize),
    Matrix(Box<Ty>, Option<(usize, usize)>),
    // ...
}
```

Парсер записывает аннотации непосредственно в `Ty`. Typechecker:

- нормализует имена через `normalize_ty`;
- хранит функции в `FnSig`;
- выводит выражения через `infer_expr`;
- сравнивает типы функцией `types_equal`;
- передаёт тот же AST напрямую в backend.

Это хорошая архитектура для простой номинальной системы типов, но она не может
служить ядром зависимой системы по следующим причинам:

1. Типы и выражения имеют разные AST.
2. Размеры представлены хостовым `usize`, а не термами Nia.
3. `types_equal` возвращает только `bool`.
4. Нет локального контекста зависимых binder-ов.
5. Нет metavariables и унификации.
6. Нет вычислительного равенства.
7. Нет universe checking.
8. Нет представления доказательств и propositions.
9. Нет разделения между вычислительными и ghost-значениями.
10. Backend получает нетипизированный surface AST.

Следовательно, полноценные зависимые типы требуют нового среднего слоя, а не
локальной доработки `Ty`.

## 4. Целевой конвейер компилятора

Предлагаемый конвейер:

```text
Source text
    -> Lexer
    -> Surface AST
    -> Name resolution
    -> Elaboration
    -> Dependently typed Core
    -> Core validation
    -> VC generation and discharge
    -> Checked HIR
    -> Erasure and monomorphization
    -> Classical MIR / Quantum IR
    -> LLVM IR / QIR
```

Каждый переход должен иметь явный контракт.

### 4.1. Surface AST

Surface AST сохраняет пользовательскую запись:

- имена вместо идентификаторов;
- сокращённые типы;
- implicit binders;
- методы;
- перегруженные операторы;
- пропущенные аргументы;
- пользовательские pattern-ы;
- синтаксис `requires`, `ensures`, `decreases`.

Surface AST не является доверенным и не передаётся backend-ам.

### 4.2. Resolved AST

После name resolution каждое имя ссылается на стабильный идентификатор:

```rust
struct DefId(u32);
struct LocalId(u32);
struct ConstructorId(u32);
struct EffectId(u32);
```

Строки остаются только для диагностики и pretty printing.

### 4.3. Core

Core содержит минимальный зависимый calculus. Все неявные аргументы уже
вставлены, перегрузка разрешена, pattern matching скомпилирован в явное
представление.

### 4.4. Checked HIR

Checked HIR содержит только программы, для которых:

- Core terms типизированы;
- все metavariables разрешены или явно обобщены;
- все обязательные VC доказаны;
- рекурсия признана завершающейся либо помечена частичным эффектом;
- эффекты согласованы;
- quantum/GPU capabilities проверены.

Backend должен принимать только Checked HIR или результат его стирания.

## 5. Зависимый Core

Ниже приведён минимальный ориентир, а не окончательная Rust API.

```rust
pub enum Term {
    Var(LocalVar),
    Global(DefId),

    Universe(Level),

    Pi {
        binder: Binder,
        body: Box<Term>,
    },
    Lam {
        binder: Binder,
        body: Box<Term>,
    },
    App {
        fun: Box<Term>,
        arg: Box<Term>,
        explicitness: Explicitness,
    },

    Sigma {
        binder: Binder,
        body: Box<Term>,
    },
    Pair {
        fst: Box<Term>,
        snd: Box<Term>,
        sigma_ty: Box<Term>,
    },
    Fst(Box<Term>),
    Snd(Box<Term>),

    Let {
        binder: Binder,
        value: Box<Term>,
        body: Box<Term>,
    },

    Data {
        datatype: DefId,
        universes: Vec<Level>,
        args: Vec<Term>,
    },
    Constructor {
        constructor: ConstructorId,
        universes: Vec<Level>,
        args: Vec<Term>,
    },
    Match(MatchTerm),

    Refinement {
        binder: Binder,
        predicate: Box<Term>,
    },

    Comp(Computation),

    Meta(MetaId),
    Error,
}
```

`Error` нужен только для восстановления после диагностик и никогда не должен
попадать в успешно проверенный модуль.

### 5.1. Binder

```rust
pub struct Binder {
    pub name_hint: String,
    pub ty: Box<Term>,
    pub explicitness: Explicitness,
    pub relevance: Relevance,
    pub multiplicity: Multiplicity,
}

pub enum Explicitness {
    Explicit,
    Implicit,
    Instance,
}

pub enum Relevance {
    Runtime,
    Erased,
}

pub enum Multiplicity {
    Zero,
    One,
    Many,
}
```

`Relevance` и `Multiplicity` решают разные задачи:

- erased binder участвует в проверке, но не существует во время исполнения;
- linear binder должен быть использован согласно правилам владения;
- обычный binder можно использовать многократно.

Не обязательно реализовывать multiplicities в первом Core milestone, но
структура binder-а не должна блокировать их последующее добавление.

### 5.2. Представление переменных

Для Core рекомендуется использовать de Bruijn levels или locally nameless
представление, а не строки.

Преимущества de Bruijn levels:

- alpha-equivalence становится структурной;
- substitution не зависит от имён;
- NbE проще реализовать;
- сериализация checked modules стабильнее.

В diagnostics можно хранить `name_hint` и source span.

### 5.3. Pi-типы

Обычная функция является частным случаем зависимой:

```text
(x : A) -> B
```

Если `B` не содержит `x`, получаем обычную стрелку `A -> B`.

Правило образования:

```text
Gamma |- A : Type u
Gamma, x : A |- B : Type v
--------------------------------
Gamma |- (x : A) -> B : Type (max u v)
```

Application подставляет аргумент в codomain:

```text
Gamma |- f : (x : A) -> B
Gamma |- a : A
--------------------------------
Gamma |- f a : B[a/x]
```

Именно эта подстановка делает функцию зависимой.

### 5.4. Sigma-типы

Sigma хранит значение вместе с компонентом, тип которого зависит от первого:

```nia
(x: A & B[x])
```

Пример:

```nia
type SomeVector(#a: Type) =
    (n: Nat & Vec[a, n])
```

Это не обычный tuple: тип второго поля зависит от первого поля.

### 5.5. Propositions и доказательства

Есть два разумных дизайна.

#### Вариант A: proposition как обычный тип

Используется Curry-Howard:

```text
P : Type 0
p : P
```

Доказательство является значением типа proposition.

#### Вариант B: отдельный `Prop`

`Prop` получает proof irrelevance и специальное стирание.

Для сходства с F* практичнее начать с propositions как типов плюс встроенный
proof-irrelevant контейнер:

```nia
type Squash(p: Type) = unit { p }
```

Пользовательские доказательства могут быть erased. Это позволяет SMT доказывать
существование свидетельства, не конструируя вычислительно значимый объект.

### 5.6. Мета-теоретические ограничения Core

Для первой версии следует выбрать интенсиональную теорию типов:

- definitional equality определяется только вычислением Core;
- propositional equality выражается типом `Eq`;
- equality reflection запрещено;
- function extensionality не является правилом conversion;
- proof irrelevance применяется только к явно erased/squashed proofs;
- `Type : Type` запрещено;
- произвольные `Div`-вычисления не раскрываются внутри типов.

Equality reflection позволило бы превращать любое доказанное равенство в
definitional equality. В сочетании с SMT это сделало бы conversion зависимым от
произвольного theorem proving и практически уничтожило бы его
предсказуемость. Extensionality при необходимости добавляется как lemma или
явное допущение, но не как скрытое правило normalizer-а.

## 6. Universes

Наивное правило `Type : Type` делает систему противоречивой. Нужна иерархия:

```text
Type 0 : Type 1
Type 1 : Type 2
...
```

Уровни:

```rust
pub enum Level {
    Zero,
    Succ(Box<Level>),
    Max(Vec<Level>),
    Param(UniverseParamId),
    Meta(UniverseMetaId),
}
```

Elaborator должен:

- создавать universe metavariables;
- собирать ограничения `u <= v`;
- решать их до завершения модуля;
- обобщать допустимые universe parameters;
- не позволять universe metavariable утечь в backend.

Первый релиз может печатать universes неявно:

```nia
fn id(#a: Type, x: a) a
```

Core всё равно должен хранить universe instantiation явно.

### 6.1. Cumulativity

Желательно поддержать:

```text
Type u <: Type (u + 1)
```

Но cumulativity усложняет conversion и subtyping. Её можно включить после
стабилизации инвариантной universe-полиморфной версии. Важно не кодировать
предположение о cumulativity в десятках мест: это должно быть одним правилом
relation solver-а.

## 7. Elaboration

Elaboration переводит удобный surface syntax в полностью явный Core.

Главный интерфейс должен быть bidirectional:

```rust
fn infer(ctx: &Context, expr: &SurfaceExpr)
    -> Result<(Term, Term, Guard), Diagnostic>;

fn check(ctx: &Context, expr: &SurfaceExpr, expected: &Term)
    -> Result<(Term, Guard), Diagnostic>;
```

`infer` возвращает elaborated term, его тип и guard. `check` проверяет выражение
против ожидаемого типа.

### 7.1. Почему bidirectional checking

Полный вывод зависимых типов в общем случае неразрешим. Пользователь иногда
должен дать аннотацию, а elaborator использует направление проверки:

- lambda обычно проверяется против ожидаемого Pi-типа;
- application обычно выводит тип функции;
- constructor проверяется против ожидаемого индуктивного семейства;
- `match` использует тип scrutinee и ожидаемый motive.

Это даёт предсказуемую систему без обещания невозможного полного inference.

### 7.2. Metavariables

Пропущенные implicit arguments представлены metavariables:

```rust
pub struct MetaVar {
    pub context: LocalContext,
    pub expected_ty: Term,
    pub solution: Option<Term>,
    pub origin: SourceSpan,
}
```

Пример:

```nia
append(xs, ys)
```

Core может стать:

```text
append {?a} {?n} {?m} xs ys
```

Унификация решает `?a`, `?n`, `?m` из типов аргументов.

Каждая metavariable обязана помнить контекст создания. Иначе можно получить
решение с захватом переменной, которой нет в допустимой области видимости.

### 7.3. Implicit arguments

Предлагаемый surface syntax:

```nia
fn id(#a: Type, x: a) a
fn map(#a: Type, #b: Type, f: a -> b, xs: List[a]) List[b]
```

`#a` является implicit binder. Пользователь может при необходимости передать
его явно:

```nia
id[#a = i32](10)
```

### 7.4. Typeclass или instance arguments

Instance arguments лучше строить поверх implicit arguments, а не делать
отдельной системой:

```nia
fn equal(#a: Type, #[instance] eq: Eq[a], x: a, y: a) bool
```

Instance search порождает metavariable специального класса и ищет значение в
локальном и глобальном окружении.

## 8. Вычислительное равенство

Зависимые типы требуют отличать:

- синтаксическое равенство;
- propositional equality, которое требует доказательства;
- definitional equality, устанавливаемое вычислением.

Например:

```text
Vec[A, 1 + 1]
Vec[A, 2]
```

могут быть definitionally equal после нормализации.

### 8.1. Редукции

Минимальный набор:

- beta: `(fun x => t) a` редуцируется в `t[a/x]`;
- zeta: `let x = a in t` редуцируется в `t[a/x]`;
- iota: `match Constructor(...)` выбирает ветку;
- delta: раскрытие глобального определения согласно transparency policy;
- projection: `(a, b).1` и `(a, b).2`.

### 8.2. NbE

Для conversion checking рекомендуется normalization by evaluation:

```text
Term -> semantic Value -> quoted normal Term
```

Преимущества:

- не требуется многократно выполнять наивные substitution;
- нейтральные термы представляются явно;
- сравнение функций выполняется через применение к fresh neutral;
- проще контролировать weak-head и полную нормализацию.

Нужны как минимум:

```rust
fn eval(term: &Term, env: &Env) -> Value;
fn quote(level: usize, value: &Value) -> Term;
fn conv(ctx: &Context, left: &Term, right: &Term) -> ConvResult;
```

### 8.3. Transparency

Не все определения нужно всегда раскрывать. Следует ввести режимы:

```rust
pub enum Transparency {
    Reducible,
    Semireducible,
    Irreducible,
}
```

Публичная абстракция модуля требует, чтобы implementation закрытого определения
не использовалась при проверке клиентов.

## 9. Унификация

Унификация решает уравнения между термами, содержащими metavariables:

```text
?F x = Vec[A, x]
```

Полная higher-order unification неразрешима. Практичный elaborator должен:

1. Полностью поддерживать first-order случаи.
2. Поддерживать higher-order pattern unification.
3. Откладывать сложные flex-rigid и flex-flex constraints.
4. После дополнительных подстановок пытаться решить их повторно.
5. В неразрешимом случае требовать аннотацию пользователя.

Нельзя маскировать провал унификации безусловной отправкой формулы в SMT:
metavariable определяет структуру Core term, а не только логическую истинность.

## 10. Refinement-типы

Refinement:

```nia
x: A { P(x) }
```

означает значения `A`, удовлетворяющие `P`.

Примеры:

```nia
type Nat = x: i64 { x >= 0 }
type NonZero = x: i64 { x != 0 }

fn div(x: i64, y: NonZero) i64
```

### 10.1. Правило введения

Чтобы проверить `e` против `x:A{P(x)}`:

1. Проверить `e : A`.
2. Породить обязательство `P(e)`.

### 10.2. Правило использования

Если в контексте есть:

```text
x : y:A{P(y)}
```

то typechecker может считать доступными:

```text
x : A
P(x)
```

### 10.3. Подтипирование

Refinement требует relation:

```text
x:A{P(x)} <: x:A{Q(x)}
```

если доказуемо:

```text
forall x:A. P(x) ==> Q(x)
```

Это уже не structural equality. Поэтому нынешний `types_equal` должен быть
заменён набором операций:

```rust
fn definitional_eq(ctx, a, b) -> ConvResult;
fn subtype(ctx, a, b) -> Guard;
fn check_has_type(ctx, term, inferred, expected) -> Guard;
```

## 11. Guards и verification conditions

Результат проверки должен включать не только тип:

```rust
pub struct Guard {
    pub formula: Formula,
    pub deferred: Vec<Constraint>,
    pub universe_constraints: Vec<UniverseConstraint>,
    pub implicits: Vec<MetaId>,
    pub labels: Vec<GuardLabel>,
}
```

Guard объединяется конъюнкцией и закрывается по binder-ам при выходе из scope.

Пример:

```nia
fn head(#a: Type, xs: List[a] { length(xs) > 0 }) a
```

Вызов:

```nia
head(1 :: rest)
```

может породить VC:

```text
length(1 :: rest) > 0
```

Нормализатор сведёт это к `1 + length(rest) > 0`, после чего арифметический
solver докажет условие.

### 11.1. Порядок решения guard

Рекомендуемый порядок:

1. Подстановка уже решённых metavariables.
2. Definitional normalization.
3. Унификация типов и computations.
4. Universe constraints.
5. Instance search.
6. Пользовательские tactics.
7. Упрощение логической формулы.
8. SMT для оставшейся first-order части.

Этот порядок важен: SMT не должен получать условия, которые на самом деле
являются задачей elaboration.

## 12. Эффекты и computation types

Полноценный аналог F* требует различать тип значения и тип вычисления.

```rust
pub struct Computation {
    pub effect: EffectId,
    pub result: Box<Term>,
    pub args: Vec<Term>,
    pub flags: Vec<CompFlag>,
}
```

Базовые computation forms:

```text
Tot A
Ghost A
Div A
Lemma P
Comp E A args
```

### 12.1. Первый набор эффектов

Начинать следует со встроенных эффектов:

- `Tot A`: чистое завершающееся вычисление;
- `Ghost A`: чистое вычисление, стираемое из runtime;
- `Div A`: потенциально незавершающееся вычисление;
- `Lemma P`: erased-доказательство;
- `IO A`: обычные внешние эффекты;
- `Quantum A pre post caps`: квантовое вычисление;
- `Gpu A contract`: GPU-вычисление.

После стабилизации можно открыть пользовательские эффекты.

### 12.2. Dijkstra monads

F*-подобная модель эффектов использует weakest preconditions. Вычисление:

```text
STATE A wp
```

содержит transformer, описывающий, какое precondition достаточно для
требуемого postcondition.

Для Nia можно начать с явного Hoare-style представления:

```nia
effect State[s: Type] (
    a: Type,
    requires: s -> Prop,
    ensures: s -> a -> s -> Prop,
)
```

Позже это можно обобщить до пользовательских effect combinators:

- `return`;
- `bind`;
- `subcomp`;
- `if_then_else`;
- `close`;
- lifts между эффектами.

Проверка законов пользовательского эффекта сама порождает VC.

### 12.3. Effect inference

Не следует пытаться сразу полностью выводить произвольные пользовательские
эффекты. Первый вариант может требовать явную аннотацию функции, а внутри тела:

- объединять эффекты последовательности через `bind`;
- применять известные lifts;
- проверять ожидаемый computation type bidirectionally.

## 13. Индуктивные типы и семейства

Surface syntax:

```nia
data Nat: Type {
    Zero: Nat,
    Succ: Nat -> Nat,
}

data Vec(#a: Type): Nat -> Type {
    Nil: Vec[a, Zero],
    Cons: (#n: Nat, head: a, tail: Vec[a, n]) -> Vec[a, Succ(n)],
}
```

Это не специальные встроенные векторы. `Vec` является обычным индуктивным
семейством, индексированным `Nat`.

### 13.1. Проверка декларации

Для каждого `data` необходимо:

1. Проверить тип параметров и индексов.
2. Проверить тип каждого конструктора.
3. Проверить strict positivity рекурсивных вхождений.
4. Проверить universe consistency.
5. Сгенерировать eliminator или внутреннее описание pattern matching.
6. Сгенерировать constructor metadata, projectors и discriminators.

### 13.2. Strict positivity

Следующее определение недопустимо:

```nia
data Bad: Type {
    Mk: (Bad -> i32) -> Bad,
}
```

`Bad` встречается слева от стрелки, что позволяет построить парадоксы.

Positivity checker должен анализировать polarity рекурсивных вхождений после
контролируемого раскрытия type aliases.

### 13.3. Зависимый pattern matching

Для:

```nia
fn head(#a: Type, #n: Nat, xs: Vec[a, Succ(n)]) a {
    match xs {
        Cons(head, _) => head,
    }
}
```

ветка `Nil` невозможна из-за индекса `Succ(n)`.

Pattern compiler должен построить motive:

```text
(n: Nat) -> Vec[a, n] -> Type
```

и уточнить индексы внутри каждой ветки. Обычная проверка всех веток против
одного неизменного типа здесь недостаточна.

Первый вариант можно ограничить:

- require explicit return annotation for dependent matches;
- поддерживать только constructor patterns;
- не поддерживать nested dependent patterns до отдельной компиляции pattern
  matrix.

## 14. Propositional equality

Нужен обычный индуктивный equality type:

```nia
data Eq(#a: Type, x: a): a -> Type {
    Refl: Eq[a, x, x],
}
```

С ним можно выражать равенство, которое не является definitional:

```nia
fn plus_zero_right(n: Nat) Eq[Nat, add(n, Zero), n]
```

Eliminator equality, часто называемый `transport` или `subst`, позволяет
переносить значения между зависимыми типами:

```nia
fn transport(
    #a: Type,
    p: a -> Type,
    #x: a,
    #y: a,
    eq: Eq[a, x, y],
    value: p(x),
) p(y)
```

Compiler не должен неявно считать propositionally equal типы definitionally
equal. Переход требует evidence или coercion, вставленной elaborator-ом.

## 15. Рекурсия и termination

Если произвольная незавершающаяся функция может вычисляться на уровне типов,
conversion checking перестаёт завершаться, а логика становится
противоречивой.

Поэтому определения, доступные в типах и доказательствах, должны быть total.

### 15.1. Structural recursion

Первый checker должен распознавать рекурсивные вызовы на структурно меньшем
аргументе:

```nia
fn length(#a: Type, xs: List[a]) Nat decreases xs {
    match xs {
        Nil => Zero,
        Cons(_, tail) => Succ(length(tail)),
    }
}
```

### 15.2. Well-founded recursion

Затем можно добавить:

```nia
fn sort(xs: List[i32]) List[i32]
    decreases length(xs)
```

Каждый рекурсивный вызов порождает VC, что мера строго уменьшается.

### 15.3. Частичные функции

Функции без доказательства завершимости получают эффект `Div`:

```nia
fn server_loop(...) Div[unit]
```

`Div`-вычисления нельзя без ограничений исполнять при normalization типов.

### 15.4. Termination и quantum-код

QIR Base Profile требует статически конечной структуры программы. Quantum
функция для Base должна быть total и после специализации иметь конечное число
операций. Adaptive Profile может разрешать циклы только при соответствующей
capability и отдельной проверке target profile.

## 16. SMT-интеграция

SMT нужен для автоматизации refinements и эффектных спецификаций, но не для
вычислительного равенства Core.

### 16.1. Что можно отправлять SMT

Хорошие кандидаты:

- first-order логика;
- равенства и неравенства целых чисел;
- algebraic datatypes;
- uninterpreted functions;
- quantified lemmas с контролируемыми patterns;
- predicates из refinements;
- pre/postconditions эффектов.

Не следует напрямую отправлять:

- нерешённые metavariables elaborator-а;
- произвольные higher-order функции без encoding;
- conversion problems;
- universe inconsistencies;
- positivity и coverage.

### 16.2. SMT pipeline

```text
Core proposition
    -> normalization
    -> lambda lifting / closure
    -> first-order encoding
    -> assumptions from context
    -> labeled assertion
    -> solver query
    -> SAT / UNSAT / UNKNOWN
```

Для доказательства `P` обычно проверяется невыполнимость:

```text
context assumptions /\ not P
```

`UNSAT` означает, что VC доказан.

### 16.3. Граница доверия

Возможны два режима.

#### Практичный F*-подобный режим

Компилятор доверяет корректности ответа `UNSAT` выбранного SMT solver-а. В
checked artifact сохраняются:

- формула VC;
- версия encoder-а;
- версия solver-а;
- hash query;
- использованные assumptions.

Это проще и соответствует практической модели F*.

#### Proof-producing режим

Solver должен вернуть proof certificate, который проверяется отдельным
checker-ом. Это уменьшает trusted computing base, но значительно увеличивает
сложность.

Для Nia рекомендуется сначала практичный режим, но API solver-а следует сделать
заменяемым, чтобы позже добавить certificates.

### 16.4. Поведение UNKNOWN и TIMEOUT

`UNKNOWN`, timeout или crash не являются доказательством. Компиляция должна:

- завершаться ошибкой;
- показывать source label VC;
- позволять увеличить лимит;
- позволять сохранить SMT query;
- не превращать timeout в warning для обязательного proof.

### 16.5. `assume` и `admit`

Нужны явные escape hatches:

```nia
assume fn external_axiom(...): P
admit P
```

Они должны:

- быть заметны в diagnostics;
- попадать в metadata checked module;
- поддерживать режим `--deny-assumptions`;
- быть запрещаемыми для production/verified build profile.

## 17. Tactics и reflection

Tactics не нужны для первого работающего dependent Core, но архитектура не
должна их исключать.

Нужны:

- quoted representation Core terms;
- безопасные constructors/destructors term views;
- proof state;
- goals с локальным контекстом;
- tactic monad;
- API создания metavariables и назначения решений;
- повторная проверка сгенерированного Core.

Главное правило: tactic может генерировать term, но результат всегда проходит
обычный Core checker. Нативный plugin не должен иметь возможность положить в
checked module непроверенный term.

## 18. Модули, интерфейсы и абстракция

Полноценная система должна поддерживать отдельные интерфейсы:

```nia
interface Counter {
    type T: Type
    val zero: T
    val next: T -> T
}
```

Implementation может скрывать representation. Клиент проверяется только против
экспортируемой сигнатуры.

Checked interface должна содержать:

- universe-полиморфные типы exported definitions;
- прозрачность определений;
- inductive metadata;
- effect declarations;
- подтверждённые lemmas;
- список assumptions;
- hash зависимостей.

Backend implementation details в интерфейс не попадают.

## 19. Стирание

После полной проверки Core нельзя напрямую передавать LLVM backend-у.
Необходим отдельный erasure pass.

### 19.1. Что стирается

- universe arguments;
- erased binders;
- proofs;
- `Lemma`;
- refinement predicates;
- equality evidence, если оно не влияет на runtime;
- ghost computations;
- индексы индуктивных семейств, не используемые вычислительно;
- type applications.

Пример:

```nia
fn get(
    #a: Type,
    #n: Nat,
    i: Nat { i < n },
    xs: Vec[a, n],
) a
```

после erasure может стать:

```text
get(i, xs)
```

Доказательство `i < n`, тип `a` и индекс `n` могут исчезнуть.

### 19.2. Relevance checking

Erased value нельзя использовать для вычисления runtime-результата:

```nia
fn bad(#erased x: i32) i32 {
    x + 1
}
```

Relevance checker должен отклонить такую программу до erasure.

### 19.3. Representation independence

Типы, которые стираются в одинаковое runtime representation, всё равно остаются
разными в Core. Backend получает явные coercions либо уже унифицированное
представление.

## 20. Полиморфизм, специализация и LLVM

LLVM не поддерживает зависимые или полиморфные типы напрямую. После erasure
нужна стратегия:

1. Мономорфизация для concrete runtime types.
2. Dictionary passing для typeclasses.
3. Uniform representation для части erased polymorphism.
4. Boxing только там, где специализация невозможна или слишком дорога.

Первый вариант Nia может использовать мономорфизацию:

```nia
id[i32](1)
id[bool](true)
```

порождает две runtime-специализации.

Нельзя мономорфизировать по каждому proof term: erased arguments не участвуют в
ключе специализации.

## 21. Зависимые типы и квантовое представление

Квантовый слой является хорошим первым крупным клиентом зависимой системы, но
его нельзя сводить только к размеру регистра.

Нужно описывать:

- доступные квантовые ресурсы;
- состояние владения ими;
- фазу вычисления;
- допустимые capabilities backend-а;
- зависимость последующего computation от результата измерения;
- соответствие QIR profile.

### 21.1. Вариант 1: indexed state effect

```nia
type QWorld: Type

effect Quantum(
    a: Type,
    pre: QWorld,
    post: a -> QWorld,
)
```

Тогда:

```nia
val h:
    (#w: QWorld, q: LiveQubit[w]) ->
    Quantum[unit, w, fun _ => apply_h(w, q)]
```

Базовое измерение возвращает opaque result и evidence о связи результата с
предыдущим миром:

```nia
val measure:
    (#w: QWorld, q: LiveQubit[w]) ->
    Quantum[
        (r: QResult & MeasurementEvidence[w, q, r]),
        w,
        fun measured => measured_world(w, q, measured.1)
    ]
```

В Adaptive Profile отдельная операция чтения связывает opaque result с
классическим значением:

```nia
val read_result:
    (#p: QirProfile,
     #[proof] adaptive: Supports[p, ReadMeasurement],
     r: QResult) ->
    (b: bool & ResultValue[r, b])
```

Тип continuation после `read_result` может зависеть от полученного `b`. Для
Base Profile такое evidence построить нельзя, поэтому branching по результату
не типизируется.

### 21.2. Вариант 2: separation logic effect

В стиле Pulse состояние описывается ресурсными assertions:

```nia
type QSlProp: Type
type Owns(q: Qubit, state: QState): QSlProp

effect QST(
    a: Type,
    requires: QSlProp,
    ensures: a -> QSlProp,
)
```

Гейт:

```nia
val h(q: Qubit):
    QST[
        unit,
        Owns(q, psi),
        fun _ => Owns(q, H(psi))
    ]
```

`psi` здесь не обязан быть буквальным вектором комплексных амплитуд. Для
практического компилятора это обычно абстрактный логический predicate:
подготовленное состояние, принадлежность к basis, entanglement relation или
другой контракт библиотеки. Попытка всегда хранить точное квантовое состояние в
типах привела бы к экспоненциальным термам и не должна быть обязательной частью
Core.

Двухкубитный гейт требует раздельного владения:

```nia
val cnot(control: Qubit, target: Qubit):
    QST[
        unit,
        Owns(control, c) * Owns(target, t),
        fun _ => Entangled(control, target)
    ]
```

Если separating conjunction `*` не позволяет дублировать ownership, вызов
`cnot(q, q)` становится недоказуемым.

### 21.3. Почему одних dependent types недостаточно

Обычный dependent calculus остаётся нелинейным:

```nia
let q2 = q
```

создаёт вторую ссылку на то же значение. Зависимость типов сама по себе не
запрещает aliasing.

Есть три решения:

1. Линейные binder-ы в Core.
2. Affine checker поверх Core.
3. Separation logic, где копировать handle можно, но нельзя скопировать
   доказательство владения ресурсом.

Для Nia рекомендуется комбинация:

- affine surface semantics для `qubit`, чтобы ошибки были простыми;
- QST/separation effect для формальной спецификации сложных quantum functions;
- dependent types для состояний, результатов и backend capabilities.

### 21.4. QIR profiles как типовые capabilities

```nia
data QirProfile {
    Base,
    Adaptive,
}

type Supports(profile: QirProfile, capability: Capability): Prop
```

Квантовая функция может иметь контракт:

```nia
quant fn feedback(
    #p: QirProfile,
    #[proof] adaptive: Supports[p, MidCircuitMeasurement],
    q: Qubit,
) Quantum[p, unit]
```

При target `Base` proof obligation неразрешим, поэтому программа отклоняется до
QIR lowering.

### 21.5. Base Profile

Base Profile удобно выразить typestate-фазами:

```nia
data Phase {
    Unitary,
    Measuring,
    Output,
}
```

Переходы:

```text
gate:       Quantum[Base, Unitary, Unitary]
measure:    Quantum[Base, Unitary|Measuring, Measuring]
record:     Quantum[Base, Measuring|Output, Output]
```

Обратного перехода из `Measuring` в `Unitary` нет. Следовательно, гейт после
первого измерения не типизируется.

### 21.6. Adaptive Profile

Adaptive разрешает:

- mid-circuit measurement;
- чтение `result` как `bool`;
- зависимое ветвление;
- в зависимости от capabilities, классическую арифметику, циклы и функции.

Тип ветвления может зависеть от результата:

```nia
bind(measure(q), fun measured =>
    if measured.1
    then x(q)
    else pure(())
)
```

### 21.7. Стирание в Quantum IR

После проверки:

- `QWorld`;
- ownership proofs;
- profile evidence;
- phase indices;
- pre/postconditions

стираются.

Quantum IR содержит только:

- resource ids;
- gates;
- measurements;
- classical control flow, разрешённый profile;
- output records;
- вычисленные capability flags.

Таким образом QIR backend остаётся простым и не реализует повторный theorem
prover.

## 22. Предлагаемая структура исходников

```text
src/
  surface/
    ast.rs
    parser.rs

  resolve/
    names.rs
    module_graph.rs

  core/
    term.rs
    level.rs
    context.rs
    value.rs
    subst.rs
    pretty.rs
    serialize.rs

  elab/
    infer.rs
    check.rs
    meta.rs
    unify.rs
    implicit.rs
    patterns.rs

  typecheck/
    conversion.rs
    relation.rs
    inductive.rs
    positivity.rs
    termination.rs
    universe.rs
    effects.rs
    guard.rs

  normalize/
    nbe.rs
    primitives.rs

  verify/
    formula.rs
    simplify.rs
    smt_encode.rs
    solver.rs
    tactics.rs

  checked/
    module.rs
    interface.rs
    cache.rs

  erase/
    relevance.rs
    erase.rs
    specialize.rs

  hir/
    classical.rs
    quantum.rs
    gpu.rs

  backend/
    llvm/
    qir/
```

Это не требует немедленно физически перемещать все существующие файлы.
Структура задаёт границы ответственности, к которым можно мигрировать
постепенно.

## 23. Миграция существующего компилятора

Переписывание всего компилятора одним коммитом слишком рискованно. Нужен
параллельный путь.

### Этап 0. Зафиксировать семантику

- Расширить parser/typechecker golden tests.
- Зафиксировать текущие diagnostics, которые важны пользователю.
- Отделить backend tests от frontend tests.
- Убедиться, что существующие изменения пользователя не затрагиваются.

### Этап 1. Ввести SurfaceTy

Переименовать смысл нынешнего `Ty` в surface representation:

```rust
enum SurfaceTy { ... }
```

На этом этапе поведение не меняется. Цель - перестать считать этот enum
финальным типовым ядром.

### Этап 2. Минимальный Core без refinements

Реализовать:

- universes;
- variables;
- Pi;
- lambda;
- application;
- let;
- globals;
- primitive constants.

Добавить Core checker и NbE. Первые программы могут быть элаборированы в Core и
затем снова стираться в существующий HIR.

### Этап 3. Bidirectional elaborator

- `infer` и `check`;
- implicit parameters;
- metavariables;
- first-order и pattern unification;
- source spans и diagnostics.

После этого обычные функции Nia должны проходить через новый elaborator.

### Этап 4. Индуктивные типы

- обычные algebraic data types;
- parameters;
- indices;
- positivity;
- constructor checking;
- dependent match;
- coverage.

Существующие `struct` и `enum` можно desugar в inductive declarations.

### Этап 5. Termination

- structural recursion;
- mutual recursion;
- `decreases`;
- эффект `Div`.

Без этого нельзя безопасно раскрывать пользовательские функции в типах.

### Этап 6. Propositions и refinements

- equality;
- erased proofs;
- refinement types;
- subtyping;
- guards;
- `requires` и `ensures`;
- `Lemma`.

На этом этапе Nia впервые получает F*-подобную верификацию без SMT или с очень
ограниченным встроенным solver-ом.

### Этап 7. SMT

- formula IR;
- encoding;
- Z3 process manager;
- labels;
- timeouts;
- query logging;
- cache;
- assumptions report.

### Этап 8. Эффекты

- `Tot`, `Ghost`, `Div`, `IO`;
- computation types;
- effect lifts;
- pre/postconditions;
- затем пользовательские effects.

### Этап 9. Erasure и новый backend contract

Backend перестаёт принимать surface AST. Вводятся:

```rust
CheckedModule
ErasedModule
ClassicalHir
QuantumHir
```

Текущий LLVM backend адаптируется к `ClassicalHir`.

### Этап 10. Quantum effect

- affine qubits;
- QST assertions;
- Base/Adaptive profile indices;
- зависимые measurement continuations;
- erasure в `QuantumHir`;
- реальный вызов `backend::qir::emit_module` из pipeline.

### Этап 11. Reflection и tactics

- quotations;
- proof state;
- interpreted tactics;
- optional native plugins.

### Этап 12. Checked modules и bootstrap

- бинарный cache проверенных модулей;
- stable interface hash;
- incremental checking;
- в далёкой перспективе возможность писать части компилятора на Nia.

## 24. Диагностика

Зависимый typechecker легко выдаёт нечитаемые ошибки. Diagnostics являются
частью дизайна, а не последующей полировкой.

Ошибка должна показывать:

1. Пользовательский expected type.
2. Пользовательский inferred type.
3. Нормализованную разницу только при необходимости.
4. Source origin неразрешённой metavariable.
5. Контекст assumptions для VC.
6. Причину появления обязательства.
7. Конкретный solver status.

Пример:

```text
type error: cannot apply `head`
  expected an argument of type:
    xs: List[i32] { length(xs) > 0 }
  found:
    xs: List[i32]

unproved obligation:
  length(xs) > 0

introduced by:
  precondition of `head`
```

Внутренние `Meta(17)` и de Bruijn indices не должны попадать в обычную ошибку.

## 25. Проверка корректности реализации

### 25.1. Unit tests

- substitution и shifting;
- alpha-equivalence;
- beta/iota/zeta/delta reduction;
- quoting после NbE;
- universe solving;
- occurs check;
- scope check metavariables;
- relevance;
- positivity;
- structural termination.

### 25.2. Property tests

- normalization idempotent;
- well-typed substitution preserves typing;
- `quote(eval(t))` definitionally equal `t`;
- successful unification даёт равные после substitution термы;
- erasure не оставляет proof variables;
- pretty-print/parse round trip для Core debug syntax.

### 25.3. Negative tests

Обязательно проверять отклонение:

- `Type : Type`;
- escaping metavariable;
- ill-scoped solution;
- occurs-check cycle;
- negative recursive datatype;
- non-terminating total definition;
- runtime use of erased proof;
- недоказанный refinement;
- `admit` в strict build;
- quantum ownership duplication;
- Base gate after measurement.

### 25.4. Golden tests

Golden fixtures должны хранить:

- elaborated Core;
- normalized type;
- generated VC;
- erased HIR;
- конечный LLVM/QIR для небольших примеров.

## 26. Производительность

Главные риски:

- повторная нормализация больших термов;
- чрезмерное delta unfolding;
- quadratic substitution;
- слишком общий unifier;
- огромный SMT context;
- повторная проверка зависимостей.

Нужны:

- hash-consing или устойчивые term ids;
- memoized weak-head normalization;
- sharing semantic values в NbE;
- transparency control;
- incremental context snapshots;
- cache checked interfaces;
- solver push/pop;
- pruning неиспользуемых assumptions;
- query hashes и proof hints.

Оптимизации нельзя внедрять ценой изменения definitional equality.

## 27. Граница доверия

Trusted computing base первого практичного варианта:

- parser не входит в TCB, если Core перепроверяется;
- name resolver не входит в TCB;
- elaborator может быть вне TCB только если все coercions, refinements,
  effect transitions и proof obligations представлены в Core явно и
  перепроверяются Core checker-ом; иначе соответствующая часть elaborator-а
  входит в TCB;
- Core checker входит в TCB;
- NbE/conversion входит в TCB;
- positivity и termination checker входят в TCB;
- effect checker входит в TCB;
- SMT encoder входит в TCB;
- SMT solver фактически входит в TCB в F*-подобном режиме;
- erasure correctness критична для соответствия runtime проверенной программе;
- LLVM/QIR backend критичен для исполнения, хотя не для логической типизации
  исходного Core.

Если в будущем появятся proof certificates и формальная проверка erasure, TCB
можно уменьшить.

## 28. Решения, которые следует принять заранее

Перед реализацией нужны явные ответы:

| Вопрос | Рекомендация |
|---|---|
| Типы и термы имеют один Core AST? | Да |
| Проверка bidirectional? | Да |
| Представление bound variables | De Bruijn levels |
| Нормализация | NbE |
| Полная higher-order unification? | Нет, pattern fragment + deferred constraints |
| Universe hierarchy | Да |
| Cumulativity | Позже, через relation layer |
| Proof irrelevance | Через erased/squashed propositions |
| Рекурсия в типах | Только доказанно total |
| SMT решает conversion? | Нет |
| SMT решает refinements/VC? | Да |
| Backend принимает surface AST? | Нет |
| Линейность следует из dependent types? | Нет |
| Quantum ownership | Affine checker + QST effect |
| Первая extraction strategy | Мономорфизация |
| `admit` допустим? | Только явно и с отчётом |

## 29. Антипаттерны

### 29.1. Добавить `Expr` внутрь нынешнего `Ty`

```rust
Ty::Dependent(Box<Expr>)
```

Это создаст циклические зависимости между двумя несогласованными AST и
разнесёт substitution по всему компилятору.

### 29.2. Сравнивать типы после печати в строки

Строковое равенство ломается на alpha-renaming, implicit arguments и
нормализации.

### 29.3. Вызывать Z3 из `types_equal`

Conversion должно быть детерминированным вычислением Core. SMT предназначен для
propositional obligations.

### 29.4. Разрешить общую рекурсию в `Tot`

Это делает typechecking потенциально незавершающимся и разрушает логическую
состоятельность.

### 29.5. Считать proof erased без relevance checking

Тогда runtime может зависеть от значения, которое backend удалил.

### 29.6. Оставить повторную семантическую проверку backend-у

Если QIR backend сам выясняет размеры, aliasing и допустимость profile, Core
гарантии перестают быть единым источником истины.

## 30. Минимальный убедительный результат

Первый milestone, который действительно демонстрирует полноценные зависимые
типы, должен поддерживать не один специальный `Array[N]`, а пользовательское
индуктивное семейство:

```nia
data Nat: Type {
    Zero: Nat,
    Succ: Nat -> Nat,
}

data Vec(#a: Type): Nat -> Type {
    Nil: Vec[a, Zero],
    Cons: (#n: Nat, head: a, tail: Vec[a, n]) -> Vec[a, Succ(n)],
}

fn append(
    #a: Type,
    #n: Nat,
    #m: Nat,
    xs: Vec[a, n],
    ys: Vec[a, m],
) Vec[a, add(n, m)] {
    match xs {
        Nil => ys,
        Cons(head, tail) => Cons(head, append(tail, ys)),
    }
}
```

Для этого уже необходимы:

- universes;
- Pi;
- implicit arguments;
- inductive families;
- dependent match;
- normalization `add`;
- recursive termination;
- elaboration constructor indices.

Следующий milestone добавляет:

```nia
fn safe_div(x: i64, y: i64 { y != 0 }) i64
```

и автоматическое доказательство precondition.

Третий milestone использует ту же общую систему в quantum effect:

```nia
quant fn bell(
    q0: Qubit,
    q1: Qubit,
) QST[
    (Result & Result),
    OwnsZero(q0) * OwnsZero(q1),
    fun results => Correlated(results.1, results.2)
]
```

Если эти три примера реализованы через один Core, а не через три набора
специальных правил, NiaLang действительно получил фундамент полноценного
зависимо типизированного языка.

## 31. Итоговая рекомендация

Полноценные зависимые типы в NiaLang следует реализовывать как новый
проверяемый Core, а не как расширение enum `Ty`.

Критический путь:

```text
единый Term
    -> universes и Pi/Sigma
    -> bidirectional elaboration
    -> metavariables и унификация
    -> NbE и definitional equality
    -> inductive families и dependent match
    -> termination
    -> refinements и guards
    -> SMT
    -> effects
    -> erasure
    -> typed Classical/Quantum HIR
```

Квантовая подсистема должна быть первым серьёзным применением этой архитектуры,
но не должна определять само ядро. Core остаётся общим, quantum semantics
выражается библиотекой зависимых типов, effect declarations и небольшим набором
проверяемых примитивов.

Именно такое разделение позволит NiaLang получить возможности уровня F*:
программирование, спецификации, доказательства и извлекаемый эффективный код в
одном языке, без превращения LLVM или QIR backend-а в неявный второй typechecker.

## 32. Локальные источники для реализации

Этот проектный документ опирается на следующие части загруженного локально F*:

- [центральное представление термов, computations и sigelts](../../FStar/src/syntax/FStarC.Syntax.Syntax.fsti);
- [guards, deferred constraints и lazy computations](../../FStar/src/typechecker/FStarC.TypeChecker.Common.fsti);
- [унификация, subtyping и discharge guards](../../FStar/src/typechecker/FStarC.TypeChecker.Rel.fst);
- [маршрут parse/desugar/typecheck/SMT/extraction](../../FStar/src/fstar/FStarC.Universal.fst);
- [SMT encoding interface](../../FStar/src/smtencoding/FStarC.SMTEncoding.Encode.fsti);
- [ML AST после erasure/extraction](../../FStar/src/extraction/FStarC.Extraction.ML.Syntax.fsti);
- [пример пользовательского индуктивного семейства `vec`](../../FStar/doc/book/code/Vec.fst);
- [описание bootstrap компилятора](../../FStar/doc/ref/bootstrapping.md).

Для квантовой части важны ограничения целевых профилей:

- [QIR Base Profile](../../QIR_REPOS_FOR_AI_CONTEXT/qir-spec/specification/profiles/Base_Profile.md);
- [QIR Adaptive Profile](../../QIR_REPOS_FOR_AI_CONTEXT/qir-spec/specification/profiles/Adaptive_Profile.md).

Эти репозитории используются только как справочный контекст. Реализация и
изменения должны находиться в основном репозитории `nialang`.
