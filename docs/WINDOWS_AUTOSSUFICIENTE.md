# Distribuição Windows x64 autossuficiente

**Estado em 27/11/2026: primeira execução analisada; correções preparadas; distribuição NÃO finalizada.** A execução `45997666880` falhou antes do Cargo. A assinatura FFmpeg foi validada, mas um caminho relativo interrompeu o script antes da compilação; o status verde do job ocultou essa falha por ausência de `pipefail` no shell externo. Consulte [causas, correções e regressão](CORRECAO_PIPELINE_WINDOWS.md). A nova execução MSVC e a validação gráfica Windows continuam pendentes.

## Executar a pipeline

O workflow [windows-x64.yml](../.github/workflows/windows-x64.yml) é acionado em GitHub Actions por **Run workflow** ou por tag `windows-v*`. Ele não publica uma release: disponibiliza o artefato `DivisorVideo-Windows-x64-nao-certificado` por 30 dias. É preciso incluir estes arquivos em um repositório GitHub com Actions habilitado e executar o workflow. Não há necessidade de segredos para o build.

1. **Ubuntu 24.04:** obtém fonte estável oficial FFmpeg 9.1, autentica a assinatura com fingerprint fixo e compila FFmpeg/FFprobe para Windows x64 com MinGW. Inclui fonte exato, objetos para relink, scripts, configuração, licenças e fontes do runtime MinGW. Gera SHA-256 dos fontes e binários e `DEPENDENCIAS.json`.
2. **Windows Server 2022:** extrai a entrega TAR e confere arquivos obrigatórios, tamanhos e SHA-256 antes de instalar toolchain Rust 1.97.1, ativar MSVC x64 e obter dependências de build e fontes Cargo. Python 3.11+ e tar, presentes nos runners, servem apenas à validação/transporte no build; não são distribuídos nem necessários no computador do usuário. Usa o empacotador existente, com CRT estático e SQLite bundled, para gerar os três formatos. Executa fmt, Clippy, testes existentes e testes da incorporação. Audita PE/subsistema GUI. A compilação Cargo usa `--locked --offline` após preencher o cache.
3. **Testes automatizados dos pacotes:** extrai ZIP, restringe PATH, testa CLI e integridade audiovisual, arquivos de 40–70 MB e maiores que 1 GB, inicia as três GUIs, registra módulos carregados, testa cache, adulteração de ferramenta e instalação/desinstalação. Resultados individuais são registrados; qualquer falha encerra a etapa. Artifacts são enviados mesmo em falha para diagnóstico, **não para publicação automática**.
4. **Aceitação gráfica real:** executar a matriz abaixo em VMs Windows 10 e 11 x64 limpas. Um runner Server com ferramentas instaladas e direitos administrativos **não** demonstra funcionamento em instalação limpa, offline ou sem privilégios.

A infraestrutura de Actions precisa de rede para obter fontes e ferramentas de compilação. Os pacotes de execução não fazem downloads. Os testes do runner restringem PATH, mas **não desligam sua conexão**; isso permanece na matriz manual.

## Dependências e procedência

O manifesto versionado é [dependencias.json](../empacotamento/windows/dependencias.json). Não há hashes fictícios: o hash do fonte local está `null` até sua obtenção; o build aceita somente um fonte com assinatura oficial válida e registra o SHA-256 calculado. Para fixar também o digest nas próximas execuções, preencher `source_sha256` com o valor de uma primeira execução autenticada revisada. Os hashes dos EXEs são transferidos ao empacotador e novamente exigidos por `build.rs` antes de incorporar bytes.

| Componente | Seleção | Situação nesta sessão |
| --- | --- | --- |
| FFmpeg / FFprobe de distribuição | 9.1 estável, fonte oficial assinado, perfil LGPL-2.1-or-later | Aquisição/compilação pela pipeline ainda pendentes |
| Chave de release FFmpeg | `FCF986EA15E6E293A5644F10B4322F04D67658D8` | Conferida na página oficial |
| FFmpeg para testes preexistentes | Gyan 9.0.2 essentials, com libx264, hash publicado pelo fornecedor | Somente no runner; nunca redistribuído |
| Rust do build Windows | 1.97.1, MSVC x64 | Fixado no workflow |
| Crates Rust / SQLite | Versões em Cargo.lock; rusqlite 0.37 com bundled | Inventário completo gerado via Cargo metadata |
| Runtime C de FFmpeg | MinGW GCC win32, link estático; CRT nativo Windows | Versões reais e avisos registrados no build |
| Runtime C do aplicativo | MSVC `+crt-static` | Configuração existente preservada; auditar binários gerados |

A divisão usa cópia de fluxos e as miniaturas usam decoder nativo + MJPEG; não precisam de libx264/libx265. Por isso o perfil distribuído desativa GPL, nonfree, bibliotecas externas opcionais, aceleração de hardware e rede. Mantém demuxers, muxers, parsers e decoders nativos, mais os encoders necessários a miniaturas/fixtures. Formatos que exigem biblioteca externa não habilitada devem ser registrados como limitação, não presumidos compatíveis. O FFmpeg GPL de testes serve exclusivamente para gerar os vídeos da suíte antiga, que permanece intacta.

Fontes primárias consultadas:

- [Releases e chave oficial FFmpeg](https://ffmpeg.org/download.html): release 9.1 e instruções de autenticação; também indica Gyan/BtbN.
- [Compilação Windows documentada pelo FFmpeg](https://ffmpeg.org/platform.html).
- [Licença e obrigações FFmpeg](https://ffmpeg.org/legal.html).
- [Builds Gyan](https://www.gyan.dev/ffmpeg/builds/): GPL, usados apenas pelo mantenedor nos testes.
- [Runners GitHub](https://docs.github.com/en/actions/reference/runners/github-hosted-runners): o runner x64 selecionado é Windows Server, não Windows 10/11 limpo.

## Saídas e evidências

Tudo destinado à entrega fica em `dist/windows-x64/`:

| Arquivo | Conteúdo |
| --- | --- |
| `DivisorVideo-Windows-x64.exe` | GUI com FFmpeg/FFprobe via `include_bytes!` |
| `DivisorVideo-Windows-x64-Portable.zip` | GUI, CLI, ferramentas, licenças e fontes; extrair e abrir GUI |
| `DivisorVideo-Windows-x64-Setup.exe` | Inno Setup, instalação por usuário em LocalAppData, `PrivilegesRequired=lowest` |
| `DivisorVideo-Windows-x64-Licencas.zip` | Acompanha obrigatoriamente o executável único |
| `SHA256SUMS.txt` | Checksums dos três formatos e pacote de licenças |
| `RELATORIO.md` / `RELATORIO.json` | Bytes/hash/resultado por formato; aprovação gráfica sempre separada |
| `compilacao-windows.log`, `dlls-*.txt`, `modulos-*.json` | Evidência de build, imports PE e módulos observados na inicialização |
| `testes-automatizados.json`, `arquivos-grandes.json` | Somente resultados realmente executados |
| `MATRIZ-VALIDACAO.json` | Lista dos testes reais pendentes, por SO e formato |

Sem artefato, tamanho e SHA-256 são `null`. O relatório local atual registra essa ausência. Os checksums não equivalem a assinatura Authenticode, que não foi aplicada. Se assinar binários, regenerar pacotes/checksums na ordem correta; não alterar FFmpeg após fixar seus hashes.

## Compilação local no Windows pelo mantenedor

Usar os insumos `ffmpeg-lgpl-windows-x64` produzidos pela etapa Ubuntu, Developer PowerShell x64 com MSVC/SDK, Rust 1.97.1 e Inno Setup 6. Preparar antes cache Cargo, fontes/licenças e FFmpeg de teste; nada disso é exigido do usuário final.

```powershell
$deps = (Resolve-Path build/windows-deps).Path
$hashes = Get-Content "$deps/licencas/DEPENDENCIAS.json" -Raw | ConvertFrom-Json
cargo fetch --locked --target x86_64-pc-windows-msvc
.\empacotamento\windows\preparar_licencas.ps1 -PastaDependencias $deps
$teste = .\empacotamento\windows\obter_ffmpeg_testes.ps1 -Destino "$env:TEMP\ffmpeg-testes"
.\empacotamento\windows\empacotar_windows.ps1 `
  -PastaFfmpegBin "$deps/bin" `
  -Sha256Ffmpeg $hashes.binaries.'ffmpeg.exe'.sha256 `
  -Sha256Ffprobe $hashes.binaries.'ffprobe.exe'.sha256 `
  -PastaLicencas "$deps/licencas" `
  -FontesCorrespondentesZip "$deps/fontes-correspondentes.zip" `
  -PastaFfmpegTeste $teste `
  -Iscc 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
```

## Segurança e dependências restantes

Nos builds `windows-bundle`, somente as ferramentas distribuídas são aceitas; PATH e caminhos personalizados não são fallback. A GUI release tem subsistema Windows e todas as invocações multimídia usam `CREATE_NO_WINDOW`.

O EXE único extrai para `%LOCALAPPDATA%\DivisorVideo\ferramentas\<sha256>` via KnownFolder do usuário. O cache usa permissões herdadas do perfil, sem ACL própria; exige perfil privado. Rejeita symlinks/reparse points/junctions nos caminhos das ferramentas, usa lock e publicação atômica e recupera arquivos corrompidos. Arquivos íntegros não são regravados. A extração bloqueia renomeação dos diretórios; o diagnóstico e executor Windows mantêm handles dos EXEs sem compartilhamento de escrita/exclusão, verificando o hash no handle aberto. O executor mantém esses handles durante sua vida, inclusive clones, impedindo substituir o arquivo validado enquanto é utilizado. Isso ainda precisa de execução nativa; há um teste Windows dedicado na pipeline.

Não é uma garantia contra administrador, kernel comprometido ou invasor com controle completo da mesma conta: ele pode substituir o próprio aplicativo. Hash sem assinatura não autentica um aplicativo recebido de terceiro. Diretórios com junctions são recusados deliberadamente, incluindo perfis redirecionados; documentar eventual incompatibilidade no ambiente de destino.

DLLs nativas do Windows e driver gráfico continuam necessários; prévia externa depende do reprodutor associado. O usuário não deve instalar FFmpeg, Rust, Cargo, SQLite, Python, Java, .NET ou VC Redistributable. A lista conservadora de imports bloqueia dependências não reconhecidas. Os módulos observados na **inicialização** são conferidos contra o diretório Windows; seus hashes e estados Authenticode são registrados, assinaturas inválidas/indeterminadas bloqueiam o teste e runtimes VC++ dinâmicos são recusados. Isso não cobre bibliotecas carregadas depois em todos os fluxos, nem garante por si só a origem de um arquivo em System32; confirmar em VM limpa e rastrear a sessão completa. Não copiar DLLs do host de build para esconder falhas.

## Licenças e fontes

A pipeline distribui o tarball exato autenticado FFmpeg, a assinatura, configuração, scripts, objetos e bibliotecas para relink, fontes/avisos MinGW e textos LGPL. As crates são obtidas por `cargo vendor --locked`, preservando seus arquivos originais de licença e copyright; fontes do aplicativo/SQLite e biblioteca padrão Rust acompanham o pacote. O inventário inclui versões e expressões de licença, sem tratar uma expressão SPDX como substituta do texto legal. `LICENSE` materializa a licença MIT já declarada no projeto.

A revisão dos arquivos reais gerados ainda está pendente: scripts não atestam automaticamente suficiência jurídica. Não publicar sem conferir que todos os materiais correspondem aos binários, incluindo componentes nativos transitivos. O EXE único precisa ser redistribuído junto ao ZIP de licenças/fontes. Manter esses materiais disponíveis pelo mesmo canal dos binários e os direitos de modificação/relink; o código do projeto permite recompilar ferramentas modificadas com novos hashes. Nenhum build nonfree/GPL é admitido no perfil distribuído.

## Matriz obrigatória em Windows 10 e Windows 11 x64 limpos

Executar **cada item para os três formatos**, em conta padrão, sem Rust/FFmpeg/VC Redistributable adicional, com rede desconectada durante o uso. Registrar hash do artefato, versão exata do SO, GPU/driver, operador, data e evidências. Testar Inno Setup somente para o formato instalador; registrar não aplicável nos outros dois sem converter em teste aprovado.

- Inicialização e ausência de consoles, inclusive subprocessos.
- Tema automático claro/escuro, mudança durante execução e preferência persistente.
- Seleção de arquivos e pastas; subpastas; arrastar/soltar.
- Miniaturas e processamento simultâneo.
- Vídeos reais de 40–70 MB e maiores que 1 GB, incluindo H.264/H.265 e múltiplas faixas.
- Cada parte <= 10.000.000 bytes, decodificação íntegra, payloads audiovisuais preservados, original intacto.
- Cancelamento e limpeza de arquivos incompletos; reprocessamento.
- Histórico SQLite após reinício, filtros/exportação e exclusão sem apagar vídeos.
- Configurações persistentes e abertura da pasta de saída.
- Operação totalmente offline e sem administrador.
- Instalação/desinstalação, atalhos e preservação de dados pessoais.
- Cache: primeira extração, reutilização, concorrência, corrupção/recuperação e tentativa de substituição durante uso.
- Portátil/instalado: corrupção bloqueada, sem fallback; reparação por reinstalação/restauração do pacote.
- Caminhos com espaços/caracteres especiais, pasta sem escrita e disco cheio.
- Rastreamento das DLLs carregadas durante **todos** os fluxos, não somente imports estáticos/início.

**Resultado atual: Windows 10 e Windows 11 não testados; todos os formatos ausentes.** No Linux foram aprovados 39 testes, fmt/Clippy/build release e as fixtures de 55.605.302 bytes e 1.112.082.822 bytes (6 e 115 partes; maior parte 9.733.466 bytes; vídeo/áudio idênticos). Essas evidências não certificam Windows.
