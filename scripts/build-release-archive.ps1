<#
.SYNOPSIS
Builds a release .zip for the review-buddy Windows binary.

.DESCRIPTION
Writes <OutputDir>\review-buddy-<version>-<Target>.zip containing review-buddy.exe at
the root plus themes, config.example.toml, LICENSE and README.md, and prints its path.

.EXAMPLE
scripts\build-release-archive.ps1 -Version v0.1.0 -Target x86_64-pc-windows-msvc -Binary target\x86_64-pc-windows-msvc\release\review-buddy.exe
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Target,
    [Parameter(Mandatory = $true)][string]$Binary,
    [string]$OutputDir = 'dist'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
    throw "Built binary not found at $Binary"
}

$VersionNoV = $Version -replace '^v', ''
$Stage = Join-Path ([System.IO.Path]::GetTempPath()) ("review-buddy-" + [System.Guid]::NewGuid().ToString('N'))
$Root = Join-Path $Stage 'review-buddy'

try {
    New-Item -ItemType Directory -Path $Root | Out-Null
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $Root 'review-buddy.exe')
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'themes') -Destination (Join-Path $Root 'themes') -Recurse
    foreach ($file in 'config.example.toml', 'LICENSE', 'README.md') {
        Copy-Item -LiteralPath (Join-Path $RepoRoot $file) -Destination $Root
    }

    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    $Archive = Join-Path (Resolve-Path -LiteralPath $OutputDir) "review-buddy-$VersionNoV-$Target.zip"
    if (Test-Path -LiteralPath $Archive) { Remove-Item -LiteralPath $Archive }
    Compress-Archive -Path (Join-Path $Root '*') -DestinationPath $Archive
    Write-Output $Archive
}
finally {
    Remove-Item -LiteralPath $Stage -Recurse -Force -ErrorAction SilentlyContinue
}
