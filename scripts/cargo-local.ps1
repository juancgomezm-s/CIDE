# Prefer the project's isolated toolchain when Rust is absent from PATH.
# No downloads, profile changes or remote CI are performed by this wrapper.
$projectRoot = Split-Path -Parent $PSScriptRoot
$localCargo = Join-Path $projectRoot 'work/toolchain/cargo/bin/cargo.exe'
$savedCargoHome = $env:CARGO_HOME
$savedRustupHome = $env:RUSTUP_HOME
$savedTemp = $env:TEMP
$savedTmp = $env:TMP
$savedIncremental = $env:CARGO_INCREMENTAL
$cargoExitCode = 1
Push-Location -LiteralPath $projectRoot
try {
    if (Test-Path -LiteralPath $localCargo) {
        $env:CARGO_HOME = Join-Path $projectRoot 'work/toolchain/cargo'
        $env:RUSTUP_HOME = Join-Path $projectRoot 'work/toolchain/rustup'
        $cargoExecutable = $localCargo
    } else {
        $cargoExecutable = (Get-Command cargo -ErrorAction Stop).Source
    }
    $testTemp = Join-Path $projectRoot 'work/test-temp'
    New-Item -ItemType Directory -Force -Path $testTemp | Out-Null
    $env:TEMP = $testTemp
    $env:TMP = $testTemp
    $env:CARGO_INCREMENTAL = '0'
    & $cargoExecutable @args
    $cargoExitCode = $LASTEXITCODE
} finally {
    $env:CARGO_HOME = $savedCargoHome
    $env:RUSTUP_HOME = $savedRustupHome
    $env:TEMP = $savedTemp
    $env:TMP = $savedTmp
    $env:CARGO_INCREMENTAL = $savedIncremental
    Pop-Location
}
exit $cargoExitCode
