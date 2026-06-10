#[cfg(test)]
mod tests {
    use crate::driver::fixtures::read_fixture;
    use crate::driver::pipeline::{elaborate_resolved_module, parse_module, resolve_parsed_module};
    use crate::verify::{collect_vcs, discharge_vcs, find_z3, format_vcs, VcStatus};

    #[test]
    fn safe_div_collects_and_discharges_refinement_vc() {
        let Some(_) = find_z3() else {
            eprintln!("skipping safe_div VC test: z3 not found in PATH");
            return;
        };

        let src = read_fixture("examples/tests/ok_safe_div.nia");
        let parsed = parse_module(&src).expect("parse");
        let resolved = resolve_parsed_module(parsed).expect("resolve");
        let elaborated = elaborate_resolved_module(&resolved).expect("elab+verify");
        let mut vcs = collect_vcs(&elaborated);
        discharge_vcs(&elaborated, &mut vcs).expect("discharge");
        let rendered = format_vcs(&vcs);
        assert!(rendered.contains(";; nialang verification conditions"));
        let guard = vcs
            .goals
            .iter()
            .find(|g| g.label.contains("refinement guard"))
            .expect("refinement guard VC");
        assert_eq!(guard.status, VcStatus::Discharged, "{rendered}");
        assert!(
            !guard.assumptions.is_empty(),
            "expected let-binding assumptions: {rendered}"
        );
        assert!(
            vcs.goals.iter().all(|g| g.status != VcStatus::Pending),
            "{rendered}"
        );
    }
}
