# Divisor de Vídeo (`divisor_video` v2.0.0)

Aplicativo **desktop multiplataforma (`egui`/`eframe`)** e **ferramenta de linha de comando (CLI)** desenvolvidos em **Rust (Edição 2024)** para dividir automaticamente arquivos de vídeo em múltiplos segmentos, respeitando rigorosamente o limite máximo configurável (padrão **10 MB = `10.000.000` bytes**) — preservando **100% da qualidade audiovisual original** por meio de **Stream Copy (`-c copy`)** via **FFmpeg** e **FFprobe**, sem recodificação.

---

## Componentes Principais

1. **Biblioteca do núcleo (`src/lib.rs`)**: análise streaming via FFprobe, planejamento adaptativo de cortes em keyframes, validação de integridade, publicação atômica e serviços desktop (`historico`, `miniaturas`, `fila_processamento`, `configuracoes_persistentes`, `integracao_so`, `notificacoes`) — **totalmente independentes de `egui`**.
2. **Interface CLI (`divisor_video`)**: ferramenta leve de terminal para scripts e automação em lote (compilável sem dependências gráficas via `--no-default-features`).
3. **Aplicativo Desktop (`divisor_video_gui`)**: interface gráfica moderna, minimalista, responsiva e em português brasileiro com:
   - **4 áreas principais**: Início, Histórico, Configurações e Sobre e Ajuda.
   - **Tema Escuro, Claro e Automático do Sistema** + persistência das dimensões da janela e suporte HiDPI.
   - **Arrastar e soltar** de arquivos e diretórios + seleção por diálogos nativos + busca recursiva + detecção de duplicados e formatos inválidos.
   - **Limites decimais predefinidos** (`5 MB`, `10 MB padrão`, `20 MB`, `25 MB`, `50 MB`, `100 MB`, `200 MB`, `500 MB`, `1 GB`, `Personalizado`) com margem de segurança automática.
   - **Fila visual assíncrona** com 9 estados, barra de progresso individual e global, velocidade em MB/s, tempo decorrido, ETA, cancelamento individual/global, reordenação, reprocessamento de falhas e adição de vídeos durante a execução.
   - **Miniaturas com cache local limitado**, informações técnicas completas e pré-visualização no reprodutor padrão do sistema.
   - **Histórico local em SQLite (`rusqlite` bundled)** com pesquisa por nome, filtros por data e status, reprocessamento, exclusão segura (nunca apaga seus vídeos) e exportação em CSV e JSON.
   - **Detecção automática de FFmpeg/FFprobe em 4 níveis**: caminho personalizado → diretório do aplicativo → pasta `ferramentas/` → `PATH` do sistema.
   - **100% offline, sem telemetria e com `unsafe_code = "forbid"`**.

---

## Pré-requisitos para Compilação

- **Rust** estável recente com Edição 2024 (`1.85+`).
- **FFmpeg e FFprobe** instalados no `PATH` (ou colocados em `ferramentas/` junto ao binário portátil).
- No Linux (para compilar a GUI): headers de desenvolvimento GTK3, OpenGL/EGL, X11/Wayland (`gtk3`, `libxkbcommon`, `dbus`).

## Compilação e Execução

```bash
# Compilar CLI + Aplicativo Desktop em modo otimizado
cargo build --release

# Executar aplicativo desktop
./target/release/divisor_video_gui

# Executar CLI
./target/release/divisor_video video_original.mp4 -o ./saida

# Compilar somente o núcleo + CLI (zero dependências gráficas)
cargo build --release --no-default-features --bin divisor_video
```

### Exemplos da CLI

```bash
# Dividir vídeo com limite padrão de 10 MB (10.000.000 bytes)
divisor_video video_original.mp4

# Processar diretório em paralelo (4 workers), busca recursiva e limite de 25 MB
divisor_video ./videos -r -j 4 -m 25000000 -o ./segmentados

# Autorizar sobrescrita e exibir logs detalhados
divisor_video video.mkv --sobrescrever -v
```

---

## Qualidade, Testes e Empacotamento

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Para distribuição portátil (Linux `.tar.gz`/AppImage/`.deb`/`PKGBUILD`, Windows `.zip`/Inno Setup e macOS `.app`/`.dmg`), requisitos de bibliotecas do sistema, assinatura de código e obrigações de licenças (MIT + LGPL/GPL do FFmpeg), consulte:

- [docs/DISTRIBUICAO_E_LICENCAS.md](docs/DISTRIBUICAO_E_LICENCAS.md)
- [docs/PROGRESSO.md](docs/PROGRESSO.md) — planejamento e andamento, incluindo os requisitos de aparência adaptativa ao sistema na Fase 6 e a preparação arquitetural na Fase 1 (implementação ampliada pendente).

### Distribuição Windows autossuficiente

A [pipeline Windows x64](.github/workflows/windows-x64.yml) pode ser executada em **Actions → Windows x64 → Run workflow** ou por tag `windows-v*`. Compila FFmpeg 9.1 LGPL a partir do fonte oficial autenticado, depois o aplicativo com MSVC/CRT estático e SQLite bundled. Reaproveita o empacotador para produzir em `dist/windows-x64/`:

- `DivisorVideo-Windows-x64.exe` (ferramentas incorporadas e cache verificado).
- `DivisorVideo-Windows-x64-Portable.zip` (GUI, CLI, ferramentas e licenças/fontes).
- `DivisorVideo-Windows-x64-Setup.exe` (instalador por usuário).
- Pacote de licenças/fontes para acompanhar o EXE único, SHA256SUMS e relatórios.

**Distribuição ainda não finalizada:** a pipeline não foi executada nesta sessão; nenhum artefato Windows foi gerado. Os testes de build/CLI no runner Windows Server são distintos da validação gráfica em Windows 10/11 limpos. O usuário final não deve instalar dependências de desenvolvimento; somente o mantenedor usa essas ferramentas durante o build.

Consulte [procedência, licenças, execução da pipeline e matriz de aceitação](docs/WINDOWS_AUTOSSUFICIENTE.md). O [relatório local](dist/windows-x64/RELATORIO.md) identifica resultados reais e pendências, sem hashes ou tamanhos inventados.
