# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
  Build non-executable MSI metadata fixtures of the launcher and check their policy; never install them.
.DESCRIPTION
  The payload files are placeholders, so the MSIs can't install anything useful. They check the
  compiled paths, identity and registration (check-install-policy.ps1) for each architecture,
  without a release build. For the first architecture it also builds packages whose uninstall
  cleanup was changed (run on updates too, run after the files are gone, joined by another custom
  action) and checks that the policy refuses each. Needs WiX 5.0.2 with its UI and Util extensions.
#>
param(
  [Parameter(Mandatory = $true)] [string] $OutputDirectory,
  [ValidateSet('x64','x86','arm64')] [string[]] $Architectures = @('x64','x86','arm64')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).ProviderPath
$policy = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'install-policy.json') | ConvertFrom-Json
[void](New-Item -ItemType Directory -Force -Path $OutputDirectory)
$output = (Resolve-Path -LiteralPath $OutputDirectory).ProviderPath
Add-Type -AssemblyName System.Drawing
function Build-Fixture([string] $Arch, [string] $Stage, [string[]] $Sources, [string] $Msi) {
  $extensions = @($policy.extensions | ForEach-Object { '-ext'; "WixToolset.$_.wixext" })
  & wix build @Sources -arch $Arch @extensions `
    -culture en-US -d 'Version=0.0.1' -d 'DisplayVersion=0.0.1-metadata-test' `
    -d "BinDir=$Stage" -d "IconPath=$(Join-Path $root $policy.icon)" `
    -d "BannerBmp=$(Join-Path $Stage 'banner.bmp')" -d "DialogBmp=$(Join-Path $Stage 'dialog.bmp')" `
    -o $Msi @($policy.localization | ForEach-Object { '-loc'; Join-Path $PSScriptRoot $_ })
  if ($LASTEXITCODE -ne 0) { throw "WiX build failed for $Msi (exit $LASTEXITCODE)." }
}
foreach ($arch in $Architectures) {
  $stage = Join-Path $output $arch
  [void](New-Item -ItemType Directory -Force -Path $stage)
  foreach ($name in $policy.payloads) {
    Set-Content -LiteralPath (Join-Path $stage $name) -Encoding Ascii -Value 'NON-EXECUTABLE MSI METADATA TEST FIXTURE'
  }
  foreach ($spec in @(@('banner.bmp', 493, 58), @('dialog.bmp', 493, 312))) {
    $bitmap = [Drawing.Bitmap]::new($spec[1], $spec[2], [Drawing.Imaging.PixelFormat]::Format24bppRgb)
    try { $bitmap.Save((Join-Path $stage $spec[0]), [Drawing.Imaging.ImageFormat]::Bmp) }
    finally { $bitmap.Dispose() }
  }
  $sources = @($policy.sources | ForEach-Object { Join-Path $PSScriptRoot $_ })
  $msi = Join-Path $stage "$($policy.id)-NOT-INSTALLABLE.msi"
  Build-Fixture $arch $stage $sources $msi
  & (Join-Path $PSScriptRoot 'check-install-policy.ps1') -MsiPath $msi
  Write-Output "PASS $arch metadata-only package. This is not an executable installer."
  if ($arch -ne $Architectures[0]) { continue }
  # The policy must refuse an uninstall cleanup that runs on updates too, one that runs once the
  # files are gone, and any other custom action beside it.
  $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'open3d-launcher.wxs')
  $condition = 'Condition="REMOVE=&quot;ALL&quot; AND NOT UPGRADINGPRODUCTCODE"'
  $other = '<CustomAction Id="Open3DOther" FileRef="Open3DLauncherExe" ExeCommand="--hidden" Execute="deferred" Impersonate="yes" Return="ignore" />' +
    '<InstallExecuteSequence><Custom Action="Open3DOther" Before="RemoveFiles" Condition="REMOVE=&quot;ALL&quot;" /></InstallExecuteSequence></Package>'
  $changed = [ordered]@{
    'runs on updates too' = $source.Replace($condition, 'Condition="REMOVE=&quot;ALL&quot;"')
    'runs once the files are gone' = $source.Replace('Action="Open3DUninstallCleanup" Before="RemoveFiles"', 'Action="Open3DUninstallCleanup" After="RemoveFiles"')
    'has another custom action beside it' = $source.Replace('</Package>', $other)
  }
  $number = 0
  foreach ($what in $changed.Keys) {
    if ($changed[$what] -eq $source) { throw "The fixture whose cleanup $what changed nothing in open3d-launcher.wxs." }
    $number++
    $wxs = Join-Path $stage "changed-$number.wxs"
    Set-Content -LiteralPath $wxs -Encoding UTF8 -Value $changed[$what]
    $changedMsi = Join-Path $stage "$($policy.id)-CHANGED-$number-NOT-INSTALLABLE.msi"
    Build-Fixture $arch $stage @($wxs) $changedMsi
    $refusal = $null
    try { & (Join-Path $PSScriptRoot 'check-install-policy.ps1') -MsiPath $changedMsi | Out-Null }
    catch { $refusal = $_.Exception.Message }
    if (-not $refusal) { throw "check-install-policy.ps1 accepted a package whose uninstall cleanup $what." }
    if ($refusal -notmatch 'custom action|uninstall cleanup') { throw "A package whose uninstall cleanup $what was refused for another reason: $refusal" }
    Write-Output "PASS refused a package whose uninstall cleanup $what ($refusal)"
  }
}
