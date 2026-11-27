# Progresso da Evolução Desktop — `divisor_video`

Este documento registra o andamento incremental de todas as fases da evolução do projeto `divisor_video` para aplicativo desktop completo em Rust Edition 2024 (`egui`/`eframe`), preservando integralmente o núcleo de processamento, a CLI e todos os testes existentes.

## Status Geral das Fases

| Fase | Descrição | Status |
| --- | --- | --- |
| Fase 1 | Auditoria do núcleo e preparação da arquitetura gráfica | Base concluída; arquitetura de aparência adaptativa pendente |
| Fase 2 | Interface principal, seleção de arquivos e arrastar e soltar | Concluída |
| Fase 3 | Fila, processamento assíncrono, progresso e cancelamento | Concluída |
| Fase 4 | Miniaturas, informações técnicas e acesso aos segmentos | Concluída |
| Fase 5 | Histórico SQLite e configurações persistentes | Concluída |
| Fase 6 | Temas, notificações e refinamentos visuais adaptativos ao sistema | Base concluída; requisitos adaptativos pendentes |
| Fase 7 | Empacotamento multiplataforma | Concluída |
| Fase 8 | Testes de integração, testes gráficos e validação final | Base concluída; validação dos novos requisitos de aparência pendente |

---

Os requisitos de aparência adaptativa abaixo ampliam o planejamento existente. As entregas anteriores permanecem registradas, mas não comprovam a implementação nem a validação dos novos requisitos. Esta atualização especifica trabalho futuro; não altera o código do aplicativo.

## Evidências e Entregas por Fase

### Fase 1 — Auditoria do núcleo e preparação da arquitetura gráfica
- Separação limpa em 3 componentes: biblioteca do núcleo (`src/lib.rs`, compilável com `--no-default-features` sem `egui`), CLI (`src/main.rs`) e GUI (`src/bin/divisor_video_gui.rs` com `windows_subsystem = "windows"` em release no Windows).
- `src/integracao_so.rs`: diretórios nativos multiplataforma (`dirs`), espaço livre em disco (`fs2`), abertura nativa de arquivos/pastas (`open`) e lista ordenada de descoberta de ferramentas em 4 níveis.
- `src/executor_ffmpeg.rs`: detecção em 4 níveis (personalizado → diretório do app → pasta `ferramentas/`/`tools/`/`bin/` → `PATH`), diagnóstico com versão (`DiagnosticoFerramentas`), extração somente leitura de miniaturas (`extrair_miniatura`) e `CREATE_NO_WINDOW` no Windows.
- `src/divisor_video.rs`: observador estruturado de progresso (`dividir_arquivo_video_com_observador`), verificação prévia de espaço livre em disco e uso de padrão temporário neutro (`seg_%03d.<ext>`) imune a caracteres especiais (`%`, acentos, espaços).
- `src/eventos.rs`: enumeração completa dos 9 estados da fila (`EstadoItemFila`) e contratos de eventos thread-safe independentes de `egui`.

#### Complemento obrigatório de arquitetura — preparar na Fase 1

- [ ] Criar um módulo independente de aparência, proposto em `src/aparencia/`, responsável pela detecção do ambiente e pelo gerenciamento dos temas, sem dependência do núcleo de processamento de vídeos. Manter Rust, `egui` e `eframe` e o isolamento pela feature `gui`, preservando a compilação da CLI sem dependências gráficas.
- [ ] Definir contratos testáveis para ambiente detectado (SO, versão relevante e desktop Linux), preferências do sistema (claro/escuro, destaque, fontes, escala, acessibilidade e contraste), preferência do usuário e estilo efetivo. Representar explicitamente informações indisponíveis e a origem dos valores usados.
- [ ] Separar provedores por plataforma, resolução de preferências, observação de alterações e aplicação dos estilos no `egui`. Usar compilação condicional (`cfg(target_os = ...)`) e dependências Cargo específicas por destino, evitando dependências de outros sistemas.
- [ ] Prever provedores substituíveis por simuladores nos testes e um fallback para ambientes desconhecidos. No Linux, priorizar XDG Desktop Portal e D-Bus quando disponíveis; complementar a identificação do desktop por informações de sessão, como `XDG_CURRENT_DESKTOP` e `DESKTOP_SESSION`, normalizando aliases e valores compostos para KDE Plasma, GNOME, XFCE e Cinnamon.
- [ ] Planejar notificações de mudança com atualização da GUI sem reinicialização. Consultas ao sistema não devem bloquear a interface nem ocorrer a cada frame; usar eventos, cache e, quando necessário, consultas periódicas limitadas, com timeout e encerramento dos observadores.
- [ ] Reutilizar `TemaPreferido` e `configuracoes_persistentes`, preservando compatibilidade com configurações existentes. A camada gráfica deve consumir o estilo resolvido sem levar lógica de aparência ao processamento de vídeos.

### Fase 2 — Interface principal, seleção de arquivos e arrastar e soltar
- `src/gui/mod.rs` e `src/gui/aba_inicio.rs`: navegação lateral minimalista em 4 áreas (**Início**, **Histórico**, **Configurações**, **Sobre e Ajuda**), arrastar e soltar global de arquivos e diretórios, diálogos nativos (`rfd`), busca recursiva opcional, detecção de duplicados, rejeição de formatos inválidos, remoção individual e limpeza da fila.
- Limites predefinidos em unidades decimais (`5 MB`, `10 MB (padrão)`, `20 MB`, `25 MB`, `50 MB`, `100 MB`, `200 MB`, `500 MB`, `1 GB`, `Personalizado`) com cálculo automático da margem de segurança.

### Fase 3 — Fila, processamento assíncrono, progresso e cancelamento
- `src/fila_processamento.rs`: análise em segundo plano dedicada, pool paralelo com concorrência configurável (`1..=16`), adição dinâmica de novos vídeos durante a execução, cancelamento individual e cancelamento global seguro com encerramento de processos filhos, reprocessamento de falhas, reordenação de itens pendentes, barra individual e global, velocidade em MB/s, tempo decorrido e ETA estimado.

### Fase 4 — Miniaturas, informações técnicas e acesso aos segmentos
- `src/miniaturas.rs`: geração de miniaturas JPEG leves via FFmpeg (somente leitura) com cache local limitado (200 arquivos / 30 MB) e cache de texturas na GPU/RAM limitado a 64 itens em `src/gui/estado.rs`.
- Exibição completa por vídeo (miniatura, nome, caminho, tamanho, duração, resolução, FPS, codec de vídeo, codec de áudio, partes estimadas), prévia do vídeo original e de cada segmento no reprodutor padrão do SO, abertura da pasta de saída e cópia de caminhos.

### Fase 5 — Histórico SQLite e configurações persistentes
- `src/historico.rs`: banco SQLite local embutido (`rusqlite` `bundled` com WAL), registro completo de metadados e lista detalhada de segmentos, pesquisa por nome/caminho, filtros por data e status, reprocessamento, exclusão individual e limpeza com modal de confirmação (sem apagar vídeos/segmentos) e exportação em CSV e JSON.
- `src/configuracoes_persistentes.rs`: persistência atômica (`.tmp` + `rename`) de tema, idioma, limite, diretório de saída, concorrência, abertura automática, busca recursiva, caminhos FFmpeg/FFprobe, preferências de notificação e dimensões/posição da janela.

### Fase 6 — Temas, notificações e refinamentos visuais
- `src/gui/componentes.rs`, `src/gui/aba_configuracoes.rs`, `src/gui/aba_sobre.rs` e `src/notificacoes.rs`: temas Escuro, Claro e Sistema (automático), ícones vetoriais HiDPI desenhados via `egui::Painter`, toasts flutuantes configuráveis, notificações nativas opcionais (`notify-rust`) e resumo final consolidado da fila.

#### Requisitos obrigatórios de aparência adaptativa — Fase 6

Objetivo: integrar visualmente o aplicativo ao desktop em execução, evitando uma identidade visual genérica ou desnecessariamente personalizada. Os componentes desenhados pelo `egui` são **estilizados**, não componentes nativos; essa distinção deve constar da documentação e da apresentação do produto.

- [ ] Detectar automaticamente o sistema operacional, distinguindo Windows 10 e Windows 11 para seleção do perfil visual; no Linux, identificar KDE Plasma, GNOME, XFCE e Cinnamon, com tratamento de ambientes desconhecidos.
- [ ] Usar o tema do sistema como padrão e acompanhar alterações entre claro e escuro durante a execução, sem reiniciar o aplicativo. Utilizar `egui::ThemePreference::System` conforme a versão instalada (atualmente `egui`/`eframe` 0.36 no manifesto), integrando os sinais dos provedores quando necessário e verificando o comportamento real do backend.
- [ ] Usar as cores de destaque do sistema quando disponíveis e adaptar tipografia, tamanhos de fonte, espaçamentos, bordas, botões, barras de progresso e demais componentes, incluindo estados de foco, seleção, interação e desabilitado. Substituir cores fixas incompatíveis com o perfil efetivo.
- [ ] Utilizar decorações nativas da janela e diálogos nativos para seleção de arquivos e diretórios. No Linux, priorizar a integração via portal quando disponível e verificar o backend efetivamente usado.
- [ ] Respeitar escala de exibição e DPI, inclusive escala fracionária e movimentação entre monitores com escalas diferentes, sem aplicar escala duas vezes.
- [ ] Respeitar preferências detectáveis de acessibilidade e contraste, incluindo fontes maiores, alto contraste e redução de movimento quando expostos pelo sistema. O destaque do sistema não deve tornar texto, foco ou controles ilegíveis.
- [ ] Disponibilizar exatamente três opções de aparência: **Automático — seguir o sistema** (padrão), **Claro** e **Escuro**. Salvar localmente a escolha usando a persistência existente. A escolha explícita Claro/Escuro prevalece sobre o esquema do sistema; retornar a Automático deve aplicar imediatamente o estado atual do SO. Demais preferências detectáveis de integração e acessibilidade continuam sendo respeitadas.

#### Identidade visual por plataforma

| Ambiente | Perfil visual exigido |
| --- | --- |
| KDE Plasma | Aparência inspirada no Breeze, respeitando preferências detectadas do usuário. |
| GNOME | Aparência inspirada no Adwaita. |
| XFCE | Aparência compatível com o tema GTK ativo. |
| Cinnamon | Aparência integrada ao ambiente GTK. |
| Windows 10 | Aparência inspirada nos componentes tradicionais do Windows. |
| Windows 11 | Aparência inspirada no Fluent Design. |
| macOS | Aparência inspirada nos componentes nativos da Apple. |

Esses perfis orientam a estilização em `egui`; não prometem reprodução integral dos temas nem substituição dos controles por widgets nativos. Decorações e diálogos nativos devem ser verificados separadamente.

#### Fallbacks e restrições de implementação

- [ ] Quando cores, fontes ou preferências não puderem ser detectadas, usar valores seguros e legíveis: preferência explícita do usuário; caso Automático, esquema informado pelo backend; na ausência de qualquer informação, esquema claro. Usar paleta de destaque com contraste adequado e fontes padrão disponíveis no `egui` quando não houver fonte do sistema utilizável.
- [ ] Falhas, ausência ou respostas inválidas de portais/APIs não podem impedir inicialização, seleção de aparência ou processamento. Registrar a limitação de forma diagnóstica, sem notificações repetitivas; permitir recuperação quando o serviço voltar a responder.
- [ ] Manter funcionamento totalmente offline, desempenho e portabilidade, sem Electron, tecnologias web ou servidores HTTP para a interface gráfica. Não modificar o núcleo de processamento de vídeos.
- [ ] Usar APIs específicas de cada plataforma por meio dos provedores definidos na Fase 1, preservando a política existente de `unsafe_code = "forbid"` no código do projeto.

#### Testes e critérios de aceite — Fase 6, com fechamento na Fase 8

- [ ] Testes de detecção: SO, versão relevante do Windows, desktops Linux, aliases/valores compostos e ambiente desconhecido, usando entradas controladas sem depender do desktop do executor.
- [ ] Testes de persistência: padrão Automático, gravação e leitura das três opções, compatibilidade com configurações anteriores e recuperação de arquivo ausente, inválido ou falha de gravação.
- [ ] Testes de alternância dinâmica: eventos claro → escuro → claro atualizam o estilo sem reinicialização; Claro/Escuro explícitos permanecem estáveis; retornar a Automático aplica o último estado do sistema. Verificar também mudanças de destaque e contraste quando suportadas.
- [ ] Testes de fallback: portal/D-Bus indisponível, timeout, valores inválidos, fonte ausente, APIs sem preferências, perda e recuperação do serviço, garantindo estilo legível e GUI responsiva.
- [ ] Testes de integração da aplicação de estilos e regressão da CLI/núcleo, mantendo a compilação sem a feature `gui`. Renderização headless complementa, mas não substitui, testes em sessões gráficas reais.
- [ ] Validar em ambientes gráficos reais: KDE Plasma, GNOME, XFCE, Cinnamon, Windows 10, Windows 11 e macOS; no Linux, cobrir X11 e Wayland onde suportados. Conferir claro/escuro durante execução, destaque, fontes, componentes, decorações, diálogos, persistência, acessibilidade e DPI, inclusive múltiplos monitores quando disponíveis.
- [ ] Registrar em `docs/DISTRIBUICAO_E_LICENCAS.md` versões do SO/desktop, backend gráfico e portal, versão do aplicativo, procedimentos, resultados, evidências e limitações por plataforma. Distinguir aprovado, falhou, indisponível e não testado para cada capacidade. Não declarar ambiente validado com base apenas em compilação ou testes headless.

A Fase 6 ampliada só poderá ser marcada concluída após a implementação e os testes previstos; a validação gráfica por plataforma deve ter evidências na Fase 8. Ambientes sem acesso para teste permanecem explicitamente pendentes.

### Fase 7 — Empacotamento multiplataforma
- Linux: `empacotamento/linux/empacotar_portatil_e_appimage.sh`, `empacotar_deb.sh`, `PKGBUILD`, `divisor_video.desktop` e `icone.svg`.
- Windows: `empacotamento/windows/empacotar_windows.ps1` e `instalador.iss` (Inno Setup).
- macOS: `empacotamento/macos/empacotar_macos.sh` (ARM64, x86_64 e universal `.app`/`.dmg`).
- Documentação dedicada: `docs/DISTRIBUICAO_E_LICENCAS.md` (requisitos de sistema, assinatura de código, licenças MIT/LGPL/GPL e estado honesto de validação por plataforma).

### Fase 8 — Testes de integração, testes gráficos e validação final
- `tests/desktop_gui.rs`: testes ponta a ponta da fila assíncrona com vídeo real (incluindo nome com espaços, acentos e `%`), geração/decodificação de miniaturas sem alteração do original, garantia de não exclusão de arquivos ao limpar histórico SQLite e renderização headless de todas as 4 abas em temas Claro e Escuro com `egui::Context::run_ui`.
- Todos os 25 testes anteriores preservados e aprovados + novos testes unitários e desktop.


### Distribuição Windows x64 autossuficiente — 26/11/2026

**Não concluída.** Pipeline preparada, mas ainda não executada em GitHub Actions. O ambiente Linux não consegue baixar os insumos pelo terminal (DNS), não possui Windows/MSVC/PowerShell e não disponibiliza repositório remoto utilizável. Nenhum ZIP/EXE Windows foi produzido.

- Reaproveitados empacotador/Inno Setup, SQLite bundled, CRT estático, `include_bytes!`, extração/cache e supressão de consoles.
- Acrescentados manifesto de dependências, aquisição/compilação do fonte FFmpeg 9.1 estável com assinatura oficial, perfil LGPL sem rede/GPL/nonfree e coleta de fontes/licenças/objetos de relink.
- Pipeline `.github/workflows/windows-x64.yml`: build de ferramentas no Ubuntu, MSVC e empacotamento no Windows Server 2022, artefatos/evidências em `dist/windows-x64/`.
- SHA-256 autenticado dos insumos novamente exigido na compilação. Caminhos de ferramentas com reparse points recusados; handles Windows sem compartilhamento de escrita/exclusão mantidos durante uso, com teste nativo previsto na pipeline.
- Scripts automatizados adicionais para inspeção PE/subsistema, módulos na inicialização, pacotes/cache e fixtures grandes; relatórios mantêm a aprovação gráfica separada.
- Linux: 39 testes aprovados; fmt, Clippy e build release aprovados. Fixtures reais: 55.605.302 bytes → 6 partes; 1.112.082.822 bytes → 115 partes. Maior parte 9.733.466 bytes; payloads de vídeo/áudio e originais preservados.
- Pendente: execução da pipeline; revisão dos insumos e licenças gerados; testes de interação gráfica, offline com rede desconectada, usuário padrão e DLLs durante todos os fluxos em Windows 10 e 11 limpos, para os três formatos. Nenhuma dessas verificações foi declarada aprovada.

Detalhes: [WINDOWS_AUTOSSUFICIENTE.md](WINDOWS_AUTOSSUFICIENTE.md). Evidências locais: [dist/windows-x64](../dist/windows-x64/).
