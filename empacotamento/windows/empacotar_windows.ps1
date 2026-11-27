# Executar em Developer PowerShell x64 (MSVC, Windows SDK, Rust e Inno Setup 6).
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PastaFfmpegBin,
    [Parameter(Mandatory)][string]$Sha256Ffmpeg,
    [Parameter(Mandatory)][string]$Sha256Ffprobe,
    [Parameter(Mandatory)][string]$PastaLicencas,
    [Parameter(Mandatory)][string]$FontesCorrespondentesZip,
    [Parameter(Mandatory)][string]$PastaFfmpegTeste,
    [string]$Iscc = 'ISCC.exe'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Execute em Windows x64.' }
$Raiz = (Resolve-Path "$PSScriptRoot\..\..").Path
$Dist = Join-Path $Raiz 'dist\windows-x64'
$Stage = Join-Path $Dist 'DivisorVideo-Windows-x64-Portable'
$env:DIVISOR_TOOLS_DIR = (Resolve-Path $PastaFfmpegBin).Path
$env:DIVISOR_FFMPEG_SHA256 = $Sha256Ffmpeg
$env:DIVISOR_FFPROBE_SHA256 = $Sha256Ffprobe
# Falhar antes de gerar qualquer artefato incompleto.
Get-Command $Iscc -ErrorAction Stop | Out-Null
Get-Command dumpbin.exe -ErrorAction Stop | Out-Null
New-Item $Dist -ItemType Directory -Force | Out-Null
$VersaoFfmpeg = (Get-Content "$PSScriptRoot\dependencias.json" -Raw | ConvertFrom-Json).ffmpeg.version
foreach ($item in @(@('ffmpeg', $Sha256Ffmpeg), @('ffprobe', $Sha256Ffprobe))) {
    $exe = Join-Path $env:DIVISOR_TOOLS_DIR ($item[0] + '.exe')
    if ($item[1] -notmatch '^[0-9a-fA-F]{64}$' -or (Get-FileHash $exe -Algorithm SHA256).Hash -ne $item[1]) { throw "SHA-256 incorreto: $exe" }
    $version = & $exe -version 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Ferramenta não executável: $exe" }
    if ($version[0] -notmatch ('^' + $item[0] + ' version ' + [regex]::Escape($VersaoFfmpeg) + '(\s|$)')) { throw "Versão inesperada: $exe" }
    $version | Set-Content (Join-Path $Dist ('versao-' + $item[0] + '.txt')) -Encoding utf8
    if (($version -join "`n") -match '--enable-(nonfree|shared|gpl|version3)') { throw 'Build fora do perfil LGPL estático.' }
    if (($version -join "`n") -notmatch '--disable-network') { throw 'Ferramenta distribuída deve ter rede desabilitada.' }
    & "$PSScriptRoot\validar_dlls.ps1" -Executavel $exe -Relatorio (Join-Path $Dist ('dlls-' + $item[0] + '.txt'))
}
foreach ($file in @('FFmpeg-LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'BUILD-INFO.txt')) {
    if (!(Test-Path (Join-Path $PastaLicencas $file))) { throw "Documento obrigatório: $file" }
}
if (!(Test-Path $FontesCorrespondentesZip -PathType Leaf)) { throw 'Forneça fontes correspondentes e scripts de compilação.' }
if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
New-Item "$Stage\ferramentas", "$Stage\licencas" -ItemType Directory -Force | Out-Null
Copy-Item "$PastaLicencas\*" "$Stage\licencas" -Recurse
Copy-Item $FontesCorrespondentesZip "$Stage\licencas\fontes-correspondentes.zip"
Copy-Item "$env:DIVISOR_TOOLS_DIR\ffmpeg.exe", "$env:DIVISOR_TOOLS_DIR\ffprobe.exe" "$Stage\ferramentas"
$PathAnterior = $env:PATH
$env:PATH = "$PastaFfmpegTeste;$env:PATH" # Somente o processo de build/teste.
Push-Location $Raiz
try {
    cargo fmt --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatação inválida' }
    cargo clippy --locked --offline --all-targets --target x86_64-pc-windows-msvc -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy falhou' }
    cargo test --locked --offline --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Testes falharam' }
    cargo clippy --locked --offline --all-targets --target x86_64-pc-windows-msvc --features embedded-tools -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy do pacote falhou' }
    cargo build --release --locked --offline --target x86_64-pc-windows-msvc --features windows-bundle --bins
    if ($LASTEXITCODE -ne 0) { throw 'Compilação falhou' }
    $Bin = Join-Path $Raiz 'target\x86_64-pc-windows-msvc\release'
    foreach ($name in @('divisor_video.exe', 'divisor_video_gui.exe')) {
        & "$PSScriptRoot\validar_dlls.ps1" -Executavel "$Bin\$name" -Relatorio "$Dist\dlls-$name.txt" -Gui:($name -eq 'divisor_video_gui.exe')
        Copy-Item "$Bin\$name" $Stage
    }
    cargo test --locked --offline --target x86_64-pc-windows-msvc --features embedded-tools --lib
    if ($LASTEXITCODE -ne 0) { throw 'Testes das ferramentas incorporadas falharam' }
    cargo build --release --locked --offline --target x86_64-pc-windows-msvc --features embedded-tools --bin divisor_video_gui
    if ($LASTEXITCODE -ne 0) { throw 'Compilação incorporada falhou' }
    & "$PSScriptRoot\validar_dlls.ps1" -Executavel "$Bin\divisor_video_gui.exe" -Relatorio "$Dist\dlls-unico.txt" -Gui
    Copy-Item "$Bin\divisor_video_gui.exe" "$Dist\DivisorVideo-Windows-x64.exe"
} finally { Pop-Location; $env:PATH = $PathAnterior }
# O executável único deve ser redistribuído com este pacote de licenças e fontes.
Compress-Archive -Path "$Stage\licencas\*" -DestinationPath "$Dist\DivisorVideo-Windows-x64-Licencas.zip" -Force
$Manifesto = @(Get-ChildItem $Stage -Recurse -File | Where-Object Name -ne 'SHA256SUMS.txt' | Sort-Object FullName | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.FullName.Substring($Stage.Length + 1)
})
$Manifesto | Set-Content "$Stage\SHA256SUMS.txt" -Encoding ASCII
Compress-Archive -Path "$Stage\*" -DestinationPath "$Dist\DivisorVideo-Windows-x64-Portable.zip" -Force
& $Iscc "$PSScriptRoot\instalador.iss"
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup falhou' }
$Artefatos = @('DivisorVideo-Windows-x64-Portable.zip', 'DivisorVideo-Windows-x64-Setup.exe', 'DivisorVideo-Windows-x64.exe', 'DivisorVideo-Windows-x64-Licencas.zip')
$Artefatos | ForEach-Object { '{0}  {1}' -f (Get-FileHash (Join-Path $Dist $_) -Algorithm SHA256).Hash.ToLower(), $_ } | Set-Content "$Dist\SHA256SUMS.txt" -Encoding ASCII
& "$PSScriptRoot\gerar_relatorio.ps1" -Dist $Dist
Write-Host 'Artefatos gerados. Validação em instalações limpas Windows 10/11 ainda obrigatória.'
