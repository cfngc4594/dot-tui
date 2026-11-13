# dot-tui native Windows installer. Does not modify PATH, execution policy or login.
param([string]$Version = '0.8.8', [string]$InstallDir = "$env:USERPROFILE\.local\bin")
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
# Only the current user, SYSTEM and Administrators may own or mutate install data.
# This inspects ACLs; it never changes them, including inherited permissions.
function Assert-TrustedAcl([Security.AccessControl.FileSystemSecurity]$Acl) {
    $trusted = @([Security.Principal.WindowsIdentity]::GetCurrent().User.Value, 'S-1-5-18', 'S-1-5-32-544')
    $owner = $Acl.GetOwner([Security.Principal.SecurityIdentifier]).Value
    if (!$owner -or $owner -notin $trusted) { throw 'Install directory has an untrusted owner' }
    $mutating = [Security.AccessControl.FileSystemRights]::Write -bor
        [Security.AccessControl.FileSystemRights]::Delete -bor
        [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles -bor
        [Security.AccessControl.FileSystemRights]::ChangePermissions -bor
        [Security.AccessControl.FileSystemRights]::TakeOwnership
    foreach ($rule in $Acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
        # SID translation failures throw. Never silently skip an unrecognized principal.
        $sid = $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
        if (!$sid) { throw 'Cannot resolve directory ACL principal' }
        if ($rule.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and
            $sid -notin $trusted -and ($rule.FileSystemRights -band $mutating)) {
            throw 'Install directory permits an untrusted principal to modify or delete it'
        }
    }
}
function Assert-InstallDirectory([string]$Path) {
    if (!$env:USERPROFILE -or ![IO.Path]::IsPathRooted($env:USERPROFILE)) { throw 'USERPROFILE must be an absolute path' }
    if (![IO.Path]::IsPathRooted($Path)) { throw 'Install directory must be absolute' }
    $root = [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\', '/')
    $full = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    # Windows can alias trailing dots/spaces and NTFS alternate streams; reject them.
    foreach ($part in ($full -split '[\\/]')) {
        if ($part -match '[. ]$' -or ($part.Contains(':') -and $part -notmatch '^[A-Za-z]:$')) { throw 'Ambiguous Windows install path' }
    }
    if (!$full.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Install directory must be a descendant of USERPROFILE'
    }
    Assert-PlainPath $full
    if (!(Test-Path -LiteralPath $root -PathType Container)) { throw 'USERPROFILE must exist as a directory' }
    $current = $full
    while ($true) {
        if (Test-Path -LiteralPath $current) {
            if (!(Test-Path -LiteralPath $current -PathType Container)) { throw 'Install path contains a non-directory' }
            Assert-TrustedAcl (Get-Acl -LiteralPath $current)
        }
        if ($current.Equals($root, [StringComparison]::OrdinalIgnoreCase)) { break }
        $current = [IO.Path]::GetDirectoryName($current)
        if (!$current) { throw 'Install path escaped USERPROFILE' }
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
    Assert-InstallDirectory $InstallDir
    [IO.Directory]::CreateDirectory($InstallDir) | Out-Null
    Assert-InstallDirectory $InstallDir
    $work = Join-Path $InstallDir ('.stage-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($work) | Out-Null
    try {
        Assert-InstallDirectory $work
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
        Assert-InstallDirectory $InstallDir; Assert-PlainPath $destination
        # Retain notices before replacing the binary. Hash-scoped avoids collisions across platforms.
        $licenses = Join-Path $InstallDir "licenses-v$Version-$expected"
        Assert-InstallDirectory $licenses
        [IO.Directory]::CreateDirectory($licenses) | Out-Null
        Assert-InstallDirectory $licenses
        foreach ($name in @('LICENSE','THIRD_PARTY_NOTICES.md','THIRD_PARTY_LICENSES.txt')) {
            $out = Join-Path $licenses $name; Assert-PlainPath $out
            if (Test-Path -LiteralPath $out) { Assert-TrustedAcl (Get-Acl -LiteralPath $out) }
            [IO.File]::Copy((Join-Path $work $name), $out, $true)
        }
        Assert-InstallDirectory $InstallDir
        Assert-PlainPath $destination
        if (Test-Path -LiteralPath $destination) {
            # File.Replace preserves the old file ACL; validate it as well as its directory.
            Assert-TrustedAcl (Get-Acl -LiteralPath $destination)
            [IO.File]::Replace($exe, $destination, $null)
        }
        else { [IO.File]::Move($exe, $destination) }
        Write-Host "Installed dot-tui $Version at $destination"
        Write-Host "For this PowerShell session: `$env:PATH = '$($InstallDir.Replace("'","''"));' + `$env:PATH"
        Write-Host 'Then run dot-tui. Add the directory to your user PATH manually for future shells.'
        Write-Host 'Sign in through official Codex first; installer ran only dot-tui --version, without opening the TUI or accessing credentials.'
    } finally { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }
}
Install-DotTui
