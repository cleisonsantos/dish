param([Parameter(Mandatory)][string]$Path)
$ErrorActionPreference = 'Stop'
if (-not $env:WINDOWS_CERT_PFX_BASE64 -and -not $env:WINDOWS_CERT_PASSWORD) {
    Write-Host 'Signing secrets absent: building unsigned artifacts.'
    exit 0
}
if (-not $env:WINDOWS_CERT_PFX_BASE64 -or -not $env:WINDOWS_CERT_PASSWORD) {
    throw 'Configure both WINDOWS_CERT_PFX_BASE64 and WINDOWS_CERT_PASSWORD.'
}
$files = @(Get-Item -Path $Path -ErrorAction Stop)
if ($files.Count -eq 0) { throw "No files to sign: $Path" }
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
if (-not $signtool) { throw 'Windows SDK signtool.exe not found' }
$pfx = Join-Path $env:RUNNER_TEMP ('dish-sign-' + [guid]::NewGuid() + '.pfx')
$certificates = @()
try {
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX_BASE64))
    $password = ConvertTo-SecureString $env:WINDOWS_CERT_PASSWORD -AsPlainText -Force
    $certificates = @(Import-PfxCertificate -FilePath $pfx -CertStoreLocation Cert:\CurrentUser\My -Password $password)
    $signers = @($certificates | Where-Object HasPrivateKey)
    if ($signers.Count -ne 1) { throw 'PFX must contain exactly one signing certificate with private key' }
    foreach ($file in $files) {
        & $signtool.FullName sign /sha1 $signers[0].Thumbprint /s My /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $file.FullName
        if ($LASTEXITCODE -ne 0) { throw "Signing failed: $($file.Name)" }
        & $signtool.FullName verify /pa /v $file.FullName
        if ($LASTEXITCODE -ne 0) { throw "Signature verification failed: $($file.Name)" }
        if ($file.Name -like '*-setup.exe') {
            $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            [IO.File]::WriteAllText($file.FullName + '.sha256', "$hash  $($file.Name)`n", [Text.UTF8Encoding]::new($false))
        }
    }
} finally {
    Remove-Item -LiteralPath $pfx -Force -ErrorAction SilentlyContinue
    foreach ($certificate in $certificates) {
        Remove-Item "Cert:\CurrentUser\My\$($certificate.Thumbprint)" -DeleteKey -ErrorAction SilentlyContinue
    }
}
