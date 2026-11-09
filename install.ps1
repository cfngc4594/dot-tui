# dot-tui native Windows installer. Does not modify PATH, execution policy or login.
param([string]$Version = '0.8.6', [string]$InstallDir = "$env:LOCALAPPDATA\dot-tui\bin")
$ErrorActionPreference = 'Stop'
function Assert-PlainPath([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing reparse point: $current" }
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }; $current = $parent
    }
}
function Fetch([string]$Url, [string]$Path) {
    & curl.exe --disable --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 15 --max-time 180 --retry 2 --output $Path $Url
    if ($LASTEXITCODE -ne 0) { throw 'Download failed; nothing installed' }
}
# Read a named tar member as bytes, not PowerShell's text pipeline (5.1 corrupts EXEs).
function Extract-Member([string]$Archive, [string]$Name, [string]$Destination) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = (Get-Command tar.exe -ErrorAction Stop).Source
    $info.Arguments = '-xOzf "' + $Archive + '" ' + $Name
    $info.UseShellExecute = $false; $info.RedirectStandardOutput = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stream = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew)
        try { $process.StandardOutput.BaseStream.CopyTo($stream); $stream.Flush() } finally { $stream.Dispose() }
        $process.WaitForExit()
        if ($process.ExitCode -ne 0 -or (Get-Item -LiteralPath $Destination).Length -eq 0) { throw "Cannot extract $Name" }
    } finally { $process.Dispose() }
}
function Install-DotTui {
    if ($env:OS -ne 'Windows_NT') { throw 'Use install.sh on macOS/Linux' }
    if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Version must be X.Y.Z' }
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (!$arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    if ($arch -ne 'AMD64') { throw 'Only native Windows x86_64 is available' }
    if (![IO.Path]::IsPathRooted($InstallDir)) { throw 'InstallDir must be absolute' }
    if ($InstallDir.Contains('"')) { throw 'Invalid install path' }
    Get-Command curl.exe, tar.exe -ErrorAction Stop | Out-Null
    $InstallDir = [IO.Path]::GetFullPath($InstallDir)
    Assert-PlainPath $InstallDir
    [IO.Directory]::CreateDirectory($InstallDir) | Out-Null
    # Reject broad write ACLs; never change permissions or require Administrator.
    $acl = Get-Acl -LiteralPath $InstallDir
    foreach ($rule in $acl.Access) {
        $sid = $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
        if ($sid -in @('S-1-1-0','S-1-5-11','S-1-5-32-545') -and $rule.AccessControlType -eq 'Allow' -and ($rule.FileSystemRights -band [Security.AccessControl.FileSystemRights]::Write)) { throw 'Install directory permits other users to write; choose a private directory' }
    }
    $work = Join-Path $InstallDir ('.stage-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($work) | Out-Null
    try {
        $asset = "dot-tui-v$Version-x86_64-pc-windows-msvc.tar.gz"
        $base = "https://github.com/cfngc4594/dot-tui/releases/download/v$Version"
        Fetch "$base/SHA256SUMS" (Join-Path $work 'SHA256SUMS')
        $matchesFound = @(Get-Content -LiteralPath (Join-Path $work 'SHA256SUMS') | Where-Object { $_ -match ('^[a-f0-9]{64}  ' + [Regex]::Escape($asset) + '$') })
        if ($matchesFound.Count -ne 1) { throw 'Missing or duplicate archive checksum' }
        $expected = $matchesFound[0].Substring(0,64)
        $archive = Join-Path $work $asset
        Fetch "$base/$asset" $archive
        if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw 'Archive checksum mismatch' }
        Extract-Member $archive 'VERSION' (Join-Path $work 'VERSION')
        if ((Get-Content -Raw -LiteralPath (Join-Path $work 'VERSION')).Trim() -ne $Version) { throw 'Archive version mismatch' }
        foreach ($name in @('dot-tui.exe','LICENSE','THIRD_PARTY_NOTICES.md','THIRD_PARTY_LICENSES.txt')) { Extract-Member $archive $name (Join-Path $work $name) }
        $exe = Join-Path $work 'dot-tui.exe'
        $stream = [IO.File]::OpenRead($exe)
        $reader = New-Object IO.BinaryReader($stream)
        try {
            if ($reader.ReadUInt16() -ne 0x5a4d) { throw 'Not a Windows executable' }
            $stream.Position = 0x3c; $offset = $reader.ReadUInt32(); $stream.Position = $offset
            if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) { throw 'Not an x86_64 PE executable' }
        } finally { $reader.Dispose() }
        # Clap handles --version before runtime/auth initialization.
        $versionInfo = New-Object Diagnostics.ProcessStartInfo
        $versionInfo.FileName = $exe; $versionInfo.Arguments = '--version'
        $versionInfo.UseShellExecute = $false
        $versionInfo.RedirectStandardOutput = $true
        $versionProcess = [Diagnostics.Process]::Start($versionInfo)
        try {
            if (!$versionProcess.WaitForExit(15000)) {
                $versionProcess.Kill()
                throw 'Executable version check timed out; previous installation retained'
            }
            $reportedVersion = $versionProcess.StandardOutput.ReadToEnd().TrimEnd("`r", "`n")
            if ($versionProcess.ExitCode -ne 0 -or $reportedVersion -cne "dot-tui $Version") {
                throw 'Executable version check failed or mismatched; previous installation retained'
            }
        } finally { $versionProcess.Dispose() }
        $destination = Join-Path $InstallDir 'dot-tui.exe'
        Assert-PlainPath $InstallDir; Assert-PlainPath $destination
        # Retain notices before replacing the binary. Hash-scoped avoids collisions across platforms.
        $licenses = Join-Path $InstallDir "licenses-v$Version-$expected"
        Assert-PlainPath $licenses
        [IO.Directory]::CreateDirectory($licenses) | Out-Null
        foreach ($name in @('LICENSE','THIRD_PARTY_NOTICES.md','THIRD_PARTY_LICENSES.txt')) {
            $out = Join-Path $licenses $name; Assert-PlainPath $out
            [IO.File]::Copy((Join-Path $work $name), $out, $true)
        }
        if (Test-Path -LiteralPath $destination) { [IO.File]::Replace($exe, $destination, $null) }
        else { [IO.File]::Move($exe, $destination) }
        Write-Host "Installed dot-tui $Version at $destination"
        Write-Host "For this PowerShell session: `$env:PATH = '$($InstallDir.Replace("'","''"));' + `$env:PATH"
        Write-Host 'Then run dot-tui. Add the directory to your user PATH manually for future shells.'
        Write-Host 'Sign in through official Codex first; installer ran only dot-tui --version, without opening the TUI or accessing credentials.'
    } finally { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }
}
Install-DotTui
