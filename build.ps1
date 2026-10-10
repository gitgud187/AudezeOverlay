param([switch]$Test)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
if (Test-Path '.tools\cargo\bin\cargo.exe') {
    $env:RUSTUP_HOME = Join-Path $PSScriptRoot '.tools\rustup'
    $env:CARGO_HOME = Join-Path $PSScriptRoot '.tools\cargo'
    $env:PATH = "$PSScriptRoot\.tools\cargo\bin;$env:PATH"
    $llvm = Get-ChildItem '.tools' -Directory -Filter 'llvm-mingw-*' | Select-Object -First 1
    if ($llvm) {
        $env:PATH = "$($llvm.FullName)\bin;$env:PATH"
        $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnullvm'
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER = 'x86_64-w64-mingw32-clang.exe'
    }
}
if ($Test) { cargo test } else { cargo build --release --bin audeze-overlay }
if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
