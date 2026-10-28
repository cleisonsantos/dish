param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$OutputPath
)
$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$') {
    throw 'MSI Version must be numeric with four parts; use package-windows.py numeric_version(label).'
}
$parts = $Version.Split('.') | ForEach-Object { [uint64]$_ }
if ($parts[0] -gt 255 -or $parts[1] -gt 255 -or $parts[2] -gt 65535 -or $parts[3] -gt 65535) {
    throw 'MSI Version exceeds Windows Installer limits (255.255.65535.65535).'
}
$wix = Get-Command wix -ErrorAction SilentlyContinue
if (-not $wix) { throw 'WiX not found. Install wix 4.0.6 and WixToolset.UI.wixext/4.0.6.' }

# Product identity: NEVER regenerate this code between releases.
$UpgradeCode = '8B5C069E-5489-43A7-92FC-A668810CD0DB'
$root = Split-Path -Parent $PSScriptRoot
$payload = Join-Path $root 'dist/windows-payload'
foreach ($name in @('dish.exe', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'BUILD.json')) {
    if (-not (Test-Path -LiteralPath (Join-Path $payload $name) -PathType Leaf)) {
        throw "Missing MSI payload: $name; run package-windows.py first."
    }
}
if (-not (Test-Path -LiteralPath (Join-Path $payload 'licenses') -PathType Container)) {
    throw 'Missing MSI license bundle; run package-windows.py first.'
}
$helper = Join-Path $PSScriptRoot 'install-pi-windows.ps1'
$icon = Join-Path $root 'assets/dish-icon.ico'
foreach ($path in @($helper, $icon)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing MSI input: $path" }
}

function Escape-Xml([string]$Value) {
    return [Security.SecurityElement]::Escape($Value)
}

# IDs (and WiX's registry-based component GUIDs) depend on relative paths,
# never on the build directory, release version, or traversal order.
function Get-StableId([string]$Prefix, [string]$RelativePath) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($RelativePath.Replace('\', '/').ToLowerInvariant())
    $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
    return $Prefix + $hash.Substring(0, 32)
}

$componentRefs = [Collections.Generic.List[string]]::new()
function Add-PayloadFile([IO.FileInfo]$File, [string]$RelativePath) {
    $id = Get-StableId 'C' $RelativePath
    $fileId = if ($RelativePath -eq 'dish.exe') { 'DishExe' } else { Get-StableId 'F' $RelativePath }
    $componentRefs.Add("<ComponentRef Id=`"$id`" />")
    # Per-user components use HKCU key paths, including non-advertised shortcuts.
    return @"
<Component Id="$id" Guid="*">
  <RegistryValue Root="HKCU" Key="Software\Dish\MSI\Components" Name="$id" Type="integer" Value="1" KeyPath="yes" />
  <File Id="$fileId" Source="$(Escape-Xml $File.FullName)" Name="$(Escape-Xml $File.Name)" />
</Component>
"@
}

function Add-PayloadDirectory([IO.DirectoryInfo]$Directory, [string]$RelativePath) {
    $id = Get-StableId 'D' $RelativePath
    $cleanup = Get-StableId 'R' $RelativePath
    $componentRefs.Add("<ComponentRef Id=`"$cleanup`" />")
    $xml = [Text.StringBuilder]::new()
    [void]$xml.AppendLine(@"
<Component Id="$cleanup" Guid="*">
  <RegistryValue Root="HKCU" Key="Software\Dish\MSI\Components" Name="$cleanup" Type="integer" Value="1" KeyPath="yes" />
  <RemoveFolder Id="$cleanup" On="uninstall" />
</Component>
"@)
    foreach ($file in Get-ChildItem -LiteralPath $Directory.FullName -File -Force | Sort-Object Name) {
        [void]$xml.AppendLine((Add-PayloadFile $file "$RelativePath/$($file.Name)"))
    }
    foreach ($child in Get-ChildItem -LiteralPath $Directory.FullName -Directory -Force | Sort-Object Name) {
        [void]$xml.AppendLine((Add-PayloadDirectory $child "$RelativePath/$($child.Name)"))
    }
    return "<Directory Id=`"$id`" Name=`"$(Escape-Xml $Directory.Name)`">`n$xml</Directory>"
}

# Match the explicit NSIS file list; do not harvest unrelated staging files.
$files = foreach ($name in @('dish.exe', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'BUILD.json')) {
    Add-PayloadFile (Get-Item -LiteralPath (Join-Path $payload $name)) $name
}
$files += Add-PayloadFile (Get-Item -LiteralPath $helper) 'install-pi-windows.ps1'
$licenses = Add-PayloadDirectory (Get-Item -LiteralPath (Join-Path $payload 'licenses')) 'licenses'

$OutputPath = [IO.Path]::GetFullPath($OutputPath)
[void][IO.Directory]::CreateDirectory((Split-Path -Parent $OutputPath))
$work = Join-Path ([IO.Path]::GetTempPath()) ('dish-msi-' + [IO.Path]::GetRandomFileName())
[void][IO.Directory]::CreateDirectory($work)
try {
    # WixUI expects RTF. Escape the same license shown by NSIS.
    $licenseText = [IO.File]::ReadAllText((Join-Path $payload 'LICENSE'))
    $rtf = $licenseText.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}').Replace("`r", '').Replace("`n", '\par ')
    $licensePath = Join-Path $work 'license.rtf'
    [IO.File]::WriteAllText($licensePath, '{\rtf1\ansi ' + $rtf + '}', [Text.UTF8Encoding]::new($false))
    $source = @"
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs" xmlns:ui="http://wixtoolset.org/schemas/v4/wxs/ui">
  <Package Name="Dish" Manufacturer="Cleison Santos" Version="$Version" UpgradeCode="$UpgradeCode" Scope="perUser" Language="1033">
    <!-- MSI ignores the fourth version field. Include equal three-part versions
         so prerelease updates replace the existing product instead of duplicating it. -->
    <MajorUpgrade AllowSameVersionUpgrades="yes" Schedule="afterInstallInitialize" DowngradeErrorMessage="A newer version of Dish is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <Icon Id="DishIcon" SourceFile="$(Escape-Xml $icon)" />
    <Property Id="ARPPRODUCTICON" Value="DishIcon" />
    <Property Id="INSTALLLEVEL" Value="1" />
    <StandardDirectory Id="LocalAppDataFolder">
      <Directory Id="UserProgramsFolder" Name="Programs">
        <Directory Id="INSTALLFOLDER" Name="Dish">
          $($files -join "`n")
          $licenses
          <Component Id="InstallDirectory" Guid="*">
            <RegistryValue Root="HKCU" Key="Software\Dish" Name="InstallDir" Type="string" Value="[INSTALLFOLDER]" KeyPath="yes" />
            <RemoveFolder Id="RemoveInstallDirectory" On="uninstall" />
            <RemoveFolder Id="RemoveUserProgramsDirectory" Directory="UserProgramsFolder" On="uninstall" />
          </Component>
        </Directory>
      </Directory>
    </StandardDirectory>
    <StandardDirectory Id="ProgramMenuFolder">
      <Directory Id="DishMenuFolder" Name="Dish">
        <Component Id="StartMenuShortcut" Guid="*">
          <Shortcut Id="StartDish" Name="Dish" Target="[#DishExe]" WorkingDirectory="INSTALLFOLDER" Icon="DishIcon" />
          <RemoveFolder Id="RemoveDishMenu" On="uninstall" />
          <RegistryValue Root="HKCU" Key="Software\Dish\MSI" Name="StartMenuShortcut" Type="integer" Value="1" KeyPath="yes" />
        </Component>
      </Directory>
    </StandardDirectory>
    <StandardDirectory Id="DesktopFolder">
      <Component Id="DesktopShortcut" Guid="*">
        <Shortcut Id="DesktopDish" Name="Dish" Target="[#DishExe]" WorkingDirectory="INSTALLFOLDER" Icon="DishIcon" />
        <RegistryValue Root="HKCU" Key="Software\Dish\MSI" Name="DesktopShortcut" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </StandardDirectory>
    <Feature Id="Dish" Title="Dish" Level="1" Absent="disallow" AllowAdvertise="no" Display="expand">
      $($componentRefs -join "`n")
      <ComponentRef Id="InstallDirectory" />
      <ComponentRef Id="StartMenuShortcut" />
      <Feature Id="DesktopShortcutFeature" Title="Desktop shortcut" Description="Create a Dish shortcut on your desktop." Level="2" AllowAdvertise="no">
        <ComponentRef Id="DesktopShortcut" />
      </Feature>
    </Feature>
    <ui:WixUI Id="WixUI_FeatureTree" />
    <WixVariable Id="WixUILicenseRtf" Value="$(Escape-Xml $licensePath)" />
  </Package>
</Wix>
"@
    $sourcePath = Join-Path $work 'Dish.wxs'
    [IO.File]::WriteAllText($sourcePath, $source, [Text.UTF8Encoding]::new($false))
    & $wix.Source build $sourcePath -arch x64 -ext WixToolset.UI.wixext/4.0.6 -pdbtype none -o $OutputPath
    if ($LASTEXITCODE -ne 0) { throw "WiX MSI build failed (exit $LASTEXITCODE)." }
    if (-not (Test-Path -LiteralPath $OutputPath -PathType Leaf) -or (Get-Item -LiteralPath $OutputPath).Length -eq 0) {
        throw "WiX did not produce a nonempty MSI: $OutputPath"
    }
    Write-Host "MSI: $OutputPath (version $Version, UpgradeCode $UpgradeCode)"
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force
}
