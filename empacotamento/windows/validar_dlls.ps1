param([Parameter(Mandatory)][string]$Executavel, [string]$Relatorio = "", [switch]$Gui)
$ErrorActionPreference = 'Stop'
$headers = & dumpbin.exe /headers $Executavel 2>&1
if ($LASTEXITCODE -ne 0 -or ($headers -join "`n") -notmatch '8664 machine') { throw "PE não é x64: $Executavel" }
$deps = & dumpbin.exe /dependents $Executavel 2>&1
if ($LASTEXITCODE -ne 0) { throw "Falha ao inspecionar $Executavel" }
if ($Gui -and ($headers -join "`n") -notmatch '2 subsystem \(Windows GUI\)') { throw "Subsistema GUI ausente: $Executavel" }
if ($Relatorio) { @($headers; $deps) | Set-Content $Relatorio -Encoding utf8 }
# Lista conservadora: DLL desconhecida bloqueia o pacote, mesmo instalada no host de build.
$Sistema = @('kernel32.dll','user32.dll','gdi32.dll','advapi32.dll','shell32.dll','ole32.dll','oleaut32.dll','comdlg32.dll','comctl32.dll','ws2_32.dll','bcrypt.dll','ntdll.dll','secur32.dll','crypt32.dll','winmm.dll','version.dll','shlwapi.dll','imm32.dll','setupapi.dll','cfgmgr32.dll','dwmapi.dll','uxtheme.dll','opengl32.dll','glu32.dll','d3d11.dll','d3d12.dll','dxgi.dll','dwrite.dll','d2d1.dll','windowscodecs.dll','propsys.dll','powrprof.dll','iphlpapi.dll','userenv.dll','netapi32.dll','dnsapi.dll','psapi.dll','msvcrt.dll','ucrtbase.dll','wintrust.dll','normaliz.dll','avrt.dll','mf.dll','mfplat.dll','mfreadwrite.dll','mfuuid.dll','strmiids.dll','ksuser.dll','winspool.drv','rpcrt4.dll','ncrypt.dll','bcryptprimitives.dll','d3dcompiler_47.dll','dxcore.dll','dbghelp.dll')
foreach ($line in $deps) {
    if ($line -match '^\s+([\w.-]+\.(dll|drv))\s*$') {
        $dll = $Matches[1].ToLower()
        if ($dll -notin $Sistema -and $dll -notmatch '^(api-ms-win-|ext-ms-win-)') { throw "Dependência não aprovada: $dll em $Executavel" }
    }
}
Write-Host "Importações PE aprovadas (não substitui teste real): $Executavel"
