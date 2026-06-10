//! Surface → Core elaborator (rewrite phase 4).

mod affine;
mod effect;
mod env;
mod expr;
mod quantum;
mod ids;
mod item;
mod prelude;
mod stmt;
mod symbols;
mod ty;

use crate::core::checker::Checker;
use crate::core::effect::Effect;
use crate::verify;
use crate::core::meta::MetaEnv;
use crate::core::term::Term;
use crate::core::termination::{
    check_fn_termination, check_seed_terminations, termination_spec_for_fn, Partiality,
};
use crate::core::{DataEnv, GlobalEnv, TypingCtx};
use crate::elab::env::{fn_type_for_def, ElabEnv};
use crate::elab::ids::fn_gid;
use crate::frontend::resolve::ResolvedModule;

pub use env::ElabEnv as ElaborationEnv;
pub use symbols::{BinOp, CmpOp, CodegenSymbols, RuntimeBuiltin, RuntimeTy};

/// One elaborated top-level function.
#[derive(Debug, Clone)]
pub struct ElaboratedFn {
    pub name: String,
    pub sig: Term,
    pub body: Term,
    pub decreases: Option<String>,
    pub partial: bool,
    pub param_names: Vec<(String, bool)>,
    pub fn_id: crate::frontend::resolve::DefId,
    pub mutual_group: Vec<crate::frontend::resolve::DefId>,
    pub requires: Option<Term>,
    pub ensures: Option<Term>,
    pub effect: Effect,
}

/// Surface module lowered to checked Core terms.
#[derive(Debug, Clone)]
pub struct ElaboratedModule {
    pub globals: GlobalEnv,
    pub data: DataEnv,
    pub symbols: CodegenSymbols,
    pub fns: Vec<ElaboratedFn>,
}

/// Lowers a resolved surface module to Core terms and metadata.
pub fn elaborate_module(resolved: &ResolvedModule) -> Result<ElaboratedModule, String> {
    let mut env = ElabEnv::new(resolved)?;
    let mut fns = Vec::new();
    for f in &resolved.fns {
        let sig = fn_type_for_def(&mut env, &f.def.params, f.def.ret.as_ref(), f.def.is_quantum)?;
        let requires = f
            .def
            .requires
            .as_ref()
            .map(|expr| {
                crate::elab::expr::elab_expr(&mut env, expr, Some(&Term::Global(crate::core::globals::prim::BOOL)))
                    .map(|(t, _)| t)
            })
            .transpose()?;
        let ensures = f
            .def
            .ensures
            .as_ref()
            .map(|expr| {
                crate::elab::expr::elab_expr(&mut env, expr, Some(&Term::Global(crate::core::globals::prim::BOOL)))
                    .map(|(t, _)| t)
            })
            .transpose()?;
        let body = item::elab_fn(&mut env, f)?;
        let gid = fn_gid(f.id);
        let effect = env.globals.effect_of(gid).unwrap_or(Effect::Tot);
        fns.push(ElaboratedFn {
            name: f.name.clone(),
            sig,
            body,
            decreases: f.def.decreases.clone(),
            partial: f.def.partial,
            param_names: f
                .def
                .params
                .iter()
                .map(|(name, _, implicit)| (name.clone(), *implicit))
                .collect(),
            fn_id: f.id,
            mutual_group: vec![],
            requires,
            ensures,
            effect,
        });
    }
    let mut symbols = env.build_symbols()?;
    for f in &fns {
        if let Ok(rt) = runtime_ret_from_sig(&f.sig, &symbols) {
            symbols.fn_rets.insert(f.name.clone(), rt);
        }
    }
    Ok(ElaboratedModule {
        globals: env.globals,
        data: env.data,
        symbols,
        fns,
    })
}

/// Typechecks elaborated function bodies against their signatures.
pub fn check_elaborated(module: &ElaboratedModule) -> Result<(), String> {
    let checker = Checker::with_data(&module.globals, &module.data);
    for f in &module.fns {
        let mut ctx = TypingCtx::default();
        let mut metas = MetaEnv::default();
        checker
            .check_term(&mut ctx, &mut metas, &f.body, &f.sig)
            .map_err(|e| format!("in `{}`: {e}", f.name))?;
    }

    check_seed_terminations(&module.globals, &module.data)?;

    verify::verify_module(module)?;

    for f in &module.fns {
        if f.partial {
            continue;
        }
        let gid = fn_gid(f.fn_id);
        let mut group: Vec<_> = f.mutual_group.iter().map(|id| fn_gid(*id)).collect();
        group.retain(|id| *id != gid);

        let probe = crate::core::termination::FnTerminationSpec {
            id: gid,
            decreases: crate::core::term::Level(0),
            partiality: Partiality::Total,
            mutual_group: group.clone(),
        };
        if crate::core::termination::has_recursive_calls(&probe, &f.body)
            && f.decreases.is_none()
        {
            return Err(format!(
                "in `{}`: recursive function requires a `decreases` clause",
                f.name
            ));
        }

        if let Some(spec) = termination_spec_for_fn(
            gid,
            &f.param_names,
            f.decreases.as_deref(),
            f.partial,
            &group,
        )? {
            check_fn_termination(&spec, &f.body, &module.data)
                .map_err(|e| format!("in `{}`: {e}", f.name))?;
        }
    }

    Ok(())
}

/// Pretty-prints elaborated Core terms for debugging.
pub fn format_elaborated_module(module: &ElaboratedModule) -> String {
    let mut out = String::from(";; nialang elaborated core\n");
    for f in &module.fns {
        out.push_str(&format!("\n;; fn {} : {:?}\n", f.name, f.sig));
        out.push_str(&format!("{:#?}\n", f.body));
    }
    out
}

fn runtime_ret_from_sig(sig: &Term, symbols: &CodegenSymbols) -> Result<RuntimeTy, String> {
    let mut cur = sig.clone();
    while let Term::Pi { body, .. } = cur {
        cur = *body;
    }
    cur = cur.peel_computation_result();
    match cur {
        Term::Global(id) => crate::erase::runtime_ty_from_term_global(id, symbols),
        _ => Err(format!("expected return type global, got `{cur:?}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::fixtures::{read_fixture, TIER_0};
    use crate::driver::pipeline::{parse_module, resolve_parsed_module};

    fn elab_fixture_ok(path: &str) {
        let src = read_fixture(path);
        let parsed = parse_module(&src).unwrap_or_else(|e| panic!("parse `{path}`: {e}"));
        let resolved =
            resolve_parsed_module(parsed).unwrap_or_else(|e| panic!("resolve `{path}`: {e}"));
        let elaborated =
            elaborate_module(&resolved).unwrap_or_else(|e| panic!("elab `{path}`: {e}"));
        check_elaborated(&elaborated)
            .unwrap_or_else(|e| panic!("check `{path}`: {e}"));
    }

    #[test]
    fn tier_0_fixtures_elaborate_and_check() {
        for path in TIER_0 {
            elab_fixture_ok(path);
        }
    }

    #[test]
    fn phase9_fixtures_elaborate_and_check() {
        for path in [
            "examples/tests/ok_floats.nia",
            "examples/tests/ok_string.nia",
            "examples/tests/ok_impl_methods.nia",
        ] {
            elab_fixture_ok(path);
        }
    }

    #[test]
    fn implicit_id_elaborates_and_checks() {
        elab_fixture_ok("examples/tests/core/ok_implicit_id.nia");
    }

    #[test]
    fn nat_add_elaborates_and_checks() {
        elab_fixture_ok("examples/tests/core/ok_nat_add.nia");
    }

    #[test]
    fn vec_append_elaborates_and_checks() {
        elab_fixture_ok("examples/tests/core/ok_vec_append.nia");
    }

    #[test]
    fn nonstructural_recursion_is_rejected() {
        let src = read_fixture("examples/tests/core/err_nonstructural_recursion.nia");
        let parsed = parse_module(&src).expect("parse");
        let resolved = resolve_parsed_module(parsed).expect("resolve");
        let elaborated = elaborate_module(&resolved).expect("elab");
        let err = check_elaborated(&elaborated).unwrap_err();
        assert!(
            err.contains("must `match` on its decreasing argument")
                || err.contains("non-structurally-smaller"),
            "expected termination error, got: {err}"
        );
    }

    #[test]
    fn safe_div_elaborates_and_checks() {
        elab_fixture_ok("examples/tests/ok_safe_div.nia");
    }

    #[test]
    fn io_call_in_tot_fn_is_rejected() {
        let src = read_fixture("examples/tests/err_io_in_tot.nia");
        let parsed = parse_module(&src).expect("parse");
        let resolved = resolve_parsed_module(parsed).expect("resolve");
        let err = elaborate_module(&resolved).unwrap_err();
        assert!(
            err.contains("effect mismatch") || err.contains("requires `IO`"),
            "expected effect error, got: {err}"
        );
    }

    #[test]
    fn quantum_fn_call_outside_quant_scope_is_rejected() {
        let src = read_fixture("examples/tests/err_quant_fn_outside_scope.nia");
        let parsed = parse_module(&src).expect("parse");
        let resolved = resolve_parsed_module(parsed).expect("resolve");
        let err = elaborate_module(&resolved).unwrap_err();
        assert!(
            err.contains("requires a `quant { }` scope"),
            "expected quant scope error, got: {err}"
        );
    }

    fn ambiguous_implicit_is_rejected() {
        let src = read_fixture("examples/tests/core/err_implicit_ambiguous.nia");
        let parsed = parse_module(&src).expect("parse");
        let resolved = resolve_parsed_module(parsed).expect("resolve");
        let elaborated = elaborate_module(&resolved).expect("elab");
        let err = check_elaborated(&elaborated).unwrap_err();
        assert!(
            err.contains("implicit argument `a`"),
            "expected implicit diagnostic, got: {err}"
        );
        assert!(
            err.contains("ambiguous") || err.contains("cannot unify"),
            "expected ambiguity/unify detail, got: {err}"
        );
    }
}
