# Pre-requisites:
# - Install the Windows ADK 10.1.26100.2454 (December 2024) from the link below
#   https://learn.microsoft.com/en-us/windows-hardware/get-started/adk-install#download-the-adk-101261002454-december-2024
# - Install samply
#   cargo install --locked samply


cargo b --example perf_test --release
samply record ./target/release/examples/perf_test.exe --output perf_test.samply