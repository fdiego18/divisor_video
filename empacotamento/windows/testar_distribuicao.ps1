# Executar em conta/VM descartável. Nunca usar configurações pessoais para a CI.
param([Parameter(Mandatory)][string]$Dist)
$ErrorActionPreference = 'Stop'
$Dist = (Resolve-Path $Dist).Path
$base = Join-Path $env:TEMP ('DivisorVideo distribuição ' + [guid]::NewGuid())
New-Item $base -ItemType Directory | Out-Null
$oldPath=$env:PATH; $oldLocal=$env:LOCALAPPDATA; $oldApp=$env:APPDATA
$resultados = [Collections.Generic.List[object]]::new()
function Teste([string]$Nome, [scriptblock]$Acao) {
    try { & $Acao; $resultados.Add(@{nome=$Nome; resultado='aprovado'}) }
    catch { $resultados.Add(@{nome=$Nome; resultado='falhou'; erro=$_.Exception.Message}); throw }
}
function IniciarGui([string]$Exe, [string]$Nome) {
    $Exe = (Resolve-Path $Exe).Path
    $p = Start-Process -FilePath $Exe -PassThru
    try {
        Start-Sleep -Seconds 8
        $p.Refresh()
        if ($p.HasExited) { throw "GUI encerrou: $($p.ExitCode)" }
        if ($p.MainWindowHandle -eq 0) { throw "GUI sem janela principal" }
        $modules = @($p.Modules | Select-Object ModuleName, FileName)
        $modules = @($modules | ForEach-Object {
            $signature = Get-AuthenticodeSignature -FilePath $_.FileName
            $signer = $null
            if ($signature.SignerCertificate) { $signer = $signature.SignerCertificate.Subject }
            [pscustomobject]@{ ModuleName=$_.ModuleName; FileName=$_.FileName; Sha256=(Get-FileHash $_.FileName -Algorithm SHA256).Hash; SignatureStatus=$signature.Status.ToString(); Signer=$signer }
        })
        $modules | ConvertTo-Json | Set-Content "$Dist/modulos-$Nome.json" -Encoding utf8
        $system = [IO.Path]::GetFullPath($env:SystemRoot).TrimEnd('\') + '\'
        foreach ($m in $modules) {
            if ($m.FileName -eq $Exe) { continue }
            if (!$m.FileName.StartsWith($system,[StringComparison]::OrdinalIgnoreCase)) { throw "Módulo externo não fornecido pelo Windows: $($m.FileName)" }
            if ($m.SignatureStatus -ne 'Valid') { throw "Assinatura de módulo inválida/indeterminada: $($m.FileName)" }
            if ($m.ModuleName -match '^(vcruntime|msvcp|concrt)') { throw "Runtime VC++ dinâmico: $($m.ModuleName)" }
        }
        # Evidência de inicialização apenas; não verifica interação nem renderização correta.
    } finally {
        if (!$p.HasExited) { [void]$p.CloseMainWindow(); if (!$p.WaitForExit(10000)) { $p.Kill(); $p.WaitForExit() } }
        $p.Dispose()
    }
}
try {
    $env:PATH="$env:SystemRoot\System32;$env:SystemRoot"
    $env:LOCALAPPDATA="$base/local"; $env:APPDATA="$base/roaming"
    New-Item $env:LOCALAPPDATA,$env:APPDATA -ItemType Directory -Force | Out-Null
    $portable="$base/Portátil com espaços"
    Teste 'extrair-portatil' { Expand-Archive "$Dist/DivisorVideo-Windows-x64-Portable.zip" $portable }
    Teste 'CLI-portatil-e-integridade-audiovisual' { & "$PSScriptRoot/testar_portatil.ps1" -PastaPortatil $portable }
    Teste '40-70MB-e-maior-1GB' { & "$PSScriptRoot/testar_arquivos_grandes.ps1" -PastaPortatil $portable -Evidencias $Dist }
    Teste 'inicio-GUI-portatil-e-modulos' { IniciarGui "$portable/divisor_video_gui.exe" 'portatil' }
    Teste 'ferramenta-adulterada-bloqueada' {
        $tool="$portable/ferramentas/ffmpeg.exe"
        $bytes=[IO.File]::ReadAllBytes($tool); $bytes[0]=0; [IO.File]::WriteAllBytes($tool,$bytes)
        $fixture="$base/invalido.mp4"; [IO.File]::WriteAllText($fixture,'fixture')
        & "$portable/divisor_video.exe" $fixture -o "$base/saida"
        if ($LASTEXITCODE -ne 3) { throw 'Falha não foi detectada como ambiente inválido' }
    }
    Teste 'inicio-exe-unico' { IniciarGui "$Dist/DivisorVideo-Windows-x64.exe" 'unico' }
    # dirs usa KnownFolder no Windows; não presumir que alterar LOCALAPPDATA redireciona KnownFolder.
    $cache = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'DivisorVideo/ferramentas'
    Teste 'cache-extraido-hashes-e-recuperacao' {
        $expected = Get-Content "$Dist/DivisorVideo-Windows-x64-Portable/licencas/DEPENDENCIAS.json" -Raw | ConvertFrom-Json
        foreach ($name in @('ffmpeg.exe','ffprobe.exe')) {
            $hash=$expected.binaries.$name.sha256
            $tool=Join-Path $cache "$hash/$name"
            if ((Get-FileHash $tool -Algorithm SHA256).Hash -ne $hash) { throw 'Extração inválida' }
            $before=(Get-Item $tool).LastWriteTimeUtc
            IniciarGui "$Dist/DivisorVideo-Windows-x64.exe" "reuso-$name"
            if ((Get-Item $tool).LastWriteTimeUtc -ne $before) { throw 'Cache íntegro foi regravado' }
            [IO.File]::WriteAllText($tool,'corrompido')
            IniciarGui "$Dist/DivisorVideo-Windows-x64.exe" "reparo-$name"
            if ((Get-FileHash $tool -Algorithm SHA256).Hash -ne $hash) { throw 'Cache não recuperado' }
        }
    }
    Teste 'instalacao-inicio-desinstalacao' {
        $install="$base/Instalado"
        $p=Start-Process "$Dist/DivisorVideo-Windows-x64-Setup.exe" -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-',('/DIR="'+$install+'"')) -Wait -PassThru
        if ($p.ExitCode -ne 0) { throw "Instalador: $($p.ExitCode)" }
        IniciarGui "$install/divisor_video_gui.exe" 'instalado'
        $p=Start-Process "$install/unins000.exe" -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Wait -PassThru
        if ($p.ExitCode -ne 0 -or (Test-Path "$install/divisor_video_gui.exe")) { throw 'Desinstalação falhou' }
    }
} finally {
    $env:PATH=$oldPath; $env:LOCALAPPDATA=$oldLocal; $env:APPDATA=$oldApp
    ConvertTo-Json -InputObject @($resultados.ToArray()) -Depth 6 | Set-Content "$Dist/testes-automatizados.json" -Encoding utf8
    & "$PSScriptRoot/gerar_relatorio.ps1" -Dist $Dist
    Remove-Item $base -Recurse -Force -ErrorAction SilentlyContinue
}
