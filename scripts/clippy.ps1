param (
    [switch]$Fix
)

$clippyArgs = @(
    "--workspace",
    "--all-targets",
    "--all-features",
    "--", "-D", "warnings"
)

if ($Fix) {
    Write-Host "⚙️ Running cargo fix before Clippy..."
    cargo fix --workspace --allow-dirty --allow-staged
}

Write-Host "🔍 Running Clippy with workspace configuration..."
cargo clippy @clippyArgs
