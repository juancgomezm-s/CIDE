param([string]$Executable='target/release/cide-tui.exe',[string]$OutputPath='docs/benchmarks/F3-conpty.json',[switch]$ProjectSearch,[switch]$DocumentSearch,[string]$GitExecutable,[switch]$GitMetrics,[int]$GitFixtureBytes=0)
$ErrorActionPreference='Stop'
if ($ProjectSearch -and $OutputPath -eq 'docs/benchmarks/F3-conpty.json') {$OutputPath='docs/benchmarks/S1-conpty.json'}
if ($DocumentSearch -and $OutputPath -eq 'docs/benchmarks/F3-conpty.json') {$OutputPath='docs/benchmarks/S0-conpty.json'}
if ($GitMetrics -and $OutputPath -eq 'docs/benchmarks/F3-conpty.json') {$OutputPath='docs/benchmarks/M2-git-metrics-smoke.json'}
if ($GitExecutable -and $OutputPath -eq 'docs/benchmarks/F3-conpty.json') {$OutputPath='docs/benchmarks/M2-git-conpty.json'}
$taskRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$taskExecutable=[IO.Path]::GetFullPath((Join-Path $taskRoot $Executable))
if (-not (Test-Path -LiteralPath $taskExecutable)) {throw 'Build jc-tui release before running the smoke test'}
Add-Type -Path @((Join-Path $PSScriptRoot 'conpty-smoke.cs'),(Join-Path $PSScriptRoot 'conpty-git.cs'),(Join-Path $PSScriptRoot 'conpty-git-metrics.cs'))
if ($GitMetrics -and -not $GitExecutable) {throw 'Git metrics require an explicit Git executable'}
if ($GitExecutable) {
    if (-not [IO.Path]::IsPathRooted($GitExecutable)) {throw 'Git executable must be an explicit absolute path'}
    $taskResult=[JcConPtySmoke]::Run($taskExecutable,(Join-Path $taskRoot 'target/release/examples/f3_console_host.exe'),$taskRoot,$false,$false,$GitExecutable,[bool]$GitMetrics,$GitFixtureBytes)
} else {
    $taskResult=[JcConPtySmoke]::Run($taskExecutable,(Join-Path $taskRoot 'target/release/examples/f3_console_host.exe'),$taskRoot,[bool]$ProjectSearch,[bool]$DocumentSearch)
}
$taskJson=($taskResult | ConvertTo-Json).Replace("`r`n","`n")
if ($OutputPath) {
    $taskOutput=[IO.Path]::GetFullPath((Join-Path $taskRoot $OutputPath))
    [IO.File]::WriteAllText($taskOutput,$taskJson+"`n",[Text.UTF8Encoding]::new($false))
    if ($GitMetrics) {[IO.File]::WriteAllLines($taskOutput+'.csv',@([GitAuxiliaryProbe]::Header)+[JcConPtySmoke]::LastGitSamples,[Text.UTF8Encoding]::new($false))}
}
$taskJson
