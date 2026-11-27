param([Parameter(Mandatory)][string]$Dist)
$ErrorActionPreference = 'Stop'
$Dist = (Resolve-Path $Dist).Path
if (!(Test-Path "$Dist/MATRIZ-VALIDACAO.json")) { Copy-Item "$PSScriptRoot/matriz_validacao.json" "$Dist/MATRIZ-VALIDACAO.json" }
$nomes = @('DivisorVideo-Windows-x64.exe','DivisorVideo-Windows-x64-Portable.zip','DivisorVideo-Windows-x64-Setup.exe')
$artefatos = foreach ($nome in $nomes) {
    $path = Join-Path $Dist $nome
    if (Test-Path $path) {
        @{ formato=$nome; geracao='gerado'; bytes=(Get-Item $path).Length; sha256=(Get-FileHash $path -Algorithm SHA256).Hash.ToLower(); validacao_grafica='pendente' }
    } else { @{ formato=$nome; geracao='ausente'; bytes=$null; sha256=$null; validacao_grafica='pendente' } }
}
$relatorio = @{
    data_utc=[DateTime]::UtcNow.ToString('o'); plataforma=[Environment]::OSVersion.VersionString
    commit=$env:GITHUB_SHA; run_url="https://github.com/$env:GITHUB_REPOSITORY/actions/runs/$env:GITHUB_RUN_ID"
    artefatos=@($artefatos); distribuicao_finalizada=$false
    validacao_windows10='nao_executada'; validacao_windows11='nao_executada'
    dependencias_externas=@('DLLs nativas Windows 10 1809+/11 x64','Driver gráfico compatível','Reprodutor associado apenas para prévia externa opcional')
    limitacoes=@('Runner Windows Server com ferramentas de desenvolvimento não é instalação limpa Windows 10/11','Assinatura Authenticode não aplicada','Testes gráficos reais e revisão final de licenças pendentes')
}
if (Test-Path "$Dist/testes-automatizados.json") { $relatorio.testes_automatizados = Get-Content "$Dist/testes-automatizados.json" -Raw | ConvertFrom-Json }
$relatorio | ConvertTo-Json -Depth 12 | Set-Content "$Dist/RELATORIO.json" -Encoding utf8
$linhas = @('# Relatório Windows x64', '', 'Distribuição NÃO finalizada. Build automatizado não certifica uso gráfico em Windows 10/11 limpos.', '', '| Artefato | Resultado | Bytes | SHA-256 |', '| --- | --- | ---: | --- |')
foreach ($a in $artefatos) { $linhas += "| $($a.formato) | $($a.geracao) | $($a.bytes) | $($a.sha256) |" }
$linhas += @('', 'Evidências: compilacao-windows.log, testes-automatizados.json, dlls-*.txt e modulos-*.json quando presentes.', 'Versões/fontes/licenças: pasta licencas do portátil, DEPENDENCIAS.json e DEPENDENCIAS-RUST.json.', 'Somente resultados explicitamente registrados como aprovados foram executados. Windows 10, Windows 11, temas, diálogos, arrastar/soltar e uso sem administrador permanecem pendentes.')
$linhas | Set-Content "$Dist/RELATORIO.md" -Encoding utf8
