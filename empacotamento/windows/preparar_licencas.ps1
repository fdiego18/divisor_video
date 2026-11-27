param([Parameter(Mandatory)][string]$PastaDependencias)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Raiz = (Resolve-Path "$PSScriptRoot\..\..").Path
$Destino = (Resolve-Path $PastaDependencias).Path
$Licencas = Join-Path $Destino 'licencas'
Copy-Item "$Raiz\LICENSE" "$Licencas\DivisorVideo-MIT.txt"
$Fontes = Join-Path $Destino 'fontes-aplicativo'
New-Item $Fontes -ItemType Directory -Force | Out-Null
Push-Location $Raiz
try {
    # Inclui os fontes completos (e seus avisos originais) em vez de inferir textos SPDX.
    cargo vendor --locked "$Fontes\vendor" | Set-Content "$Fontes\vendor-config.toml" -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw 'Falha ao obter fontes Cargo' }
    $metadata = cargo metadata --locked --offline --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Falha no inventário Cargo' }
    $metadata | Set-Content "$Licencas\cargo-metadata.json" -Encoding utf8
    $packages = ($metadata | ConvertFrom-Json).packages | Select-Object name, version, license, license_file, source
    $packages | ConvertTo-Json -Depth 5 | Set-Content "$Licencas\DEPENDENCIAS-RUST.json" -Encoding utf8
    foreach ($item in @('Cargo.toml','Cargo.lock','build.rs','LICENSE','src','tests','empacotamento','.cargo','README.md')) {
        Copy-Item (Join-Path $Raiz $item) $Fontes -Recurse -Force
    }
    $sysroot = rustc --print sysroot
    if ($LASTEXITCODE -ne 0) { throw 'Rust sysroot indisponível' }
    New-Item "$Licencas\rust-runtime" -ItemType Directory -Force | Out-Null
    Copy-Item "$sysroot\share\doc\rust\COPYRIGHT*.html", "$sysroot\share\doc\rust\licenses" "$Licencas\rust-runtime" -Recurse
    Copy-Item "$sysroot\lib\rustlib\src\rust\library" "$Fontes\rust-standard-library" -Recurse
    # tar não possui o limite de arquivo individual de 2 GB do Compress-Archive.
    tar -czf "$Licencas\fontes-rust-aplicativo.tar.gz" -C $Destino fontes-aplicativo
    if ($LASTEXITCODE -ne 0) { throw 'Falha ao arquivar fontes Rust' }
    Add-Content "$Licencas\THIRD-PARTY-NOTICES.txt" "`nCrates: DEPENDENCIAS-RUST.json. Textos originais de licença e copyright: fontes-rust-aplicativo.tar.gz, vendor/. SQLite: vendor/libsqlite3-sys, compilado com bundled. Biblioteca padrão Rust: rust-runtime/ e fontes. Código divisor_video: MIT conforme Cargo.toml; autoria registrada no manifesto."
} finally { Pop-Location }
