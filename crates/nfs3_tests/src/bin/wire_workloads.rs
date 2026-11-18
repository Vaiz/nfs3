fn main() -> anyhow::Result<()> {
    let name = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("supply one case; external process watchdog required"))?;
    nfs3_tests::wire_workloads::run(&name)
}
