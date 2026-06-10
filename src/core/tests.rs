use crate::core::{
    meta::MetaEnv,
    checker::Checker,
    globals::{prim, GlobalEnv},
    nbe::{eval, is_def_eq, whnf},
    term::{Binder, Level, Term, UniverseLevel},
    TypingCtx,
    EvalEnv,
};

fn globals() -> GlobalEnv {
    GlobalEnv::with_primitives()
}

fn checker(g: &GlobalEnv) -> Checker<'_> {
    Checker::new(g)
}

fn i32_ty() -> Term {
    Term::Global(prim::I32)
}

fn bool_ty() -> Term {
    Term::Global(prim::BOOL)
}

fn mk_id_i32() -> Term {
    let level = Level(0);
    Term::Lam {
        binder: Binder::new("x", level, i32_ty()),
        body: Box::new(Term::Var(level)),
    }
}

fn mk_const_i32(n: i32) -> Term {
    Term::I32(n)
}

// --- term / subst ---

#[test]
fn subst_replaces_matching_var() {
    let t = Term::Var(Level(0));
    let r = Term::I32(7);
    assert_eq!(t.subst(Level(0), &r), Term::I32(7));
}

#[test]
fn subst_preserves_other_vars() {
    let t = Term::Var(Level(1));
    let r = Term::I32(7);
    assert_eq!(t.subst(Level(0), &r), Term::Var(Level(1)));
}

#[test]
fn subst_under_shadowing_binder() {
    let body = Term::Var(Level(1));
    let t = Term::Pi {
        binder: Binder::new("x", Level(0), i32_ty()),
        body: Box::new(body),
    };
    let r = Term::I32(1);
    assert_eq!(t.subst(Level(1), &r), t);
}

#[test]
fn subst_into_app() {
    let t = Term::App {
        fun: Box::new(Term::Var(Level(0))),
        arg: Box::new(Term::I32(2)),
    };
    let r = Term::I32(9);
    let out = t.subst(Level(0), &r);
    assert_eq!(
        out,
        Term::App {
            fun: Box::new(Term::I32(9)),
            arg: Box::new(Term::I32(2)),
        }
    );
}

#[test]
fn arrow_builder_non_dependent() {
    let arr = Term::arrow(i32_ty(), i32_ty());
    assert!(matches!(arr, Term::Pi { .. }));
}

// --- universe ---

#[test]
fn infer_type_has_kind() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let k = c.infer(&mut ctx, &mut metas, &Term::ty()).expect("kind");
    assert_eq!(k, Term::Universe(UniverseLevel(1)));
}

#[test]
fn infer_kind_has_superkind() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let k = c.infer(&mut ctx, &mut metas, &Term::kind()).expect("superkind");
    assert_eq!(k, Term::Universe(UniverseLevel(2)));
}

#[test]
fn check_i32_type_in_type() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    c.check(&mut ctx, &mut metas, &i32_ty(), &Term::ty()).expect("i32 in Type");
}

// --- literals ---

#[test]
fn infer_i32_literal() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let ty = c.infer(&mut ctx, &mut metas, &Term::I32(42)).expect("infer");
    assert!(c.def_eq(&ctx, &metas, &ty, &i32_ty()));
}

#[test]
fn infer_bool_literal() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let ty = c.infer(&mut ctx, &mut metas, &Term::Bool(true)).expect("infer");
    assert!(c.def_eq(&ctx, &metas, &ty, &bool_ty()));
}

#[test]
fn check_i32_literal() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    c.check(&mut ctx, &mut metas, &Term::I32(1), &i32_ty()).expect("check");
}

#[test]
fn check_bool_literal() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    c.check(&mut ctx, &mut metas, &Term::Bool(false), &bool_ty()).expect("check");
}

// --- pi ---

#[test]
fn infer_arrow_type() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let arr = Term::arrow(i32_ty(), i32_ty());
    let ty = c.infer(&mut ctx, &mut metas, &arr).expect("infer pi");
    assert!(c.def_eq(&ctx, &metas, &ty, &Term::ty()));
}

#[test]
fn infer_dependent_pi() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let pi = Term::Pi {
        binder: Binder::new("x", level, i32_ty()),
        body: Box::new(i32_ty()),
    };
    let ty = c.infer(&mut ctx, &mut metas, &pi).expect("infer dep pi");
    assert!(c.def_eq(&ctx, &metas, &ty, &Term::ty()));
}

#[test]
fn check_pi_against_type() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let pi = Term::arrow(bool_ty(), i32_ty());
    c.check(&mut ctx, &mut metas, &pi, &Term::ty()).expect("check pi");
}

// --- lambda ---

#[test]
fn check_identity_lambda() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let lam = mk_id_i32();
    let arr = Term::arrow(i32_ty(), i32_ty());
    c.check_term(&mut ctx, &mut metas, &lam, &arr).expect("id");
}

#[test]
fn check_const_lambda() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let lam = Term::Lam {
        binder: Binder::new("x", level, i32_ty()),
        body: Box::new(Term::I32(99)),
    };
    c.check_term(&mut ctx, &mut metas, &lam, &Term::arrow(i32_ty(), i32_ty()))
        .expect("const");
}

#[test]
fn check_lambda_domain_mismatch_fails() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let lam = Term::Lam {
        binder: Binder::new("x", level, bool_ty()),
        body: Box::new(Term::Var(level)),
    };
    let err = c
        .check_term(&mut ctx, &mut metas, &lam, &Term::arrow(i32_ty(), i32_ty()))
        .expect_err("domain mismatch");
    assert!(
        err.to_string().contains("domain mismatch")
            || err.to_string().contains("cannot unify"),
        "{err}"
    );
}

#[test]
fn check_lambda_body_mismatch_fails() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let lam = Term::Lam {
        binder: Binder::new("x", level, i32_ty()),
        body: Box::new(Term::Bool(true)),
    };
    let err = c
        .check_term(&mut ctx, &mut metas, &lam, &Term::arrow(i32_ty(), i32_ty()))
        .expect_err("body mismatch");
    assert!(
        err.to_string().contains("type mismatch")
            || err.to_string().contains("cannot unify"),
        "{err}"
    );
}

// --- application ---

#[test]
fn check_application_identity() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let app = Term::App {
        fun: Box::new(mk_id_i32()),
        arg: Box::new(Term::I32(5)),
    };
    let ty = c.infer(&mut ctx, &mut metas, &app).expect("app");
    assert!(c.def_eq(&ctx, &metas, &ty, &i32_ty()));
}

#[test]
fn check_application_const_fn() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let k = Term::Lam {
        binder: Binder::new("x", level, i32_ty()),
        body: Box::new(Term::I32(7)),
    };
    let app = Term::App {
        fun: Box::new(k),
        arg: Box::new(Term::I32(0)),
    };
    let ty = c.infer(&mut ctx, &mut metas, &app).expect("app const");
    assert!(c.def_eq(&ctx, &metas, &ty, &i32_ty()));
}

#[test]
fn application_arg_mismatch_fails() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let app = Term::App {
        fun: Box::new(mk_id_i32()),
        arg: Box::new(Term::Bool(true)),
    };
    let err = c.infer(&mut ctx, &mut metas, &app).expect_err("arg mismatch");
    assert!(
        err.to_string().contains("type mismatch")
            || err.to_string().contains("cannot unify"),
        "{err}"
    );
}

#[test]
fn application_non_function_fails() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let app = Term::App {
        fun: Box::new(Term::I32(1)),
        arg: Box::new(Term::I32(2)),
    };
    let err = c.infer(&mut ctx, &mut metas, &app).expect_err("not a function");
    assert!(err.to_string().contains("expected Pi"), "{err}");
}

// --- let ---

#[test]
fn infer_let_binding() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let t = Term::Let {
        binder: Binder::new("x", level, i32_ty()),
        value: Box::new(Term::I32(3)),
        body: Box::new(Term::Var(level)),
    };
    let ty = c.infer(&mut ctx, &mut metas, &t).expect("let");
    assert!(c.def_eq(&ctx, &metas, &ty, &i32_ty()));
}

#[test]
fn let_annotation_mismatch_fails() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let t = Term::Let {
        binder: Binder::new("x", level, bool_ty()),
        value: Box::new(Term::I32(3)),
        body: Box::new(Term::Var(level)),
    };
    let err = c.infer(&mut ctx, &mut metas, &t).expect_err("let mismatch");
    assert!(
        err.to_string().contains("type mismatch")
            || err.to_string().contains("cannot unify"),
        "{err}"
    );
}

#[test]
fn let_shadows_correctly() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let outer = Level(0);
    let inner = Level(1);
    let t = Term::Let {
        binder: Binder::new("x", outer, i32_ty()),
        value: Box::new(Term::I32(1)),
        body: Box::new(Term::Let {
            binder: Binder::new("y", inner, bool_ty()),
            value: Box::new(Term::Bool(true)),
            body: Box::new(Term::Var(inner)),
        }),
    };
    let ty = c.infer(&mut ctx, &mut metas, &t).expect("nested let");
    assert!(c.def_eq(&ctx, &metas, &ty, &bool_ty()));
}

// --- def_eq / nbe ---

#[test]
fn def_eq_i32_literals() {
    let env = EvalEnv::default();
    let globals = GlobalEnv::with_primitives();
    assert!(is_def_eq(&env, &globals, &Term::I32(1), &Term::I32(1)));
    assert!(!is_def_eq(&env, &globals, &Term::I32(1), &Term::I32(2)));
}

#[test]
fn def_eq_arrow_types() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let a = Term::arrow(i32_ty(), bool_ty());
    let b = Term::arrow(i32_ty(), bool_ty());
    assert!(c.def_eq(&ctx, &metas, &a, &b));
}

#[test]
fn def_eq_alpha_equivalent_lambda() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let l1 = Term::Lam {
        binder: Binder::new("x", Level(0), i32_ty()),
        body: Box::new(Term::Var(Level(0))),
    };
    let l2 = Term::Lam {
        binder: Binder::new("y", Level(0), i32_ty()),
        body: Box::new(Term::Var(Level(0))),
    };
    assert!(c.def_eq(&ctx, &metas, &l1, &l2));
}

#[test]
fn eval_beta_redex() {
    let env = EvalEnv::default();
    let globals = GlobalEnv::with_primitives();
    let level = Level(0);
    let t = Term::App {
        fun: Box::new(Term::Lam {
            binder: Binder::new("x", level, i32_ty()),
            body: Box::new(Term::Var(level)),
        }),
        arg: Box::new(Term::I32(10)),
    };
    let v = eval(&env, &globals, &t);
    assert!(matches!(v, crate::core::Value::I32(10)));
}

#[test]
fn eval_let_reduces() {
    let env = EvalEnv::default();
    let globals = GlobalEnv::with_primitives();
    let level = Level(0);
    let t = Term::Let {
        binder: Binder::new("x", level, i32_ty()),
        value: Box::new(Term::I32(4)),
        body: Box::new(Term::Var(level)),
    };
    let v = eval(&env, &globals, &t);
    assert!(matches!(v, crate::core::Value::I32(4)));
}

#[test]
fn whnf_leaves_pi_unfolded() {
    let env = EvalEnv::default();
    let globals = GlobalEnv::with_primitives();
    let pi = Term::arrow(i32_ty(), i32_ty());
    let v = whnf(&env, &globals, &pi);
    assert!(matches!(v, crate::core::Value::Pi { .. }));
}

#[test]
fn universe_mismatch_not_def_eq() {
    let env = EvalEnv::default();
    let globals = GlobalEnv::with_primitives();
    assert!(!is_def_eq(
        &env,
        &globals,
        &Term::Universe(UniverseLevel(0)),
        &Term::Universe(UniverseLevel(1)),
    ));
}

#[test]
fn infer_var_out_of_scope_fails() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let err = c.infer(&mut ctx, &mut metas, &Term::Var(Level(0))).expect_err("unbound");
    assert!(err.to_string().contains("unbound variable"), "{err}");
}

#[test]
fn check_composed_program() {
    let g = globals();
    let c = checker(&g);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let level = Level(0);
    let program = Term::Let {
        binder: Binder::new("id", level, Term::arrow(i32_ty(), i32_ty())),
        value: Box::new(mk_id_i32()),
        body: Box::new(Term::App {
            fun: Box::new(Term::Var(level)),
            arg: Box::new(mk_const_i32(42)),
        }),
    };
    let ty = c.infer(&mut ctx, &mut metas, &program).expect("program");
    assert!(c.def_eq(&ctx, &metas, &ty, &i32_ty()));
}

#[test]
fn vec_append_typechecks_in_core() {
    use crate::core::inductive::seed;

    let mut globals = GlobalEnv::with_primitives();
    let mut data = crate::core::DataEnv::default();
    let nat = seed::register_nat(&mut globals, &mut data).expect("nat");
    seed::register_nat_add_value(&mut globals, nat.family, nat.add);
    let vec = seed::register_vec(&mut globals, &mut data).expect("vec");
    seed::register_append_value(
        &mut globals,
        nat.family,
        vec.family,
        nat.add,
        vec.append,
    );

    let i32 = Term::Global(prim::I32);
    let nil = seed::nil_ctor(vec.family);
    let one = Term::I32(1);
    let two = Term::I32(2);
    let three = Term::I32(3);

    let inner = seed::cons_ctor(
        vec.family,
        nat.family,
        seed::zero_ctor(nat.family),
        two,
        nil.clone(),
    );
    let n_one = seed::succ_ctor(nat.family, seed::zero_ctor(nat.family));
    let xs = seed::cons_ctor(vec.family, nat.family, n_one, one, inner);

    let ys = seed::cons_ctor(
        vec.family,
        nat.family,
        seed::zero_ctor(nat.family),
        three,
        nil,
    );

    let call = Term::App {
        fun: Box::new(Term::App {
            fun: Box::new(Term::Global(vec.append)),
            arg: Box::new(xs),
        }),
        arg: Box::new(ys),
    };

    let checker = Checker::with_data(&globals, &data);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let ty = checker.infer(&mut ctx, &mut metas, &call).expect("append");
    let n_two = seed::succ_ctor(
        nat.family,
        seed::succ_ctor(nat.family, seed::zero_ctor(nat.family)),
    );
    let expected = crate::core::inductive::apply_family(
        vec.family,
        &[i32],
        &[Term::App {
            fun: Box::new(Term::App {
                fun: Box::new(Term::Global(nat.add)),
                arg: Box::new(n_two),
            }),
            arg: Box::new(seed::succ_ctor(nat.family, seed::zero_ctor(nat.family))),
        }],
    );
    assert!(
        checker.def_eq(&ctx, &metas, &ty, &expected),
        "got `{ty:?}`, expected `{expected:?}`"
    );
}

#[test]
fn nat_add_typechecks_in_core() {
    use crate::core::inductive::seed;

    let mut globals = GlobalEnv::with_primitives();
    let mut data = crate::core::DataEnv::default();
    let nat = seed::register_nat(&mut globals, &mut data).expect("nat");
    seed::register_nat_add_value(&mut globals, nat.family, nat.add);
    let checker = Checker::with_data(&globals, &data);
    let term = Term::App {
        fun: Box::new(Term::App {
            fun: Box::new(Term::Global(nat.add)),
            arg: Box::new(seed::zero_ctor(nat.family)),
        }),
        arg: Box::new(seed::succ_ctor(
            nat.family,
            seed::zero_ctor(nat.family),
        )),
    };
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let ty = checker
        .infer(&mut ctx, &mut metas, &term)
        .expect("nat add");
    assert!(checker.def_eq(
        &ctx,
        &metas,
        &ty,
        &Term::Global(nat.family)
    ));
}

#[test]
fn infer_call_with_implicit_type_param() {
    use crate::core::term::{Explicitness, Relevance};
    use crate::frontend::resolve::DefId;

    let mut globals = GlobalEnv::with_primitives();
    let id_gid = DefId(100);
    let mut bind_a = Binder::new("a", Level(0), Term::ty());
    bind_a.explicitness = Explicitness::Implicit;
    bind_a.relevance = Relevance::Erased;
    let id_ty = Term::Pi {
        binder: bind_a,
        body: Box::new(Term::Pi {
            binder: Binder::new("x", Level(1), Term::Var(Level(0))),
            body: Box::new(Term::Var(Level(0))),
        }),
    };
    globals.insert_type(id_gid, id_ty);

    let c = checker(&globals);
    let mut ctx = TypingCtx::default();
    let mut metas = MetaEnv::default();
    let call = Term::App {
        fun: Box::new(Term::Global(id_gid)),
        arg: Box::new(Term::I32(42)),
    };
    let ty = c.infer(&mut ctx, &mut metas, &call).expect("implicit call");
    assert!(
        c.def_eq(&ctx, &metas, &ty, &i32_ty()),
        "got `{ty:?}`, metas={metas:?}"
    );
}
