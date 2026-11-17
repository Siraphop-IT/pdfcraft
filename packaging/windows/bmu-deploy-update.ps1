<#
.SYNOPSIS
  Publish a BMU PDF installer to the office NAS, where every installed copy finds it.

.DESCRIPTION
  BMU PDF checks http://<NAS>:8888/bmu-pdf/update.json at start and on Help > Check for updates
  (apps/pdfcraft/src/update.rs). This script signs the installer's manifest with the release key
  (cargo xtask bmu-update sign), then uploads the installer, update.json and update.json.sig into
  the Web Station folder that port 8888 serves, and reads update.json back over that address.

  The installer goes up first: until update.json and its signature both describe it, no copy is
  offered anything (a manifest whose signature doesn't match is ignored, never half-believed).

  NAS address and login come from BMU Drive's own settings on this machine
  (%APPDATA%\BmuDrive\appsettings.local.json) and are never printed. Nothing is written to the repo.

.EXAMPLE
  ./packaging/windows/bmu-deploy-update.ps1 -Msi 'D:\BMU PDF Setup\bmu-pdf-0.18.42-windows-x64.msi' -Notes 'ลากแท็บไปรวมกับอีกหน้าต่างได้'
  ./packaging/windows/bmu-deploy-update.ps1 -Msi … -WhatIf     # sign and show, upload nothing
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
  [Parameter(Mandatory)] [string] $Msi,
  # What changed, shown in the update prompt (Thai is fine).
  [string] $Notes = '',
  [string] $NotesFile,
  # The release key (cargo xtask bmu-update keygen); kept off the repo.
  [string] $Key = (Join-Path $env:USERPROFILE 'BMU PDF Release\update-signing.key'),
  # The FileStation folder Web Station serves as http://<NAS>:8888/bmu-pdf/.
  [string] $NasFolder = '/web/BMU_Drive/bmu-pdf',
  # Where the app reads it, to check the upload.
  [string] $CheckUrl = 'http://192.168.1.100:8888/bmu-pdf/update.json',
  [string] $ConfigPath = (Join-Path $env:APPDATA 'BmuDrive\appsettings.local.json')
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

if (-not (Test-Path -LiteralPath $Msi)) { throw "Installer not found: $Msi" }
$setup = Get-Item -LiteralPath $Msi
if (-not (Test-Path -LiteralPath $Key)) { throw "Release key not found: $Key" }

# ---- sign ---------------------------------------------------------------------------------------
$out = Join-Path ([IO.Path]::GetTempPath()) "bmu-pdf-update-$PID"
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $out | Out-Null
$signArgs = @('xtask', 'bmu-update', 'sign', '--key', $Key, '--msi', $setup.FullName, '--out', $out)
if ($NotesFile) { $signArgs += @('--notes-file', $NotesFile) } elseif ($Notes) { $signArgs += @('--notes', $Notes) }
Push-Location $Root
try { & cargo @signArgs; if ($LASTEXITCODE -ne 0) { throw "cargo xtask bmu-update sign failed ($LASTEXITCODE)" } } finally { Pop-Location }
$manifest = Get-Content -Raw -Encoding UTF8 (Join-Path $out 'update.json') | ConvertFrom-Json
Write-Host "Release   : BMU PDF $($manifest.version), $($setup.Name) ($([math]::Round($setup.Length / 1MB, 1)) MB)"
Write-Host "NAS folder: $NasFolder"
if (-not $PSCmdlet.ShouldProcess($NasFolder, "upload $($setup.Name), update.json and update.json.sig")) { return }

# ---- NAS login (BMU Drive's settings) ----------------------------------------------------------
if (-not (Test-Path -LiteralPath $ConfigPath)) { throw "BMU Drive settings not found: $ConfigPath" }
$cfg = Get-Content -Raw -Encoding UTF8 $ConfigPath | ConvertFrom-Json
$nasUrl = $cfg.Synology.BaseUrl.TrimEnd('/')
$account = $cfg.Synology.Username
$passwd = $cfg.Synology.Password
if (-not $account -or -not $passwd) { throw "No NAS login in $ConfigPath" }

$handler = New-Object System.Net.Http.HttpClientHandler
if ($cfg.Synology.IgnoreCertErrors) {
  $handler.ServerCertificateCustomValidationCallback = [System.Net.Http.HttpClientHandler]::DangerousAcceptAnyServerCertificateValidator
}
$http = New-Object System.Net.Http.HttpClient($handler)
$http.Timeout = [TimeSpan]::FromMinutes(30)

# DSM labels some replies `charset="UTF-8"` (quoted), which .NET's GetStringAsync rejects: read bytes.
function Get-Utf8([string] $Url) {
  [Text.Encoding]::UTF8.GetString($http.GetByteArrayAsync($Url).GetAwaiter().GetResult())
}
function Invoke-Syno([string] $Url) {
  $json = (Get-Utf8 $Url) | ConvertFrom-Json
  if (-not $json.success) { throw "Synology API error $($json.error.code) on $(($Url -split '\?')[0])" }
  $json
}

# SYNO.FileStation.Upload with a hand-built multipart body: DSM rejects .NET's
# MultipartFormDataContent (RFC 5987 file names, unquoted field names) with error 101. The same
# shape as BMU Drive's deploy-update.ps1.
function Send-NasFile([string] $LocalPath, [string] $Name) {
  $item = Get-Item -LiteralPath $LocalPath
  Write-Host ("Uploading : {0} ({1:N1} MB)" -f $Name, ($item.Length / 1MB))
  $boundary = 'BmuPdf' + [guid]::NewGuid().ToString('N')
  $mtime = [long]([DateTimeOffset]$item.LastWriteTimeUtc).ToUnixTimeMilliseconds()
  $head = New-Object Text.StringBuilder
  $fields = [ordered]@{
    api = 'SYNO.FileStation.Upload'; version = '2'; method = 'upload'; _sid = $sid
    path = $NasFolder; create_parents = 'true'; overwrite = 'true'; mtime = "$mtime"
  }
  foreach ($f in $fields.GetEnumerator()) {
    [void]$head.Append("--$boundary`r`nContent-Disposition: form-data; name=`"$($f.Key)`"`r`n`r`n$($f.Value)`r`n")
  }
  [void]$head.Append("--$boundary`r`nContent-Disposition: form-data; name=`"file`"; filename=`"$Name`"`r`nContent-Type: application/octet-stream`r`n`r`n")
  $utf8 = New-Object Text.UTF8Encoding($false)
  $prefix = $utf8.GetBytes($head.ToString())
  $suffix = $utf8.GetBytes("`r`n--$boundary--`r`n")
  $bytes = [IO.File]::ReadAllBytes($item.FullName)
  $buffer = New-Object byte[] ($prefix.Length + $bytes.Length + $suffix.Length)
  [Array]::Copy($prefix, 0, $buffer, 0, $prefix.Length)
  [Array]::Copy($bytes, 0, $buffer, $prefix.Length, $bytes.Length)
  [Array]::Copy($suffix, 0, $buffer, $prefix.Length + $bytes.Length, $suffix.Length)
  $body = New-Object System.Net.Http.ByteArrayContent -ArgumentList (, $buffer)
  $body.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::Parse("multipart/form-data; boundary=$boundary")
  try {
    $resp = $http.PostAsync("$nasUrl/webapi/entry.cgi?_sid=$([uri]::EscapeDataString($sid))", $body).GetAwaiter().GetResult()
    $text = [Text.Encoding]::UTF8.GetString($resp.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult())
    if (-not $resp.IsSuccessStatusCode) { throw "HTTP $([int]$resp.StatusCode) uploading $Name" }
    # DSM answers 200 with a JSON envelope even when the upload failed.
    if (-not ($text | ConvertFrom-Json).success) { throw "Upload of $Name failed: $text" }
  } finally { $body.Dispose() }
}

$sid = $null
try {
  $q = "account=$([uri]::EscapeDataString($account))&passwd=$([uri]::EscapeDataString($passwd))"
  $sid = (Invoke-Syno "$nasUrl/webapi/auth.cgi?api=SYNO.API.Auth&version=6&method=login&$q&session=FileStation&format=sid").data.sid
  Write-Host 'Login     : OK'
  Send-NasFile $setup.FullName $setup.Name
  Send-NasFile (Join-Path $out 'update.json') 'update.json'
  Send-NasFile (Join-Path $out 'update.json.sig') 'update.json.sig'
  $live = (Get-Utf8 $CheckUrl) | ConvertFrom-Json
  if ($live.version -ne $manifest.version) { throw "$CheckUrl says $($live.version), expected $($manifest.version)" }
  Write-Host "Verified  : $CheckUrl offers BMU PDF $($live.version)"
} finally {
  if ($sid) {
    try { Invoke-Syno "$nasUrl/webapi/auth.cgi?api=SYNO.API.Auth&version=6&method=logout&session=FileStation&_sid=$sid" | Out-Null } catch { Write-Warning "Logout: $($_.Exception.Message)" }
  }
  $http.Dispose(); $handler.Dispose()
  Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
}
