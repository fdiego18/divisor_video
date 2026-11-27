# Dependência EXCLUSIVA do mantenedor: libx264 para a suíte preexistente; nunca empacotada.
param([Parameter(Mandatory)][string]$Destino)
$ErrorActionPreference = 'Stop'
$cfg = (Get-Content "$PSScriptRoot\dependencias.json" -Raw | ConvertFrom-Json).test_only_ffmpeg
New-Item $Destino -ItemType Directory -Force | Out-Null
$zip = Join-Path $Destino 'ffmpeg-testes.zip'
Invoke-WebRequest $cfg.url -OutFile $zip
$checksum = (Invoke-WebRequest $cfg.checksum_url).Content.Trim()
if ($checksum -notmatch '^([a-fA-F0-9]{64})(\s|$)') { throw 'Checksum do fornecedor inválido' }
$esperado = $Matches[1]
if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $esperado) { throw 'Checksum de FFmpeg de teste divergente' }
Expand-Archive $zip -DestinationPath $Destino -Force
$bin = @(Get-ChildItem $Destino -Filter ffmpeg.exe -Recurse)
if ($bin.Count -ne 1) { throw 'Arquivo de teste ambíguo' }
$version = & $bin[0].FullName -version
if ($LASTEXITCODE -ne 0 -or $version[0] -notmatch ('ffmpeg version ' + [regex]::Escape($cfg.version))) { throw 'Versão de teste incorreta' }
@{ url=$cfg.url; sha256=$esperado; version=$version; distributed=$false } | ConvertTo-Json -Depth 5 | Set-Content "$Destino\PROVENIENCIA-TESTES.json" -Encoding utf8
$bin[0].Directory.FullName
