# Correção da primeira execução Windows — 27/11/2026

Execução: `45997666880`. Job aplicativo: `140501285590`.

## Evidência e causa raiz

Os arquivos locais `erro-windows.log` e `compilacao-windows.log` mostram que
`Get-Content` falhou ao abrir `build/windows-deps/licencas/DEPENDENCIAS.json`,
às 12:23:11–12:23:12 UTC. O empacotador e o Cargo ainda não tinham sido chamados.
O download do artefato foi concluído com digest válido, mas seu tamanho era
somente **1.415 bytes**. Portanto, não era uma falha de compilação Rust/MSVC.

Para determinar a origem, foi consultado também o [log do job ferramentas
140501052403](https://github.com/fdiego18/divisor_video/actions/runs/45997666880/job/140501052403).
Às 12:19:55 UTC, ele registra, nesta ordem:

1. Assinatura válida do fonte FFmpeg, com a chave cujo fingerprint já está fixado.
2. `cp: cannot stat 'empacotamento/windows/compilar_ffmpeg.sh': No such file or directory`.
3. Upload de somente três arquivos, totalizando os mesmos 1.415 bytes.

O script foi invocado com `$0` relativo e mudou para `windows-deps/downloads`.
Depois tentou copiar `"$0"` para as fontes correspondentes. Esse caminho deixou
de apontar para o script, e `set -e` encerrou a execução **antes de configure/make
e da geração de DEPENDENCIAS.json**.

O workflow usava `/usr/bin/bash -e {0}` e `script | tee log`. Sem `pipefail` no
shell externo, o retorno zero de `tee` mascarou a falha do script. O job verde
confirmava apenas o sucesso da pipeline de shell; a autenticação realmente
passou, mas a compilação FFmpeg não chegou a acontecer. O upload aceitava a
entrega parcial porque `if-no-files-found: error` exige algum arquivo, não todos.

## Correções

- Resolver o caminho absoluto do script antes dos `cd` e utilizá-lo na cópia.
  A versão FFmpeg, URLs, chave/fingerprint, autenticação, opções configure,
  compiladores, codecs, perfil LGPL, link estático e fontes/relink foram mantidos.
- Definir `shell: bash` e `set -euo pipefail` no workflow, propagando falhas através
  de `tee`. Informar explicitamente o diretório absoluto de saída.
- Validar a entrega antes do upload e após a extração: arquivos obrigatórios
  não vazios, manifesto JSON, tamanho/SHA-256 de ambos os executáveis e estrutura/CRC
  do ZIP de fontes. Isso complementa, sem substituir, GPG e as auditorias existentes.
- Transportar um único TAR com `bin/`, `licencas/`, ZIP de fontes e log,
  preservando a estrutura entre Linux e Windows. Uma entrega incompleta agora
  interrompe o produtor ou o consumidor antes da preparação onerosa do aplicativo.
- Atualizar todas as Actions que usavam Node.js 20, mantendo SHA completo.

Versões estáveis e `runs.using: node24` foram consultados na API oficial do GitHub
e no `action.yml` de cada commit em 27/11/2026:

| Action | Versão | SHA |
| --- | --- | --- |
| [checkout](https://github.com/actions/checkout/releases/tag/v7.0.1) | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| [upload-artifact](https://github.com/actions/upload-artifact/releases/tag/v7.0.2) | v7.0.2 | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |
| [download-artifact](https://github.com/actions/download-artifact/releases/tag/v8.0.2) | v8.0.2 | `9000827ccba6bdab643e8b6fd33ac0654aef8333` |

O runner observado é `2.338.2`; supera o mínimo `2.327.1` para Node.js 24
documentado pelo [checkout](https://github.com/actions/checkout/tree/3d3c42e5aac5ba805825da76410c181273ba90b1).
O upload continua com o empacotamento ZIP padrão da Action, contendo o único TAR;
o download descompacta esse ZIP e o passo seguinte extrai o TAR.

## Revisão Windows/MSVC

- Mantidos `x86_64-pc-windows-msvc`, `crt-static` em `.cargo/config.toml` e a
  exigência correspondente em `build.rs`; SQLite continua `bundled`. Nenhuma
  biblioteca ou dependência Cargo foi trocada, e Cargo.lock permanece intacto.
- Caminhos das ferramentas usam Path/PathBuf e argumentos separados em Command;
  não há shell intermediário nas invocações FFmpeg. A geração de `include_bytes!`
  usa escape de literal Rust para caminhos Windows. Os testes existentes de
  distribuição continuam cobrindo espaços e acentos.
- Mantidos CREATE_NO_WINDOW, drenagem paralela de stderr, espera pelos processos,
  controles de cancelamento e proteção contra escrita/substituição das ferramentas.
- Mantidos SHA-256 ancorados no aplicativo, rejeição de symlinks/reparse points,
  locks de extração, cache no diretório local do usuário, instalador sem elevação
  obrigatória e ausência de fallback para ferramentas externas nos pacotes.
- Mantidas auditorias PE/imports/subsistema GUI, módulos carregados/assinaturas,
  PATH restrito, testes de adulteração, cache, instalador e arquivos grandes.
  A lista de DLLs permitidas não foi ampliada para esconder dependências.

Não foi encontrado erro adicional demonstrável no código Rust nesta revisão.
O Clippy para Windows GNU cobre ramos `cfg(windows)`, mas **não comprova link MSVC,
comportamento de ACL/handles em execução nem funcionamento gráfico**.

## Regressão local

Executados no Linux com Rust 1.97.1:

| Verificação | Resultado |
| --- | --- |
| `cargo test --locked --offline` | 39 aprovados: 26 unitários, 3 desktop, 10 integração; zero ignorados |
| `cargo fmt --check` | Aprovado |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | Aprovado |
| `cargo clippy --locked --offline --all-targets --target x86_64-pc-windows-gnu -- -D warnings` | Aprovado |
| `cargo build --release --locked --offline` | Aprovado |
| `python3 -m unittest discover -s empacotamento/windows -p 'test_*.py' -v` | 6 aprovados |
| `bash -n empacotamento/windows/compilar_ffmpeg.sh` | Aprovado |
| Parse YAML e `git diff --check` | Aprovados |

Os seis testes adicionais verificam manifesto ausente, binário ausente/adulterado,
fontes incompletas, transporte com espaços/acentos, resolução do script após `cd`
e propagação de erro por `tee`. O teste da receita simula download/GPG e para
antes da compilação; não constitui nova validação criptográfica do FFmpeg.
Os 39 testes existentes e todos os arquivos Rust permanecem sem alterações.

Não há Windows, PowerShell ou MSVC local. A execução nativa MSVC, os três formatos
e os testes Windows 10/11 continuam pendentes da nova pipeline e das VMs previstas.
Nenhum binário Windows foi declarado aprovado, nenhum commit/push foi realizado.

Mensagem de commit sugerida: `Corrige entrega FFmpeg e propagação de falhas na pipeline Windows`.
