//! Effect lattice for computation types (phase 12).

/// Built-in effects for the first milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    Tot,
    Ghost,
    Div,
    IO,
    Quantum,
    Gpu,
}

impl Effect {
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Tot => "Tot",
            Effect::Ghost => "Ghost",
            Effect::Div => "Div",
            Effect::IO => "IO",
            Effect::Quantum => "Quantum",
            Effect::Gpu => "Gpu",
        }
    }
}

/// Joins two effects (max in the capability lattice).
pub fn join_effect(a: Effect, b: Effect) -> Effect {
    if a == Effect::Quantum && b == Effect::Gpu || a == Effect::Gpu && b == Effect::Quantum {
        return a.max(b);
    }
    a.max(b)
}

/// `sub` may run in a context that allows `sup`.
pub fn is_subeffect(sub: Effect, sup: Effect) -> bool {
    if sub == sup {
        return true;
    }
    match (sub, sup) {
        (Effect::Tot, _) => true,
        (Effect::Ghost, Effect::Tot) => false,
        (Effect::Ghost, _) => true,
        (Effect::Div, Effect::Tot | Effect::Ghost) => false,
        (Effect::Div, _) => true,
        (Effect::IO, Effect::Tot | Effect::Ghost | Effect::Div) => false,
        (Effect::IO, Effect::Quantum | Effect::Gpu) => true,
        (Effect::IO, Effect::IO) => true,
        (Effect::Quantum, Effect::Quantum) => true,
        (Effect::Gpu, Effect::Gpu) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_not_subeffect_of_tot() {
        assert!(!is_subeffect(Effect::IO, Effect::Tot));
    }

    #[test]
    fn tot_allowed_in_io() {
        assert!(is_subeffect(Effect::Tot, Effect::IO));
    }

    #[test]
    fn io_allowed_in_quantum() {
        assert!(is_subeffect(Effect::IO, Effect::Quantum));
    }

    #[test]
    fn quantum_not_allowed_in_io() {
        assert!(!is_subeffect(Effect::Quantum, Effect::IO));
    }
}
