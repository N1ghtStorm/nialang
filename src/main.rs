//! CLI binary for the Nialang compiler.

fn main() {
    match nialang::driver::pipeline::run_cli() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
