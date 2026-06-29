//! Built-in "std" surface: `println`, `len`, heap helpers, matrix helpers (reserved names).

mod crypto_llvm;

use crate::ast::{Ability, EnumDef, EnumVariantDef, EnumVariantFields, StructDef, Ty};

pub const PRINTLN: &str = "println";
/// Array length: `len(arr)` → `i32` (compile-time size of `[T; N]`).
pub const LEN: &str = "len";
pub const ALLOC: &str = "alloc";
pub const DEALLOC: &str = "dealloc";
pub const REALLOC: &str = "realloc";
pub const MATRIX_TYPE: &str = "Matrix";
pub const MATRIX_NEW: &str = "matrix";
pub const MATRIX_GET: &str = "matrix_get";
pub const MATRIX_SET: &str = "matrix_set";
pub const MATRIX_ROWS: &str = "matrix_rows";
pub const MATRIX_COLS: &str = "matrix_cols";
pub const MATRIX_LEN: &str = "matrix_len";
pub const MATRIX_CLONE: &str = "matrix_clone";
pub const MATRIX_DROP: &str = "matrix_drop";
pub const VECTOR_GET: &str = "vector_get";
pub const VECTOR_SET: &str = "vector_set";
pub const VECTOR_LEN: &str = "vector_len";
pub const VECTOR_CLONE: &str = "vector_clone";
pub const VECTOR_DROP: &str = "vector_drop";
pub const LIST_TYPE: &str = "List";
pub const OPTION_TYPE: &str = "Option";
pub const RESULT_TYPE: &str = "Result";
pub const OPTION_SOME: &str = "Some";
pub const OPTION_NONE: &str = "None";
pub const RESULT_OK: &str = "Ok";
pub const RESULT_ERR: &str = "Err";
pub const LIST_NEW: &str = "list_new";
pub const LIST_WITH_CAPACITY: &str = "list_with_capacity";
pub const LIST_LEN: &str = "len";
pub const LIST_CAPACITY: &str = "capacity";
pub const LIST_PUSH: &str = "push";
pub const LIST_GET: &str = "get";
pub const OUTER: &str = "outer";
pub const TO_ARRAY: &str = "to_array";
pub const TO_MATRIX: &str = "to_matrix";
pub const TO_VEC: &str = "to_vec";
pub const COMPLEX_TYPE: &str = "Complex";
pub const COMPLEX_NEW: &str = "complex";
pub const COMPLEX_ADD: &str = "complex_add";
pub const COMPLEX_SUB: &str = "complex_sub";
pub const COMPLEX_MUL: &str = "complex_mul";
pub const COMPLEX_SCALE: &str = "complex_scale";
pub const COMPLEX_DIV: &str = "complex_div";
pub const SIN: &str = "sin";
pub const COS: &str = "cos";
pub const PI: &str = "PI";
pub const CIS: &str = "cis";
pub const QUBIT: &str = "qubit";
pub const RESULT: &str = "result";
pub const ORDERING_TYPE: &str = "Ordering";
pub const ORDERING_RELAXED: &str = "Relaxed";
pub const ORDERING_ACQUIRE: &str = "Acquire";
pub const ORDERING_RELEASE: &str = "Release";
pub const ORDERING_ACQ_REL: &str = "AcqRel";
pub const ORDERING_SEQ_CST: &str = "SeqCst";
pub const ATOMIC_BOOL_TYPE: &str = "AtomicBool";
pub const ATOMIC_I8_TYPE: &str = "AtomicI8";
pub const ATOMIC_U8_TYPE: &str = "AtomicU8";
pub const ATOMIC_I16_TYPE: &str = "AtomicI16";
pub const ATOMIC_U16_TYPE: &str = "AtomicU16";
pub const ATOMIC_I32_TYPE: &str = "AtomicI32";
pub const ATOMIC_U32_TYPE: &str = "AtomicU32";
pub const ATOMIC_I64_TYPE: &str = "AtomicI64";
pub const ATOMIC_U64_TYPE: &str = "AtomicU64";
pub const ATOMIC_I128_TYPE: &str = "AtomicI128";
pub const ATOMIC_U128_TYPE: &str = "AtomicU128";
pub const ATOMIC_ISIZE_TYPE: &str = "AtomicIsize";
pub const ATOMIC_USIZE_TYPE: &str = "AtomicUsize";
pub const ATOMIC_PTR_TYPE: &str = "AtomicPtr";
pub const ATOMIC_BOOL: &str = "atomic_bool";
pub const ATOMIC_I8: &str = "atomic_i8";
pub const ATOMIC_U8: &str = "atomic_u8";
pub const ATOMIC_I16: &str = "atomic_i16";
pub const ATOMIC_U16: &str = "atomic_u16";
pub const ATOMIC_I32: &str = "atomic_i32";
pub const ATOMIC_U32: &str = "atomic_u32";
pub const ATOMIC_I64: &str = "atomic_i64";
pub const ATOMIC_U64: &str = "atomic_u64";
pub const ATOMIC_I128: &str = "atomic_i128";
pub const ATOMIC_U128: &str = "atomic_u128";
pub const ATOMIC_ISIZE: &str = "atomic_isize";
pub const ATOMIC_USIZE: &str = "atomic_usize";
pub const ATOMIC_PTR: &str = "atomic_ptr";
pub const ATOMIC_FENCE: &str = "atomic_fence";
pub const THREAD_TYPE: &str = "Thread";
pub const SPAWN: &str = "spawn";
pub const JOIN: &str = "join";
pub const GATE_I: &str = "I";
pub const GATE_H: &str = "H";
pub const GATE_X: &str = "X";
pub const GATE_Y: &str = "Y";
pub const GATE_Z: &str = "Z";
pub const GATE_S: &str = "S";
pub const GATE_SDG: &str = "Sdg";
pub const GATE_T: &str = "T";
pub const GATE_TDG: &str = "Tdg";
pub const GATE_CNOT: &str = "CNOT";
pub const GATE_CZ: &str = "CZ";
pub const GATE_SWAP: &str = "SWAP";
pub const GATE_CH: &str = "CH";
pub const GATE_CY: &str = "CY";
pub const GATE_CS: &str = "CS";
pub const GATE_CSDG: &str = "CSdg";
pub const GATE_CT: &str = "CT";
pub const GATE_CTDG: &str = "CTdg";
pub const GATE_CCNOT: &str = "CCNOT";
pub const GATE_CCZ: &str = "CCZ";
pub const GATE_CSWAP: &str = "CSWAP";
pub const GATE_RX: &str = "Rx";
pub const GATE_RY: &str = "Ry";
pub const GATE_RZ: &str = "Rz";
pub const GATE_R1: &str = "R1";
pub const GATE_CRX: &str = "CRx";
pub const GATE_CRY: &str = "CRy";
pub const GATE_CRZ: &str = "CRz";
pub const GATE_CR1: &str = "CR1";
pub const MEASURE: &str = "q_measure";
pub const READ: &str = "q_read";
pub const RECORD: &str = "q_record";
pub const SHA256: &str = "sha256";
pub const DIGEST_EQ: &str = "digest_eq";
pub const MERKLE_LEAF_HASH: &str = "merkle_leaf_hash";
pub const MERKLE_NODE_HASH: &str = "merkle_node_hash";
pub const MERKLE_ROOT: &str = "merkle_root";
pub const MERKLE_ROOT_FROM_DATA: &str = "merkle_root_from_data";
pub const MERKLE_VERIFY: &str = "merkle_verify";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicOrdering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

impl AtomicOrdering {
    pub fn from_variant_name(name: &str) -> Option<Self> {
        match name {
            ORDERING_RELAXED => Some(Self::Relaxed),
            ORDERING_ACQUIRE => Some(Self::Acquire),
            ORDERING_RELEASE => Some(Self::Release),
            ORDERING_ACQ_REL => Some(Self::AcqRel),
            ORDERING_SEQ_CST => Some(Self::SeqCst),
            _ => None,
        }
    }

    pub fn variant_name(self) -> &'static str {
        match self {
            Self::Relaxed => ORDERING_RELAXED,
            Self::Acquire => ORDERING_ACQUIRE,
            Self::Release => ORDERING_RELEASE,
            Self::AcqRel => ORDERING_ACQ_REL,
            Self::SeqCst => ORDERING_SEQ_CST,
        }
    }
}

pub fn atomic_ordering_from_path(path: &str) -> Option<AtomicOrdering> {
    let (enum_name, variant) = path.rsplit_once("::")?;
    if enum_name == ORDERING_TYPE {
        AtomicOrdering::from_variant_name(variant)
    } else {
        None
    }
}

pub fn complex_ty() -> Ty {
    Ty::Struct(COMPLEX_TYPE.into())
}

pub fn builtin_structs() -> Vec<StructDef> {
    vec![StructDef {
        name: COMPLEX_TYPE.into(),
        abilities: vec![Ability::Copy, Ability::Clone, Ability::Drop],
        is_tuple: false,
        fields: vec![("re".into(), Ty::F64), ("im".into(), Ty::F64)],
    }]
}

pub fn builtin_enums() -> Vec<EnumDef> {
    vec![EnumDef {
        name: ORDERING_TYPE.into(),
        abilities: vec![Ability::Copy, Ability::Clone, Ability::Drop],
        variants: vec![
            EnumVariantDef {
                name: ORDERING_RELAXED.into(),
                fields: EnumVariantFields::Unit,
            },
            EnumVariantDef {
                name: ORDERING_ACQUIRE.into(),
                fields: EnumVariantFields::Unit,
            },
            EnumVariantDef {
                name: ORDERING_RELEASE.into(),
                fields: EnumVariantFields::Unit,
            },
            EnumVariantDef {
                name: ORDERING_ACQ_REL.into(),
                fields: EnumVariantFields::Unit,
            },
            EnumVariantDef {
                name: ORDERING_SEQ_CST.into(),
                fields: EnumVariantFields::Unit,
            },
        ],
    }]
}

pub fn is_builtin_enum_type_name(name: &str) -> bool {
    matches!(name, ORDERING_TYPE | OPTION_TYPE | RESULT_TYPE)
}

pub fn is_reserved_type_name(name: &str) -> bool {
    matches!(
        name,
        MATRIX_TYPE
            | COMPLEX_TYPE
            | LIST_TYPE
            | OPTION_TYPE
            | RESULT_TYPE
            | QUBIT
            | RESULT
            | ORDERING_TYPE
            | ATOMIC_BOOL_TYPE
            | ATOMIC_I8_TYPE
            | ATOMIC_U8_TYPE
            | ATOMIC_I16_TYPE
            | ATOMIC_U16_TYPE
            | ATOMIC_I32_TYPE
            | ATOMIC_U32_TYPE
            | ATOMIC_I64_TYPE
            | ATOMIC_U64_TYPE
            | ATOMIC_I128_TYPE
            | ATOMIC_U128_TYPE
            | ATOMIC_ISIZE_TYPE
            | ATOMIC_USIZE_TYPE
            | ATOMIC_PTR_TYPE
            | THREAD_TYPE
    )
}

pub fn is_reserved_fn_name(name: &str) -> bool {
    matches!(
        name,
        PRINTLN
            | LEN
            | ALLOC
            | DEALLOC
            | REALLOC
            | MATRIX_NEW
            | MATRIX_GET
            | MATRIX_SET
            | MATRIX_ROWS
            | MATRIX_COLS
            | MATRIX_LEN
            | MATRIX_CLONE
            | MATRIX_DROP
            | VECTOR_GET
            | VECTOR_SET
            | VECTOR_LEN
            | VECTOR_CLONE
            | VECTOR_DROP
            | LIST_NEW
            | LIST_WITH_CAPACITY
            | OPTION_SOME
            | OPTION_NONE
            | RESULT_OK
            | RESULT_ERR
            | OUTER
            | COMPLEX_NEW
            | COMPLEX_ADD
            | COMPLEX_SUB
            | COMPLEX_MUL
            | COMPLEX_SCALE
            | COMPLEX_DIV
            | SIN
            | COS
            | PI
            | CIS
            | ATOMIC_BOOL
            | ATOMIC_I8
            | ATOMIC_U8
            | ATOMIC_I16
            | ATOMIC_U16
            | ATOMIC_I32
            | ATOMIC_U32
            | ATOMIC_I64
            | ATOMIC_U64
            | ATOMIC_I128
            | ATOMIC_U128
            | ATOMIC_ISIZE
            | ATOMIC_USIZE
            | ATOMIC_PTR
            | ATOMIC_FENCE
            | SPAWN
            | JOIN
            | QUBIT
            | RESULT
            | GATE_I
            | GATE_H
            | GATE_X
            | GATE_Y
            | GATE_Z
            | GATE_S
            | GATE_SDG
            | GATE_T
            | GATE_TDG
            | GATE_CNOT
            | GATE_CZ
            | GATE_SWAP
            | GATE_CH
            | GATE_CY
            | GATE_CS
            | GATE_CSDG
            | GATE_CT
            | GATE_CTDG
            | GATE_CCNOT
            | GATE_CCZ
            | GATE_CSWAP
            | GATE_RX
            | GATE_RY
            | GATE_RZ
            | GATE_R1
            | GATE_CRX
            | GATE_CRY
            | GATE_CRZ
            | GATE_CR1
            | MEASURE
            | READ
            | RECORD
            | SHA256
            | DIGEST_EQ
            | MERKLE_LEAF_HASH
            | MERKLE_NODE_HASH
            | MERKLE_ROOT
            | MERKLE_ROOT_FROM_DATA
            | MERKLE_VERIFY
    )
}

/// LLVM IR prelude used by builtin `println` codegen.
///
/// Contains:
/// - all static format strings/text fragments used by generated `printf` calls,
/// - external declaration of `printf`.
/// The returned string is embedded at the top of every generated module.
pub fn llvm_prelude() -> String {
    let mut prelude = String::from(
        r#"; --- nialang std ---
@nialang.std.fmt.i32 = private unnamed_addr constant [4 x i8] c"%d\0A\00", align 1
@nialang.std.fmt.u32 = private unnamed_addr constant [4 x i8] c"%u\0A\00", align 1
@nialang.std.fmt.i64 = private unnamed_addr constant [6 x i8] c"%lld\0A\00", align 1
@nialang.std.fmt.u64 = private unnamed_addr constant [6 x i8] c"%llu\0A\00", align 1
@nialang.std.fmt.i128hex = private unnamed_addr constant [18 x i8] c"0x%016llx%016llx\0A\00", align 1
@nialang.std.fmt.ptrhex = private unnamed_addr constant [8 x i8] c"0x%llx\0A\00", align 1
@nialang.std.fmt.matrix = private unnamed_addr constant [30 x i8] c"Matrix(rows=%lld, cols=%lld)\0A\00", align 1
@nialang.std.fmt.matrix.nn = private unnamed_addr constant [29 x i8] c"Matrix(rows=%lld, cols=%lld)\00", align 1
@nialang.std.fmt.f64 = private unnamed_addr constant [4 x i8] c"%f\0A\00", align 1
@nialang.std.fmt.f64.nn = private unnamed_addr constant [3 x i8] c"%f\00", align 1
@nialang.std.fmt.str = private unnamed_addr constant [4 x i8] c"%s\0A\00", align 1
@nialang.std.fmt.str.nn = private unnamed_addr constant [3 x i8] c"%s\00", align 1
@nialang.std.fmt.i32.nn = private unnamed_addr constant [3 x i8] c"%d\00", align 1
@nialang.std.fmt.u32.nn = private unnamed_addr constant [3 x i8] c"%u\00", align 1
@nialang.std.fmt.i64.nn = private unnamed_addr constant [5 x i8] c"%lld\00", align 1
@nialang.std.fmt.u64.nn = private unnamed_addr constant [5 x i8] c"%llu\00", align 1
@nialang.std.fmt.i128hex.nn = private unnamed_addr constant [17 x i8] c"0x%016llx%016llx\00", align 1
@nialang.std.fmt.ptrhex.nn = private unnamed_addr constant [7 x i8] c"0x%llx\00", align 1
@nialang.std.txt.arr_open = private unnamed_addr constant [2 x i8] c"[\00", align 1
@nialang.std.txt.arr_sep = private unnamed_addr constant [3 x i8] c", \00", align 1
@nialang.std.txt.arr_close = private unnamed_addr constant [2 x i8] c"]\00", align 1
@nialang.std.txt.arr_close_ln = private unnamed_addr constant [3 x i8] c"]\0A\00", align 1
@nialang.std.txt.obj_open = private unnamed_addr constant [2 x i8] c"{\00", align 1
@nialang.std.txt.obj_close = private unnamed_addr constant [2 x i8] c"}\00", align 1
@nialang.std.txt.obj_close_ln = private unnamed_addr constant [3 x i8] c"}\0A\00", align 1
@nialang.std.txt.tuple_open = private unnamed_addr constant [2 x i8] c"(\00", align 1
@nialang.std.txt.tuple_close = private unnamed_addr constant [2 x i8] c")\00", align 1
@nialang.std.txt.tuple_close_ln = private unnamed_addr constant [3 x i8] c")\0A\00", align 1
@nialang.std.txt.anonvec.close = private unnamed_addr constant [3 x i8] c"})\00", align 1
@nialang.std.txt.anonvec.close_ln = private unnamed_addr constant [4 x i8] c"})\0A\00", align 1
@nialang.std.txt.anonvec.open.i8 = private unnamed_addr constant [5 x i8] c"(i8{\00", align 1
@nialang.std.txt.anonvec.open.u8 = private unnamed_addr constant [5 x i8] c"(u8{\00", align 1
@nialang.std.txt.anonvec.open.i16 = private unnamed_addr constant [6 x i8] c"(i16{\00", align 1
@nialang.std.txt.anonvec.open.u16 = private unnamed_addr constant [6 x i8] c"(u16{\00", align 1
@nialang.std.txt.anonvec.open.i32 = private unnamed_addr constant [6 x i8] c"(i32{\00", align 1
@nialang.std.txt.anonvec.open.u32 = private unnamed_addr constant [6 x i8] c"(u32{\00", align 1
@nialang.std.txt.anonvec.open.i64 = private unnamed_addr constant [6 x i8] c"(i64{\00", align 1
@nialang.std.txt.anonvec.open.u64 = private unnamed_addr constant [6 x i8] c"(u64{\00", align 1
@nialang.std.txt.anonvec.open.i128 = private unnamed_addr constant [7 x i8] c"(i128{\00", align 1
@nialang.std.txt.anonvec.open.isize = private unnamed_addr constant [8 x i8] c"(isize{\00", align 1
@nialang.std.txt.anonvec.open.usize = private unnamed_addr constant [8 x i8] c"(usize{\00", align 1
@nialang.std.txt.anonvec.open.u128 = private unnamed_addr constant [7 x i8] c"(u128{\00", align 1
@nialang.std.txt.anonvec.open.f16 = private unnamed_addr constant [6 x i8] c"(f16{\00", align 1
@nialang.std.txt.anonvec.open.f32 = private unnamed_addr constant [6 x i8] c"(f32{\00", align 1
@nialang.std.txt.anonvec.open.f64 = private unnamed_addr constant [6 x i8] c"(f64{\00", align 1
@nialang.std.txt.anonvec.open.bool = private unnamed_addr constant [7 x i8] c"(bool{\00", align 1
@nialang.std.txt.anonvec.open.string = private unnamed_addr constant [9 x i8] c"(string{\00", align 1

declare i32 @printf(ptr nocapture, ...)
declare i32 @strcmp(ptr, ptr)
declare ptr @malloc(i64)
declare void @free(ptr)
declare ptr @realloc(ptr, i64)
declare void @abort()
declare double @sin(double)
declare double @cos(double)
declare i32 @pthread_create(ptr, ptr, ptr, ptr)
declare i32 @pthread_join(ptr, ptr)
declare i32 @"\01_pthread_join"(ptr, ptr)
declare i32 @pthread_detach(ptr)

define ptr @nialang.thread.entry(ptr %arg) {
entry:
  %fnv = load { ptr, ptr, ptr, ptr }, ptr %arg
  call void @free(ptr %arg)
  %code = extractvalue { ptr, ptr, ptr, ptr } %fnv, 0
  %env = extractvalue { ptr, ptr, ptr, ptr } %fnv, 1
  call void %code(ptr %env)
  %drop = extractvalue { ptr, ptr, ptr, ptr } %fnv, 2
  %has_drop = icmp ne ptr %drop, null
  br i1 %has_drop, label %drop.env, label %done

drop.env:
  call void %drop(ptr %env)
  br label %done

done:
  ret ptr null
}

"#,
    );
    prelude.push_str(crypto_llvm::CRYPTO_LLVM_PRELUDE);
    prelude
}
