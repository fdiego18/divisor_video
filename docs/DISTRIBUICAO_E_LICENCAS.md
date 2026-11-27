# Distribuição Multiplataforma, Bibliotecas de Sistema, Assinatura de Código e Licenças

Este documento detalha separadamente os requisitos de bibliotecas do sistema operacional, procedimentos de assinatura de código, obrigações de licença para distribuições portáteis com FFmpeg/FFprobe e o estado real de validação em cada plataforma.

---

## 1. Estado Real de Validação por Plataforma

> [!IMPORTANT]
> Nenhuma plataforma é declarada validada sem compilação e execução de testes reais no ambiente correspondente.

| Plataforma | Arquitetura | Estado de Validação | Observações |
| --- | --- | --- | --- |
| **Linux** (Arch/Manjaro/Debian/Ubuntu) | `x86_64` | **Compilado e testado** | Binários CLI e GUI compilados em release; suíte de testes unitários, integração e renderização gráfica headless validados neste host Linux. |
| **Windows 10 / 11** | `x86_64` | **Pipeline preparada — Não executada nem validada nativamente** | Pipeline MSVC/FFmpeg LGPL preparada; requer execução e aceitação gráfica em Windows 10/11 limpos. Ver [procedimentos](WINDOWS_AUTOSSUFICIENTE.md). |
| **macOS** | `ARM64` (Apple Silicon) e `x86_64` (Intel) | **Pipeline preparada — Não executada nem validada nativamente** | Requer compilação, teste nativo de `.app`/`.dmg` e notarização em host macOS com Xcode Command Line Tools. |

---

### Validação pendente da aparência adaptativa

O estado acima registra as entregas anteriores e não comprova os novos requisitos de aparência da [Fase 6](PROGRESSO.md#requisitos-obrigatórios-de-aparência-adaptativa--fase-6). A implementação ampliada e sua validação em sessões gráficas reais estão pendentes.

| Ambiente | Estado da aparência adaptativa | Limitações a verificar e documentar na execução |
| --- | --- | --- |
| KDE Plasma | Não testado | Disponibilidade do portal, destaque/fontes, perfil Breeze e decorações em X11/Wayland. |
| GNOME | Não testado | Preferências expostas pelo portal, perfil Adwaita, contraste e decorações em X11/Wayland. |
| XFCE | Não testado | Exposição das preferências do tema GTK ativo, notificações de mudança e fallback sem portal. |
| Cinnamon | Não testado | Integração GTK, preferências expostas, atualização dinâmica e fallback sem portal. |
| Windows 10 | Não testado | Perfil tradicional, destaque, alto contraste, diálogos e DPI por monitor. |
| Windows 11 | Não testado | Identificação da versão, perfil Fluent, destaque, alto contraste e DPI por monitor. |
| macOS | Não testado | Aparência Apple, fontes, destaque, acessibilidade, diálogos e escala entre monitores. |

Para cada execução, registrar versão do aplicativo, SO/desktop, backend e portal, procedimento, resultado por capacidade e evidência. Os itens da última coluna são pontos de validação, não limitações já constatadas. Documentar separadamente dados que o sistema não expõe, fallbacks utilizados e diferenças visuais observadas. Componentes estilizados pelo `egui` não são widgets nativos; verificar a integração nativa das decorações e dos diálogos separadamente.

## 2. Requisitos de Bibliotecas do Sistema (Usuário Final)

O usuário final **não precisa** instalar Rust, Cargo nem ferramentas de desenvolvimento. O SQLite já é compilado estaticamente no binário (`rusqlite` com feature `bundled`).

### Linux (Arch Linux, Manjaro, Debian, Ubuntu)
- **Gráficos / Janela**: OpenGL (`libGL` / `libEGL` via Mesa ou driver proprietário), Wayland (`libwayland-client`) ou X11 (`libX11`, `libxkbcommon`).
- **Diálogos nativos**: GTK 3 (`libgtk-3-0` no Debian/Ubuntu; `gtk3` no Arch/Manjaro) ou XDG Desktop Portal.
- **Notificações (opcional)**: D-Bus (`libdbus-1`) com daemon de notificações do ambiente desktop.
- **Multimídia**: `ffmpeg` e `ffprobe` (dispensados de instalação separada caso empacotados na pasta `ferramentas/` da distribuição portátil).

### Windows 10 e 11 (64 bits)
- **Sistema**: Windows 10 (1809+) ou Windows 11 64 bits (Universal C Runtime nativo do SO).
- **Gráficos**: Driver compatível com OpenGL 3.2+ / WGPU (DirectX 12/Vulkan/OpenGL conforme driver).
- **Console**: O binário `divisor_video_gui.exe` utiliza `#![windows_subsystem = "windows"]` em release e aplica `CREATE_NO_WINDOW` ao invocar `ffmpeg.exe`/`ffprobe.exe`, operando sem abrir janela de terminal.
- **Multimídia**: os pacotes Windows exigem FFmpeg/FFprobe estáticos, hashes incorporados e auditoria de DLLs. Consulte [distribuição Windows autossuficiente](WINDOWS_AUTOSSUFICIENTE.md).

### macOS (Apple Silicon ARM64 e Intel x86_64)
- **Sistema**: macOS 12 Monterey ou superior (frameworks nativos `AppKit`, `CoreGraphics`, `Metal`/`OpenGL`).
- **Multimídia**: Colocar `ffmpeg` e `ffprobe` em `Divisor de Vídeo.app/Contents/MacOS/ferramentas/` (compilados para a mesma arquitetura ou universais via `lipo`).

---

## 3. Assinatura de Código e Notarização

### Linux
- Recomenda-se assinar os artefatos distribuídos (`.tar.gz`, `.AppImage`, `.deb`, `.pkg.tar.zst`) com chave **GPG** do mantenedor e publicar arquivo `SHA256SUMS.asc`.

### Windows (Authenticode)
- Assinar `divisor_video_gui.exe`, `divisor_video.exe` e o instalador `DivisorVideo-*-setup.exe` com certificado EV/OV Code Signing via `signtool.exe`:
  ```powershell
  signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /a divisor_video_gui.exe
  ```

### macOS (Code Signing + Notarização Apple)
- Assinar internamente os binários em `Contents/MacOS/ferramentas/`, depois os binários principais e o bundle `.app` com `Hardened Runtime`:
  ```bash
  codesign --force --options runtime --timestamp --sign "Developer ID Application: NOME (TEAMID)" "Divisor de Vídeo.app/Contents/MacOS/ferramentas/ffmpeg"
  codesign --force --options runtime --timestamp --sign "Developer ID Application: NOME (TEAMID)" "Divisor de Vídeo.app/Contents/MacOS/ferramentas/ffprobe"
  codesign --force --options runtime --timestamp --sign "Developer ID Application: NOME (TEAMID)" --deep "Divisor de Vídeo.app"
  ```
- Submeter o `.dmg` ao serviço de notarização da Apple (`xcrun notarytool submit ... --wait`) e anexar o ticket (`xcrun stapler staple DivisorVideo-*.dmg`).

---

## 4. Licenças e Obrigações de Redistribuição

### Código próprio (`divisor_video`)
- Licenciado sob a **Licença MIT**.
- Crates Rust utilizadas são licenciadas predominantemente sob **MIT** e/ou **Apache-2.0** (além de SQLite em **Domínio Público** via `rusqlite/bundled`).

### Distribuição Portátil com FFmpeg e FFprobe
O `divisor_video` invoca `ffmpeg` e `ffprobe` como **processos externos independentes** (sem linkagem estática ou dinâmica das bibliotecas `libav*` dentro do binário Rust). Mesmo assim, ao **redistribuir os binários** de `ffmpeg` e `ffprobe` junto ao pacote portátil/instalador, você deve cumprir a licença exata da compilação do FFmpeg escolhida:

1. **Compilação FFmpeg LGPL v2.1+ (recomendada para redistribuição)**:
   - Compilar sem `--enable-gpl` nem `--enable-nonfree`.
   - Incluir o texto integral da licença **LGPL v2.1** (ou v3) na pasta `licencas/`.
   - Informar claramente a versão exata e disponibilizar o código-fonte correspondente da compilação do FFmpeg distribuída.
2. **Compilação FFmpeg GPL v2+/v3+ (quando habilitados componentes GPL como `libx264`/`libx265`)**:
   - Incluir o texto integral da licença **GPL** aplicável na pasta `licencas/`.
   - Cumprir as obrigações de disponibilização do código-fonte correspondente (e scripts de build) dos binários `ffmpeg`/`ffprobe` distribuídos.
3. **Nunca redistribuir compilações `--enable-nonfree`** (ex: `libfdk_aac`), pois são legalmente não redistribuíveis.

---

## 5. Comandos Rápidos de Empacotamento

- **Linux portátil + AppDir/AppImage**:
  ```bash
  INCLUIR_FFMPEG=1 ./empacotamento/linux/empacotar_portatil_e_appimage.sh
  ```
- **Debian / Ubuntu (`.deb`)**:
  ```bash
  ./empacotamento/linux/empacotar_deb.sh
  ```
- **Arch Linux / Manjaro (`PKGBUILD`)**:
  ```bash
  cd empacotamento/linux && makepkg -si
  ```
- **Windows (PowerShell em host Windows 10/11 x64)**:
  ```powershell
  # Consulte WINDOWS_AUTOSSUFICIENTE.md para os parâmetros obrigatórios e testes.
  ```
- **macOS (host macOS — `arm64`, `x86_64` ou `universal`)**:
  ```bash
  ./empacotamento/macos/empacotar_macos.sh universal
  ```
