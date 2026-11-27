param([Parameter(Mandatory)][string]$PastaPortatil, [Parameter(Mandatory)][string]$Evidencias)
$ErrorActionPreference = 'Stop'
$PastaPortatil = (Resolve-Path $PastaPortatil).Path
$Evidencias = (Resolve-Path $Evidencias).Path
$ffmpeg = "$PastaPortatil/ferramentas/ffmpeg.exe"
$trab = Join-Path $env:TEMP ('Divisor vídeos ' + [guid]::NewGuid())
New-Item $trab -ItemType Directory | Out-Null
$resultados = @()
function HashFluxo([string[]]$Arquivos, [string]$Stream) {
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        foreach ($arquivo in $Arquivos) {
            $info = New-Object Diagnostics.ProcessStartInfo
            $info.FileName = $ffmpeg
            $info.Arguments = '-v error -i "' + $arquivo + '" -map ' + $Stream + ' -c copy -f data pipe:1'
            $info.UseShellExecute = $false; $info.CreateNoWindow = $true
            $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
            $proc = [Diagnostics.Process]::Start($info)
            try {
                $erro = $proc.StandardError.ReadToEndAsync()
                $buffer = New-Object byte[] 65536
                while (($n = $proc.StandardOutput.BaseStream.Read($buffer,0,$buffer.Length)) -gt 0) {
                    [void]$hash.TransformBlock($buffer,0,$n,$null,0)
                }
                $proc.WaitForExit()
                if ($proc.ExitCode -ne 0) { throw $erro.Result }
            } finally { $proc.Dispose() }
        }
        [void]$hash.TransformFinalBlock([byte[]]@(),0,0)
        ([BitConverter]::ToString($hash.Hash)).Replace('-','')
    } finally { $hash.Dispose() }
}
try {
    # Raw RGB + PCM preserva tamanho previsível e permite comparação byte a byte dos fluxos.
    foreach ($caso in @(@{nome='40-70MB';frames=80;min=40000000;max=70000000}, @{nome='maior-1GB';frames=1600;min=1000000001;max=2000000000})) {
        $video = "$trab/vídeo $($caso.nome).mov"
        & $ffmpeg -v error -f lavfi -i 'testsrc=size=640x360:rate=25' -f lavfi -i 'sine=frequency=440:sample_rate=48000' -t ($caso.frames / 25).ToString([Globalization.CultureInfo]::InvariantCulture) -c:v rawvideo -pix_fmt rgb24 -threads:v 1 -c:a pcm_s16le $video
        if ($LASTEXITCODE -ne 0) { throw 'Falha ao gerar fixture grande' }
        $bytes = (Get-Item $video).Length
        if ($bytes -lt $caso.min -or $bytes -gt $caso.max) { throw "Fixture fora da faixa: $bytes" }
        $original = (Get-FileHash $video -Algorithm SHA256).Hash
        $saida = "$trab/saída $($caso.nome)"
        & "$PastaPortatil/divisor_video.exe" $video -o $saida -m 10000000
        if ($LASTEXITCODE -ne 0) { throw 'Divisão falhou' }
        $partes = @(Get-ChildItem $saida -Recurse -Filter '*.mov' | Sort-Object FullName)
        if ($partes.Count -lt 2 -or @($partes | Where-Object Length -gt 10000000).Count) { throw 'Quantidade/tamanho de segmentos inválido' }
        foreach ($stream in @('0:v:0','0:a:0')) {
            if ((HashFluxo @($video) $stream) -ne (HashFluxo @($partes.FullName) $stream)) { throw "Fluxo alterado: $stream" }
        }
        if ((Get-FileHash $video -Algorithm SHA256).Hash -ne $original) { throw 'Original alterado' }
        $resultados += @{caso=$caso.nome; bytes=$bytes; segmentos=$partes.Count; limite=10000000; payload_video_audio='identico'; resultado='aprovado'}
        Remove-Item $video, $saida -Recurse -Force
    }
} finally {
    ConvertTo-Json -InputObject @($resultados) -Depth 5 | Set-Content "$Evidencias/arquivos-grandes.json" -Encoding utf8
    Remove-Item $trab -Recurse -Force -ErrorAction SilentlyContinue
}
