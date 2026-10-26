# Also copied by Dish's manual-install button: paste into PowerShell.
& {
    $ErrorActionPreference = 'Stop'
    $directory = Join-Path ([IO.Path]::GetTempPath()) ('dish-pi-' + [guid]::NewGuid())
    $script = Join-Path $directory 'install.ps1'
    try {
        New-Item -ItemType Directory -Path $directory | Out-Null
        $url = 'https://pi.dev/install.ps1'
        Write-Host "Baixando $url para $script"
        Invoke-WebRequest -Uri $url -OutFile $script -UseBasicParsing
        $hash = (Get-FileHash -LiteralPath $script -Algorithm SHA256).Hash
        Write-Host "SHA-256: $hash"
        Get-AuthenticodeSignature -LiteralPath $script | Format-List Status, StatusMessage, SignerCertificate
        Write-Host 'Revise o arquivo no editor. O hash identifica o download, mas nao comprova sua origem.'
        Start-Process -FilePath notepad.exe -ArgumentList ('"' + $script + '"') -Wait
        if ((Read-Host 'Digite INSTALAR para executar este arquivo; qualquer outra resposta cancela') -cne 'INSTALAR') {
            Write-Host 'Instalacao cancelada.'
            return
        }
        if ((Get-FileHash -LiteralPath $script -Algorithm SHA256).Hash -ne $hash) {
            throw 'O arquivo mudou durante a revisao. Cancelado; baixe e revise novamente.'
        }
        # Respect the machine's execution policy; never bypass or unblock it.
        & powershell.exe -NoProfile -File $script
        if ($LASTEXITCODE -ne 0) { throw "Instalador terminou com status $LASTEXITCODE" }
    } catch {
        Write-Host "Nao foi possivel instalar o Pi: $_"
    } finally {
        if (Test-Path -LiteralPath $directory) { Remove-Item -LiteralPath $directory -Recurse -Force }
        Read-Host 'Pressione Enter para fechar' | Out-Null
    }
}
