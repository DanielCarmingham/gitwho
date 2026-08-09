//! CLI entry point. Subcommand dispatch arrives with the credential helper;
//! for now the resolver is consumed as a library by the test suite.

fn main() -> std::process::ExitCode {
    eprintln!("gitfriend: no subcommands yet -- the resolver is library-only so far");
    std::process::ExitCode::FAILURE
}
