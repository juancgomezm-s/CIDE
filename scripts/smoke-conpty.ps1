param([string]$Executable='target/release/jc-ide-tui.exe',[string]$OutputPath='docs/benchmarks/F3-conpty.json')
$ErrorActionPreference='Stop'
$taskRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$taskExecutable=[IO.Path]::GetFullPath((Join-Path $taskRoot $Executable))
if (-not (Test-Path -LiteralPath $taskExecutable)) {throw 'Build jc-tui release before running the smoke test'}
Add-Type -Path (Join-Path $PSScriptRoot 'conpty-smoke.cs')
$taskResult=[JcConPtySmoke]::Run($taskExecutable,(Join-Path $taskRoot 'target/release/examples/f3_console_host.exe'),$taskRoot)
$taskJson=($taskResult | ConvertTo-Json).Replace("`r`n","`n")
if ($OutputPath) {
    $taskOutput=[IO.Path]::GetFullPath((Join-Path $taskRoot $OutputPath))
    [IO.File]::WriteAllText($taskOutput,$taskJson+"`n",[Text.UTF8Encoding]::new($false))
}
$taskJson
