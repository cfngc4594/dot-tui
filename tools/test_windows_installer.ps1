param([string]$Installer = (Join-Path $PSScriptRoot '../install.ps1'))
$ErrorActionPreference='Stop'
$t=$null;$e=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path -LiteralPath $Installer).Path,[ref]$t,[ref]$e)
if($e.Count){throw 'Installer parse errors'}
$functions=@($ast.FindAll({param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -like 'Assert-*'},$false))
foreach($n in $functions){. ([scriptblock]::Create($n.Extent.Text))}
$self=[Security.Principal.WindowsIdentity]::GetCurrent().User
$foreign=New-Object Security.Principal.SecurityIdentifier('S-1-5-21-111111111-222222222-333333333-1001')
foreach($kind in @('DirectorySecurity','FileSecurity')){foreach($right in @('none','Write','Delete','DeleteSubdirectoriesAndFiles','ChangePermissions','TakeOwnership','owner')){
$a=New-Object ('Security.AccessControl.'+$kind);$a.SetOwner($self)
$a.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($self,'FullControl','Allow')))
if($right -eq 'owner'){$a.SetOwner($foreign)}elseif($right -ne 'none'){$a.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($foreign,[Security.AccessControl.FileSystemRights]$right,'Allow')))}
$r=$false;try{Assert-TrustedAcl $a}catch{$r=$true}
if($r -ne ($right -ne 'none')){throw "Unexpected ACL outcome: $kind/$right"}
}}
$root=[IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\','/')
$dir=Join-Path $root ('dot-installer-acl-test-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($dir)|Out-Null
try{
Assert-InstallDirectory $dir
foreach($p in @($root,($root+'-spoof\x'),[IO.Path]::GetPathRoot($root))){$r=$false;try{Assert-InstallDirectory $p}catch{$r=$true};if(!$r){throw "Forbidden path accepted: $p"}}
}finally{Remove-Item -LiteralPath $dir}
Write-Output 'PASS: 14 ACL fixtures + descendant/root/prefix/outside path checks'
