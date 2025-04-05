param (
    [switch]$Fix
)

$clippyArgs = @(
    "--workspace",
    "--all-targets",
    "--all-features",
    "--", "-D", "warnings"
)

clear
if ($Fix) {
    Write-Host "⚙️ Running cargo clippy --fix..."
    cargo clippy --fix --allow-dirty @clippyArgs
    cargo +nightly fmt
} else {
    Write-Host "⚙️ Running cargo clippy..."
    cargo clippy @clippyArgs
}
