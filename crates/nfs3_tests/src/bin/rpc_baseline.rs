//! One isolated B0.3 case per process; always run with an external watchdog.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let case = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("supply one case: {:?}", nfs3_tests::rpc_baseline::CASES))?;
    anyhow::ensure!(args.next().is_none(), "exactly one case per process");
    match nfs3_tests::rpc_baseline::run(&case) {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("harness=failed; exact_claims=invalid");
            Err(error)
        }
    }
}
