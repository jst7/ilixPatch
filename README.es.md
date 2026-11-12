<p align="center">
  <img src="app/src-tauri/icons/128x128@2x.png" width="128" height="128" alt="Logo de ilixPatch: un pato con botas de agua">
</p>

<h1 align="center">ilixPatch</h1>

<p align="center">
  Un <b>cliente de APIs</b> rápido, de código abierto y <b>local</b>: colecciones, entornos, scripts, tests,
  pruebas de carga y un cliente de Git integrado. Sin cuenta ni nube: tus peticiones son ficheros en tu repositorio.
</p>

<p align="center"><a href="README.md">Read in English</a></p>

> **Estado: desarrollo temprano (0.x).** Avanza rápido y el formato en disco aún puede cambiar (está versionado).

## Por qué ilixPatch
- **Rápido y ligero:** núcleo en Rust y [Tauri](https://tauri.app), que usa el webview del sistema en lugar de un Chromium empaquetado.
- **Local:** sin registro ni sincronización. Todo vive en la carpeta de tu proyecto.
- **Pensado para Git:** un fichero TOML por petición, así que los diffs se leen bien y es fácil revisarlos. Además trae un cliente de Git al estilo de IntelliJ.
- **Compatible con Postman donde importa:**
  - los scripts `pm.*` y las aserciones Chai funcionan sin cambios;
  - la CLI es compatible con Newman;
  - incluye un importador de colecciones, entornos y exportaciones completas de Postman.

## Funcionalidades
**Peticiones y editor**
- Métodos HTTP con colores, URL multilínea, params, headers y bodies JSON, texto o formulario.
- **Pega un comando cURL** en la barra de URL y se rellena la petición entera. Con **Copiar como cURL** la exportas, con las variables resueltas o con `{{variables}}`.
- Editores CodeMirror con JSON coloreado, plegado de nodos, buscar y reemplazar con regex y multilínea, y autocompletado de `pm`.
- Cancelar una petición colgada con `⌘.` / `Ctrl+.`.
- **Errores descriptivos** que explican la causa probable (DNS, conexión rechazada, timeout, certificado, mTLS, proxy…), con botones para solucionarlo.
- Respuesta con estado, tiempo, tamaño, headers, tests, consola, versión HTTP, redirecciones y certificado TLS.

**Organización**
- **Proyecto › Colección › Carpeta › Petición**. Cada proyecto suele ser un repositorio Git.
- **Entornos del proyecto**: se comparten por Git y los secretos van a un `.secret.toml` que git ignora.
- **Entornos globales**: son del usuario. Se pueden copiar en ambos sentidos entre proyecto y global.
- Peticiones abiertas como pestañas en una barra lateral derecha plegable, con buscadores en el árbol, las pestañas y el historial.
- **Historial** de todas las llamadas con su respuesta, guardado en SQLite local con retención por días y tamaño. Cada entrada se puede recuperar en una pestaña nueva.

**Scripts, tests y automatización**
- Scripts de pre-request y de tests con la API `pm.*` de Postman, sobre QuickJS integrado.
- **Runner de colecciones** con iteraciones, datos CSV o JSON, retardo y bail.
- **Pruebas de carga** con usuarios virtuales, perfiles, percentiles y gráficas en vivo.
- **CLI `ilixpatch`** compatible con Newman para CI, con informes JUnit, JSON y HTML.

**Git**
- Cualquier proyecto, colección o carpeta puede ser su propio repo con su propia rama.
- Cambios con diff semántico, commit, ramas, fetch/pull/push e historial con el detalle de cada commit.
- Revert, reset y cherry-pick.
- **Resolución de conflictos a tres bandas**, **Annotate (blame)** en los editores y clonar desde la interfaz.

**Red**
- Verificación SSL, CAs propias y **certificados de cliente (mTLS)** por host, en PEM o PKCS#12.
- Proxy, redirecciones, timeouts, versión TLS y HTTP/1.1 o HTTP/2, en global y por petición.
- CORS no aplica, porque las peticiones las envía el motor en Rust y no el navegador.

**Personalización**
- Temas claro, oscuro o del sistema, con colores editables que se pueden exportar e importar.
- Interfaz en castellano e inglés.

## Descarga
Instaladores y binarios de la CLI en **[Releases](https://github.com/jst7/ilixPatch/releases)**:
- macOS: `.dmg`
- Windows: `.msi` y `.exe`
- Linux: `.deb`, `.rpm` y `.AppImage`

Las primeras versiones pueden estar **sin firmar**:
- **macOS:** la primera vez, haz clic derecho en la app y elige *Abrir*. También puedes ejecutar `xattr -dr com.apple.quarantine /Applications/ilixPatch.app`.
- **Windows:** en el aviso de SmartScreen, pulsa *Más información* y después *Ejecutar de todas formas*.

## Compilar desde el código
### La forma rápida
```sh
git clone https://github.com/jst7/ilixPatch.git
cd ilixPatch
./init.sh
```
`init.sh` hace lo siguiente:
1. Detecta tu sistema: macOS, Debian/Ubuntu, Fedora, Arch, openSUSE, WSL o Git Bash.
2. Comprueba los requisitos y **ofrece instalar lo que falte**, preguntando antes.
3. Instala las dependencias y arranca la app.

| Comando | Qué hace |
|---|---|
| `./init.sh` | Comprueba, instala lo que falte y arranca la app en modo desarrollo |
| `./init.sh --check` | Solo comprueba; no cambia nada |
| `./init.sh --yes` | Instala lo que falte sin preguntar |
| `./init.sh --cli` | Ejecuta la CLI contra el proyecto de ejemplo |
| `./init.sh --build` | Genera los instaladores en `target/release/bundle/` |

### A mano
Necesitas:
- [Rust](https://rustup.rs) ≥ 1.85, Node.js ≥ 24 (LTS) y Git;
- los [requisitos de Tauri](https://tauri.app/start/prerequisites/) de tu sistema.

```sh
cd app && npm ci
npm run tauri dev        # la app con recarga en caliente
npm run tauri build      # instaladores en ../target/release/bundle/
cd .. && cargo build --release -p ilixpatch-cli   # → target/release/ilixpatch
```

### Pruébalo
1. Abre el ejemplo con **+ › Abrir carpeta…** y elige `examples/demo-project`.
2. Selecciona el entorno `dev`.
3. Pulsa `⌘/Ctrl+Enter`.

Para traer tu trabajo de Postman, usa **+ › Importar desde Postman…**.

## CLI
```sh
ilixpatch run examples/demo-project --env dev -r cli,junit --reporter-junit-export results/junit.xml
ilixpatch load examples/demo-project/jsonplaceholder --vus 5 --duration 30s --max-p95 500
ilixpatch import postman coleccion.json --project ./mi-proyecto
```
La tabla completa de flags, compatibles con Newman, está en el [README en inglés](README.md#cli-newman-compatible-runner-and-load-testing).

## Atajos de teclado
| Atajo (macOS / Windows·Linux) | Acción |
|---|---|
| `⌘ Enter` / `Ctrl Enter` | Enviar (también Enter en la URL) |
| `⌘ .` / `Ctrl .` | Cancelar la petición en curso |
| `⌘ S` / `Ctrl S` | Guardar |
| `⌘ W` / `Ctrl W` | Cerrar pestaña |
| `⌘ P` / `Ctrl P` | Buscar en el árbol o en el historial |
| `⌘ E` / `Ctrl E` | Buscar en las pestañas |
| `⌘ F` / `Ctrl F` | Buscar en el editor |
| `F7` / `Shift F7` | Siguiente o anterior conflicto en el merge |

## Desarrollo y publicación
- **Comprobaciones locales:**
  ```sh
  cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
  cd app && npm run check
  ```
- **CI:** el workflow `ci.yml` lo ejecuta en Linux, macOS y Windows.
- **Publicar una versión:**
  1. Sube la versión en `Cargo.toml` y en `app/src-tauri/tauri.conf.json`.
  2. Crea el tag y súbelo: `git tag v0.2.0 && git push origin v0.2.0`.
  3. El workflow `release.yml` genera los instaladores y la CLI en un **borrador de release**. Revísalo y publícalo.
- **Decisiones de diseño:** [docs/DECISIONS.md](docs/DECISIONS.md).

## Licencia
[MIT](LICENSE-MIT) o [Apache-2.0](LICENSE-APACHE), a elegir.
Componentes de terceros: [THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES).

## Autor
Creado por **Jorge Segovia Tormo**: [@jst7](https://github.com/jst7).
