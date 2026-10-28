param([Parameter(Mandatory)][string[]]$Path)
$ErrorActionPreference = 'Stop'

$signtool = (Get-Command signtool.exe -CommandType Application -ErrorAction SilentlyContinue).Source
if (-not $signtool) {
    $sdkTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -File -ErrorAction SilentlyContinue |
        Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
    if (-not $sdkTool) { throw 'Windows SDK signtool.exe not found' }
    $signtool = $sdkTool.FullName
}

foreach ($pattern in $Path) {
    $files = @(Get-Item -Path $pattern -ErrorAction Stop)
    if ($files.Count -eq 0) { throw "No files to verify: $pattern" }
    foreach ($file in $files) {
        if ($file.PSIsContainer) { throw "Expected a file to verify: $($file.FullName)" }
        & $signtool verify /pa /v $file.FullName
        if ($LASTEXITCODE -ne 0) { throw "Signature verification failed: $($file.FullName) (signtool exit $LASTEXITCODE)" }
    }
}
