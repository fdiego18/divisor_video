# Executar como usuário comum em Windows 10/11 limpo, rede desligada.
param([Parameter(Mandatory)][string]$PastaPortatil)
$ErrorActionPreference = 'Stop'
$Pasta = (Resolve-Path $PastaPortatil).Path
$PathAnterior = $env:PATH
$Temporario = Join-Path $env:TEMP ('DivisorVideo-teste-' + [guid]::NewGuid())
New-Item $Temporario -ItemType Directory | Out-Null
function Executar([string]$Programa, [string[]]$Argumentos) {
    & $Programa @Argumentos
    if ($LASTEXITCODE -ne 0) { throw "Falha: $Programa $Argumentos" }
}
try {
    $env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    foreach ($linha in Get-Content "$Pasta\SHA256SUMS.txt") {
        if ($linha -notmatch '^([a-fA-F0-9]{64})  (.+)$') { throw 'Manifesto inválido' }
        if ((Get-FileHash (Join-Path $Pasta $Matches[2]) -Algorithm SHA256).Hash -ne $Matches[1]) { throw "Hash inválido: $linha" }
    }
    $ffmpeg = "$Pasta\ferramentas\ffmpeg.exe"
    $ffprobe = "$Pasta\ferramentas\ffprobe.exe"
    $video = Join-Path $Temporario 'vídeo com espaços.mp4'
    Executar $ffmpeg @('-v','error','-f','lavfi','-i','testsrc2=size=640x360:rate=25:duration=20','-f','lavfi','-i','sine=frequency=440:duration=20','-c:v','mpeg4','-q:v','2','-g','25','-c:a','aac',$video)
    $original = (Get-FileHash $video -Algorithm SHA256).Hash
    Executar "$Pasta\divisor_video.exe" @($video,'-o',"$Temporario\saida",'-m','2000000')
    if ((Get-FileHash $video -Algorithm SHA256).Hash -ne $original) { throw 'Original foi alterado' }
    $partes = @(Get-ChildItem "$Temporario\saida" -Recurse -Filter '*.mp4' | Sort-Object FullName)
    if ($partes.Count -lt 2) { throw 'Vídeo não foi dividido' }
    foreach ($parte in $partes) {
        if ($parte.Length -gt 2000000) { throw 'Parte excede limite' }
        Executar $ffmpeg @('-v','error','-i',$parte.FullName,'-f','null','-')
    }
    # Compara payload comprimido de cada stream, ignorando apenas timestamps do container.
    function Pacotes([string]$Arquivo) {
        $json = & $ffprobe -v error -show_packets -show_data_hash sha256 -show_entries packet=stream_index,data_hash -of json $Arquivo
        if ($LASTEXITCODE -ne 0) { throw 'Falha na inspeção de pacotes' }
        ($json -join "`n" | ConvertFrom-Json).packets
    }
    $antes = @(Pacotes $video)
    $depois = @($partes | ForEach-Object { Pacotes $_.FullName })
    foreach ($stream in @($antes.stream_index | Sort-Object -Unique)) {
        $a = @($antes | Where-Object stream_index -eq $stream | ForEach-Object data_hash) -join "`n"
        $b = @($depois | Where-Object stream_index -eq $stream | ForEach-Object data_hash) -join "`n"
        if ($a -ne $b) { throw "Payload audiovisual divergente: stream $stream" }
    }
    Write-Host "Smoke CLI aprovado. Evidências e vídeo: $Temporario"
    Write-Host 'Interface, cancelamento, DLLs carregadas dinamicamente e instalador ainda exigem teste manual.'
} finally { $env:PATH = $PathAnterior }
