<#
.SYNOPSIS
Installs review-buddy on Windows: downloads, verifies and installs a release build.

.EXAMPLE
irm https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.ps1 | iex

.EXAMPLE
.\install.ps1 -Version v0.1.0 -InstallDir D:\Tools\review-buddy -Yes

.NOTES
Testing only: the RB_INSTALL_BASE_URL environment variable points at a directory (or file:// URL) holding the release assets and a VERSION file.
#>
[CmdletBinding()]
param(
    [string]$Version = $env:VERSION,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\review-buddy'),
    [switch]$Uninstall,
    [switch]$DryRun,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = 'smorrisods/review-buddy'
$ManifestName = 'install-manifest.txt'

function Write-Note([string]$Text) { Write-Host $Text }

function Get-LocalPath([string]$Url) {
    if ($Url -like 'file://*') {
        return ([Uri]$Url).LocalPath
    }
    return $null
}

function Get-RemoteFile([string]$Url, [string]$Dest) {
    $local = Get-LocalPath $Url
    if ($local) {
        Copy-Item -LiteralPath $local -Destination $Dest -Force
        return
    }
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $headers = @{}
    if ($env:GITHUB_TOKEN -and $Url -like 'https://api.github.com/*') {
        $headers['Authorization'] = "Bearer $($env:GITHUB_TOKEN)"
    }
    Invoke-WebRequest -Uri $Url -OutFile $Dest -Headers $headers -UseBasicParsing
}

function Test-InteractiveSession {
    return [Environment]::UserInteractive -and -not [Console]::IsInputRedirected
}

function Test-OnUserPath([string]$Dir) {
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $current) { return $false }
    $trimmed = $Dir.TrimEnd('\')
    foreach ($entry in $current.Split(';')) {
        if ($entry.TrimEnd('\') -ieq $trimmed) { return $true }
    }
    return $false
}

function Confirm-PathUpdate([string]$Dir) {
    if ($Yes) { return $true }
    if (-not (Test-InteractiveSession)) { return $false }
    $answer = Read-Host "Add $Dir to your user PATH? [y/N]"
    return $answer -match '^(y|yes)$'
}

Write-Host 'review buddy installer'

if ($Uninstall) {
    $manifest = Join-Path $InstallDir $ManifestName
    if (-not (Test-Path -LiteralPath $manifest)) {
        Write-Note "I couldn't find an install manifest in $InstallDir, so there's nothing to remove."
        return
    }
    Write-Note "Removing review-buddy from $InstallDir"
    foreach ($rel in Get-Content -LiteralPath $manifest) {
        if (-not $rel -or $rel -match '\.\.') { continue }
        $target = Join-Path $InstallDir $rel
        if ($DryRun) {
            Write-Note "  would remove $target"
        } else {
            Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
            Write-Note "  removed $target"
        }
    }
    if (-not $DryRun) {
        Remove-Item -LiteralPath $manifest -Force
        Get-ChildItem -LiteralPath $InstallDir -Directory -Recurse -ErrorAction SilentlyContinue |
            Sort-Object { $_.FullName.Length } -Descending |
            Where-Object { -not (Get-ChildItem -LiteralPath $_.FullName -Force) } |
            Remove-Item -Force
        if (-not (Get-ChildItem -LiteralPath $InstallDir -Force)) { Remove-Item -LiteralPath $InstallDir -Force }
        if (Test-OnUserPath $InstallDir) {
            Write-Note "$InstallDir is still on your user PATH. Remove it in Settings > System > About > Advanced system settings if you like."
        }
    }
    Write-Note 'Done. Your config and cache are untouched.'
    return
}

$arch = if ($env:RB_INSTALL_ARCH) { $env:RB_INSTALL_ARCH } else { $env:PROCESSOR_ARCHITECTURE }
switch ($arch) {
    'AMD64' { $target = 'windows-amd64' }
    'ARM64' { $target = 'windows-arm64' }
    default { throw "Unsupported CPU architecture '$arch'. Windows builds exist for AMD64 and ARM64; see https://github.com/$Repo/releases." }
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("review-buddy-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    if ($env:RB_INSTALL_BASE_URL) {
        $base = $env:RB_INSTALL_BASE_URL.TrimEnd('/')
    }
    if (-not $Version) {
        if ($base) {
            Get-RemoteFile "$base/VERSION" (Join-Path $work 'version')
            $Version = (Get-Content -LiteralPath (Join-Path $work 'version') -Raw).Trim()
        } else {
            Get-RemoteFile "https://api.github.com/repos/$Repo/releases/latest" (Join-Path $work 'latest.json')
            $Version = (Get-Content -LiteralPath (Join-Path $work 'latest.json') -Raw | ConvertFrom-Json).tag_name
        }
    }
    if (-not $Version) { throw "Couldn't work out the latest release. Pass -Version vX.Y.Z." }
    $tag = if ($Version.StartsWith('v')) { $Version } else { "v$Version" }
    $num = $tag.Substring(1)
    if (-not $base) { $base = "https://github.com/$Repo/releases/download/$tag" }
    $asset = "review-buddy-$num-$target.zip"

    Write-Note "Installing review-buddy $tag ($target) into $InstallDir"
    if ($DryRun) {
        Write-Note 'Dry run: nothing will be downloaded or changed.'
        Write-Note "  would download $base/$asset"
        Write-Note "  would download $base/SHA256SUMS and verify the archive"
        Write-Note "  would install review-buddy.exe and its themes into $InstallDir"
        if (-not (Test-OnUserPath $InstallDir)) { Write-Note '  would offer to add the folder to your user PATH' }
        return
    }

    $zip = Join-Path $work $asset
    $sums = Join-Path $work 'SHA256SUMS'
    Get-RemoteFile "$base/$asset" $zip
    Get-RemoteFile "$base/SHA256SUMS" $sums

    $expected = $null
    foreach ($line in Get-Content -LiteralPath $sums) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) { $expected = $parts[0]; break }
    }
    if (-not $expected) { throw "SHA256SUMS has no entry for $asset. Nothing was installed." }
    $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
    if ($actual -ine $expected) {
        throw "Checksum mismatch for $asset (expected $expected, got $actual). Nothing was installed."
    }
    Write-Note 'Checksum verified.'

    $stage = Join-Path $work 'stage'
    Expand-Archive -LiteralPath $zip -DestinationPath $stage -Force
    if (-not (Test-Path -LiteralPath (Join-Path $stage 'review-buddy.exe'))) {
        throw 'The archive has no review-buddy.exe. Nothing was installed.'
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $manifest = Join-Path $InstallDir $ManifestName
    $installed = @()
    $stageRoot = (Resolve-Path -LiteralPath $stage).Path.TrimEnd('\') + '\'
    foreach ($file in Get-ChildItem -LiteralPath $stage -File -Recurse) {
        $rel = $file.FullName.Substring($stageRoot.Length)
        $dest = Join-Path $InstallDir $rel
        New-Item -ItemType Directory -Path (Split-Path -Parent $dest) -Force | Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $dest -Force
        $installed += $rel
    }
    Unblock-File -LiteralPath (Join-Path $InstallDir 'review-buddy.exe')
    $installed + $ManifestName | Set-Content -LiteralPath $manifest -Encoding ASCII
    Write-Note "Installed $($installed.Count) files."

    if (-not (Test-OnUserPath $InstallDir)) {
        if (Confirm-PathUpdate $InstallDir) {
            $current = [Environment]::GetEnvironmentVariable('Path', 'User')
            $new = if ($current) { "$current;$InstallDir" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable('Path', $new, 'User')
            Write-Note 'Added to your user PATH. Open a new terminal to pick it up.'
        } else {
            Write-Note "$InstallDir isn't on your PATH. Re-run with -Yes to add it, or add it yourself."
        }
    }

    Write-Host ''
    Write-Note 'Next steps'
    Write-Note '  review-buddy --demo         explore with no network or account'
    Write-Note '  review-buddy auth status    check your GitHub and GitLab sign-in'
    Write-Note 'To remove it later, run this installer again with -Uninstall.'
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
