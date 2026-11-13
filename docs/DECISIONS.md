# Decisiones de diseño de ilixPatch

Registro de las decisiones de arquitectura y producto. Cada entrada explica **qué** se decidió y **por qué**.
Cuando una decisión cambie, no se borra: se marca como *Reemplazada por DXX* y se añade la nueva.

| Estado | Significado |
|---|---|
| ✅ Aceptada | Vigente e implementada (o en curso) |
| 🗓️ Planificada | Decidida, pendiente de implementar |
| ♻️ Reemplazada | Ya no aplica; ver la decisión que la sustituye |

---

## D01 · Objetivo del producto ✅
Cliente de APIs de código abierto, alternativa a Postman: **rápido, local y sin cuenta**, y con las colecciones versionables en git.
- **Por qué:** Postman es lento por Electron, por la sincronización en la nube y por obligar a tener cuenta. Esa es nuestra diferencia.
- **Competencia:** Yaak (Tauri + Rust), Bruno (Electron, archivos en git) y Hoppscotch (web). Hay que diferenciarse en rendimiento, en la experiencia con git (D16) y en la CLI.

## D02 · Rust para el núcleo ✅
Toda la lógica (modelo, variables, almacenamiento y ejecución HTTP) está en Rust.
- **Por qué:** rendimiento, poco consumo de memoria y un binario único que sirve para la app y para la CLI.

## D03 · Tauri v2 para la app de escritorio ✅
- **Alternativas:** GPUI e iced/egui/Slint (UI nativa), o Electron.
- **Por qué Tauri:** es mucho más ligero que Electron (instalador de ~10 MB en lugar de ~200 MB, arranque rápido), tiene el backend en Rust y permite usar editores web maduros como CodeMirror. Además hay muchos más contribuidores que saben tecnologías web que GPUI.
- **Riesgos y cómo se mitigan:**
  - En Linux usa WebKitGTK, que es más lento; hay que probar en los tres sistemas.
  - La comunicación (IPC) entre Rust y JS cuesta: los datos grandes se quedan en Rust (D09).
  - El núcleo está separado de la UI (D05), así que se puede añadir otra UI más adelante sin reescribirlo.

## D04 · Svelte 5 + SvelteKit (modo SPA, adapter-static) para el frontend ✅
- **Por qué:** Svelte es un compilador, así que el bundle es pequeño y no hay virtual DOM. Es fácil de aprender.
- **Alternativa descartada:** SolidJS, también válida. React se descartó por tamaño y rendimiento.
- **Normas:** usar solo *runes* (`$state`, `$derived`, `$props`), sin stores antiguos ni `export let`.

## D05 · Workspace de Cargo con el núcleo separado de la UI ✅
```
crates/core     modelo, {{variables}}, formato en disco, proyectos   (sin HTTP ni UI)
crates/engine   ejecución HTTP (reqwest, conexiones reutilizadas, medición de tiempos)
crates/cli      binario `ilixpatch` para terminal y CI
app/src-tauri   comandos de Tauri (capa fina sobre core y engine)
app/src         interfaz en Svelte
```
- **Por qué:** el núcleo se puede probar sin UI, la CLI y la app comparten lógica, y los contribuidores pueden aportar a los importadores o al motor sin tocar la interfaz.

## D06 · Jerarquía Proyecto > Colección > Carpeta > Petición ✅
- Un **proyecto** es una carpeta, normalmente un repositorio git, que contiene `.ilixpatch/config.toml`.
- Una **colección** es un subdirectorio con `collection.toml`. Las **carpetas** se pueden anidar. Cada **petición** es un archivo `.toml`.
```
mi-repo/
├── .ilixpatch/config.toml        # version, name, collections_dir (opcional, por defecto ".")
└── users-api/                   # colección
    ├── collection.toml          # nombre y variables de la colección
    ├── environments/
    │   ├── dev.toml
    │   └── dev.secret.toml      # ignorado por git
    └── users/                   # carpeta
        └── list-users.toml      # petición
```
- La **lista de proyectos abiertos** es del usuario, no del repositorio. Se guarda en `projects.toml`, dentro del directorio de configuración de la app.

## D07 · Formato en disco: TOML, un archivo por petición ✅
- **Por qué:** los diffs se leen bien, hay pocos conflictos al hacer merge y se puede revisar en un PR. TOML es legible y admite cadenas multilínea para los bodies.
- El formato está **versionado** (`version = 1` y la constante `FORMAT_VERSION`). Los cambios incompatibles obligan a subir la versión.
- Los nombres de archivo son *slugs* del nombre de la petición, con sufijos `-2`, `-3`… si se repiten.

## D08 · Variables y secretos ✅
- Sintaxis `{{variable}}`. Prioridad de menor a mayor: **colección → entorno → secretos**. Más adelante se añadirán variables de ejecución (runtime).
- Los secretos van en `environments/<env>.secret.toml`, que **está en `.gitignore`** y se fusiona sobre el entorno.
- Si falta una variable, es un error explícito que la nombra. Nunca se envía `{{x}}` literal.
- 🗓️ **Futuro:** guardar los secretos en el llavero del sistema (crate `keyring`).

## D09 · Reglas de rendimiento ✅
- Un solo `reqwest::Client` compartido, para reutilizar conexiones y sesiones TLS.
- Los datos grandes (bodies de respuesta) **se quedan en Rust**. La UI pide solo lo que muestra.
  - 🗓️ Pendiente: paginar o hacer streaming de las respuestas enormes por canales de Tauri.
- Los editores son CodeMirror 6, que solo renderiza las líneas visibles.
- Se mide en CI: tiempo de arranque, RAM en reposo y render de respuestas grandes. 🗓️

## D10 · Editores: CodeMirror 6 ✅
- **Alternativa descartada:** Monaco, más pesado.
- **Funciones:** JSON coloreado, búsqueda y reemplazo (⌘F), plegado de nodos (en el margen, con atajos de teclado y con "plegar/desplegar todo"), resaltado de paréntesis, números de línea y modo solo lectura para las respuestas.
- Componente reutilizable `app/src/lib/components/CodeEditor.svelte`, con su configuración en `app/src/lib/editor/setup.ts`.

## D11 · Temas personalizables ✅
- Todos los colores salen de **variables CSS** (`--bg`, `--surface`, `--text`, `--accent`, `--method-*`, `--syntax-*`…). Las define en tiempo de ejecución `app/src/lib/theme/index.svelte.ts`.
- **Regla: ningún componente escribe colores a mano** ni usa `prefers-color-scheme`.
- Temas incluidos: **Claro** y **Oscuro**, de solo lectura. "Sistema" sigue al sistema operativo en vivo.
- **Color de acento por defecto: verde** (`#0b8a5c` en claro y `#20c997` en oscuro), para no parecerse a Postman (naranja).
  - Es distinto del verde de "éxito" y del de GET, para no confundir la marca con un estado.
  - El contraste del texto sobre el acento es 4,4:1 en claro y 8:1 en oscuro.
- El usuario puede **duplicar** un tema, **editar** cualquier color (agrupados en interfaz, estados, métodos HTTP y sintaxis), **restablecer**, **exportar o importar** en JSON y **borrar**.
- Al importar un tema antiguo, los colores que le falten se rellenan con los del tema base, así que añadir colores nuevos no rompe temas existentes.
- Se guarda en `localStorage` del webview, que en Tauri es persistente. 🗓️ Más adelante, en el archivo de ajustes de la app.

## D11b · Ajustes organizados por secciones ✅
- El diálogo de Ajustes tiene **pestañas verticales por concepto**: General (idioma), Apariencia (tema, duplicar, importar y exportar), Colores (editor de colores del tema) e Historial (retención).
- Se navega con el teclado (↑↓, Inicio y Fin), recuerda la última sección abierta y en pantallas estrechas las pestañas pasan arriba.
- Añadir una sección es añadir una entrada a `SECTIONS` en `SettingsDialog.svelte`. **No se mezclan conceptos en una misma sección.**

## D12 · Internacionalización: inglés y castellano, ampliable ✅
- Hecha a mano, sin dependencias: `app/src/lib/i18n/`.
- **Inglés es el idioma fuente** (`locales/en.ts`). Los demás idiomas tienen el tipo `Messages`, así que **TypeScript obliga a traducir todas las claves**.
- Uso: `t("clave")` y `t("clave", { name })`. Es reactivo: la interfaz cambia de idioma al instante.
- Para añadir un idioma: crear `locales/<código>.ts` y registrarlo en `LOCALES`.
- Al arrancar se usa el idioma guardado; si no hay ninguno, el del sistema; si no está soportado, inglés.
- Las frases de CodeMirror (buscador, plegado) también se traducen, mediante `EditorState.phrases`.
- **Regla: ningún texto visible escrito directamente en los componentes.**

## D13 · CLI para terminal y CI ✅ (ampliada por D18)
> **Actualizado:** la regla de fallo ahora es la de Newman. El código de salida es **1** si falla un test, un script o la petición (error de red o variable sin resolver), y **2** si los argumentos son incorrectos. Un estado que no es 2xx **sin tests** ya no hace fallar la ejecución. Los flags completos están en D18.

- `ilixpatch ls <ruta>` y `ilixpatch run <ruta> [--env] [--filter] [--verbose]`. La ruta puede ser una colección o un proyecto.
- Termina con código distinto de 0 si alguna petición falla o devuelve un estado que no es 2xx. Así se puede usar en CI.

## D14 · Licencia y comunidad ✅
- Licencia doble **MIT o Apache-2.0**, la convención en Rust.
- CI en GitHub Actions (fmt, clippy `-D warnings` y tests en Linux, macOS y Windows, más svelte-check).
- Buenos puntos de entrada para contribuidores: importadores y CLI.
- Los cambios grandes se discuten en un issue antes de programarlos.

- **Licencias de terceros:**
  - `THIRD_PARTY_LICENSES` se genera con `scripts/third-party-licenses.mjs`. Incluye las dependencias que van dentro de la app y la CLI (crates de Rust en tiempo de ejecución y paquetes npm de producción) más el código incluido dentro del binario (QuickJS, chai, SQLite y el runtime de Svelte).
  - Los textos idénticos se escriben una sola vez.
  - Se regenera en cada `npm run build`, se copia dentro de la app y se puede ver en **Acerca de → Licencias de terceros**.
  - La CI falla si el archivo no está actualizado.
  - 🗓️ **Pendiente:** incluirlo también en los archivos de release de la CLI y añadir `cargo-deny` para rechazar licencias no permitidas.
- **Autoría:** Jorge Segovia Tormo ([github.com/jst7](https://github.com/jst7)). Aparece en el "Acerca de", en el panel nativo de macOS, en `Cargo.toml` y en los metadatos del instalador.

- **Licencia confirmada (12-11-2026):** se mantiene la **doble licencia MIT o Apache-2.0** (`LICENSE-MIT` y `LICENSE-APACHE`). El `LICENSE` (MIT) creado al abrir el repo en GitHub se conserva tal cual. Es el mismo texto MIT y no cambia nada.

## D15 · Seguridad de las operaciones de archivos ✅
- Los comandos que borran o escriben verifican que la ruta esté **dentro de un proyecto registrado**. Nunca borran la raíz del proyecto ni `.ilixpatch/`.
- "Quitar proyecto" solo lo saca de la lista; **nunca borra archivos**.
- Comprobaciones aplicadas a cada comando:
  - Crear, guardar y borrar solo aceptan rutas dentro de un proyecto registrado, y nunca dentro de `.ilixpatch/`.
  - `save_request` solo escribe archivos `.toml`.
  - `collections_dir` debe ser una ruta relativa y no puede contener `..`.
- Las rutas se guardan **canonicalizadas**. Por ejemplo, en macOS `/var` se guarda como `/private/var`.
- Nombres reservados: no se permite que una petición sobrescriba `collection.toml`, ni crear carpetas llamadas `environments` en la raíz de una colección, ni colecciones llamadas `node_modules` o `target`. Si se intenta, se añade un sufijo `-2`.
- 🗓️ **Mejora pendiente:** ahora un único archivo `.toml` corrupto hace que falle todo el proyecto. Lo correcto es cargar el resto y marcar el archivo con error en el árbol.

## D16 · Cliente de git integrado, al estilo de IntelliJ ✅
> **Estado:**
> - **Backend:** `crates/git`, con `gix` para leer y el `git` del sistema para escribir. El estado se lee con `git status --porcelain=v2`, porque es exacto con renombrados y conflictos.
> - **Pestaña de Git:** cambios agrupados por carpeta, diff semántico de las peticiones más diff por líneas (Myers, propio), commit y "commit y push", descartar cambios, ramas (buscar, crear, checkout y merge), pull con ff-only, merge o rebase, y el historial.
> - **Árbol:** colores de git por archivo. Una carpeta toma el estado más importante de lo que contiene.
Como cada proyecto es un repositorio, integrar git es una ventaja diferencial.
- **Varios repositorios por proyecto:** cualquier directorio de la jerarquía (proyecto, colección o carpeta) puede ser **su propio repositorio, con su propia rama**. Por ejemplo, un proyecto que no es repo puede tener dos colecciones que sí lo son, cada una en su rama.
  - En el árbol, **junto a cada nodo que es raíz de un repo** se muestra un icono de git y el nombre de su rama. También indica si hay cambios sin guardar en git, los commits por delante o por detrás (↑↓) y si hay conflictos.
  - **Al pulsar la rama se abre la pestaña de Git de ese repositorio** (`OpenTab { kind: "git", repo }`). Desde ahí se ven los cambios y los diffs, se hace commit, se cambia o crea rama, y se hacen fetch, pull y push. También está el historial.
  - El estado de cada archivo (modificado, nuevo…) lo da el **repo más cercano** que lo contiene. Un repo anidado no aparece en el estado del repo exterior.
  - "Inicializar git aquí" convierte cualquier carpeta en su propio repo.
- Contrato en `app/src/lib/api.ts` (sección Git), implementado en el crate `crates/git`.
- **Enfoque híbrido:**
  - **Lectura** (estado, diff, log, ramas): con `gix` (gitoxide, Rust puro), porque es rápido y no lanza procesos.
  - **Escritura y red** (commit, checkout, pull, push, merge, rebase): llamando al **`git` del sistema**. Así se respetan las credenciales, las claves SSH, los hooks, la configuración y la firma GPG del usuario. Es lo mismo que hace IntelliJ.
- **Fases:**
  1. Estado en el árbol (modificado, nuevo o sin seguimiento, con colores del tema), rama actual en la barra de estado y botón de refrescar.
  2. Vista de cambios y diff por petición. **Diff semántico**: "URL cambiada" o "header añadido", no solo líneas de TOML.
  3. Commit (seleccionar archivos y escribir el mensaje), descartar cambios, crear y cambiar de rama.
  4. Pull y push con progreso, log o historial por petición, y stash.
  5. Resolución de conflictos con una vista a tres bandas por campos de la petición.
- **Por qué no `git2`/libgit2:** es una dependencia en C, complica la compilación cruzada y no soporta bien SSH ni los helpers de credenciales del usuario.

## D17 · Scripts compatibles con Postman (API `pm.*`) sobre QuickJS ✅
- Cada petición puede tener dos scripts, **pre-request** y **tests**. Se guardan en su `.toml` en `[scripts]` (`pre_request`, `tests`).
- Se ejecutan en **QuickJS** integrado (crate `rquickjs`), en el crate `crates/scripting`. Es ligero, no depende de Node y arranca en microsegundos.
- **Compatibilidad:** la misma API que el sandbox de Postman y Newman, para que los tests existentes funcionen sin cambios:
  - `pm.test` y `pm.expect`. Para las aserciones se incluye **chai** (licencia MIT) dentro del binario.
  - `pm.response`, `pm.request`, `pm.environment`, `pm.collectionVariables`, `pm.globals`, `pm.variables`, `pm.iterationData`, `pm.info` y `postman.setNextRequest`.
  - La sintaxis antigua (`tests[...]`, `responseBody`…).
- **Seguridad:** límite de memoria y de tiempo por script.
- Los cambios de variables se devuelven como una lista de actualizaciones. Es quien llama quien decide aplicarlas: en memoria en el runner, como "valor actual" en la app o en el archivo de `--export-environment` en la CLI.
- 🗓️ **Pendiente:** `pm.sendRequest`, librerías del sandbox (lodash, moment, crypto-js) y scripts a nivel de colección y carpeta.

## D18 · Runner de colecciones compatible con Newman ✅
- Crate `crates/runner`. Toda petición pasa por el mismo flujo, ya sea en la app, la CLI o una prueba de carga: script pre-request → resolver variables → enviar → script de tests.
- **Prioridad de variables:** local > datos de la iteración > entorno > colección > globales. Es la misma que en Postman.
- **Opciones:** iteraciones, fichero de datos CSV o JSON (una iteración por fila), retardo entre peticiones, timeout, parar al primer fallo (*bail*), `setNextRequest` y cancelar.
- **CLI con los mismos flags que Newman:** `-e`, `-n`, `-d`, `--delay-request`, `--timeout-request`, `--bail`, `--folder`, `-r cli,json,junit,html`, `--export-environment`, `--env-var`, `--global-var`, `--insecure` y `--suppress-exit-code`.
- Los informes **JUnit** salen en el formato que esperan los sistemas de CI.
- En la app, el runner se abre como una pestaña y muestra los resultados en vivo mediante un canal de Tauri.

## D19 · Pruebas de carga ✅
- **Usuarios virtuales** como tareas de tokio que comparten un solo cliente HTTP.
- **Perfiles:** fijo, rampa, pico (*spike*) y cumbre (*peak*).
- Latencias medidas con **HdrHistogram**: p50, p90, p95 y p99, más peticiones por segundo y tasa de error. También por petición.
- Se envía un resumen cada segundo a la interfaz, que lo muestra en gráficas SVG dibujadas a mano, sin librería de gráficas.
- **CLI:** `ilixpatch load` con umbrales para CI (`--max-p95` y `--max-error-rate`).

## D20 · Pestañas abiertas en una barra lateral derecha plegable ✅
- Las peticiones abiertas, y también el runner y las pruebas de carga, son **pestañas verticales** en una barra a la derecha.
- Al plegarla queda un riel estrecho que solo muestra los colores de los métodos.
- Modelo genérico: `OpenTab = request | runner | load`.
- Las pestañas abiertas se restauran al arrancar.
- **Atajos:** ⌘W cierra la pestaña, Ctrl+Tab cambia de pestaña y ⌘\ pliega o despliega la barra.

## D21 · Controles propios en lugar de los nativos del sistema ✅
- Los controles importantes, empezando por el **selector de método HTTP**, son componentes propios con los colores del tema (`--method-*`), accesibles por teclado y con semántica ARIA.
- **Por qué:** el `<select>` nativo no admite colores por opción y se ve distinto en cada sistema.
- En cada nodo del árbol, junto a "⋯", hay un botón **"+"** para añadir el nivel inferior:
  - En un proyecto, una colección.
  - En una colección o carpeta, una carpeta o una petición.
  - En la cabecera de la barra lateral, un proyecto.

## D22 · Historial de peticiones (SQLite local) 🗓️ (en curso)
- Se guardan **todas las llamadas con su respuesta**: la petición editada, la petición resuelta, el entorno, la respuesta, los tests y la consola.
- **Dónde:** en una base de datos SQLite (`rusqlite`, compilada dentro del binario) en el directorio de datos de la app. **Nunca en el repositorio**, porque puede contener secretos.
- Crate `crates/history`. El listado no carga los bodies, así que es rápido aunque haya miles de entradas.
- **Al pulsar una entrada se abre siempre una pestaña nueva y fija** (`OpenTab { kind: "history", id }`) con la petición y la respuesta guardadas. Desde ahí se puede editar, reenviar o guardar en una colección.
- **Borrado:** "Limpiar todo" con confirmación, borrar las entradas seleccionadas (selección múltiple) o una sola desde su menú contextual.
- **Retención configurable en Ajustes**, para que no crezca sin límite:
  - Días a conservar: por defecto 30. Hay opciones como "últimos 7 días" o sin límite.
  - Tamaño máximo en MB: por defecto 200.
  - Guardar o no los bodies de respuesta, y tamaño máximo por body: por defecto 1 MB, recortado si lo supera.
  - Se limpia al arrancar, al insertar y al cambiar los ajustes. Se borran primero las entradas más antiguas.

## D23 · Buscar y reemplazar avanzado en los editores ✅
- Panel propio (`app/src/lib/editor/searchPanel.ts`).
- **Atajos:**
  - ⌘F busca.
  - ⌘⌥F abre buscar y reemplazar. En Windows y Linux también Ctrl+H; en macOS no, porque ⌘H oculta la app.
  - Enter va a la siguiente coincidencia y Shift+Enter a la anterior. ⌥Enter inserta un salto de línea en la búsqueda.
- Muestra el contador "n de m" (hasta 10.000) y avisa de las regex no válidas.
- **Rendimiento:** la búsqueda de texto literal se ejecuta internamente como regex escapada, unas 30 veces más rápida en respuestas de varios MB.
- **Limitación:** `.` no encaja con saltos de línea, igual que en JavaScript. Para cruzar líneas se usa `\n`, `\s` o `[\s\S]`.
- Todos los editores tienen un buscador con **expresiones regulares**, **búsqueda multilínea** (el campo de búsqueda admite saltos de línea y la regex puede cruzar líneas), distinción de mayúsculas y palabra completa.
- **Reemplazar** y **Reemplazar todo** en los editores editables, con grupos de captura (`$1`) cuando se usa regex.
- En las respuestas, que son de solo lectura, solo se puede buscar.

## D24 · Resolución de conflictos de merge por interfaz (estilo IntelliJ) 🗓️ (backend ✅, vista de 3 paneles en curso)
- **Vista de tres paneles:**
  - Izquierda, **tu versión** (*ours*), de solo lectura.
  - Centro, el **resultado**, editable.
  - Derecha, la **versión entrante** (*theirs*), de solo lectura.
- **Comparación a tres bandas** (diff3) entre *base*, *ours* y *theirs*, línea a línea y con resaltado por palabra.
  - Los cambios que no chocan se aplican solos al resultado. Los conflictos quedan marcados para resolverlos.
- **Acciones por bloque:** `»` / `«` para aplicar un lado (o los dos, en orden) y `×` para ignorarlo.
  - También "Aplicar cambios sin conflicto", "Aceptar lo mío" y "Aceptar lo suyo" para el archivo entero.
  - Navegación entre conflictos con F7 / Shift+F7, contador de pendientes y scroll sincronizado.
  - El resultado se puede editar a mano y los bloques siguen bien colocados.
- **Archivos binarios**, o borrados en un lado y modificados en el otro: solo se ofrece elegir un lado.
- **Flujo:**
  - Pull con *fast-forward* (por defecto), *merge* o *rebase*, y "Merge en la rama actual" desde el selector de ramas.
  - Mientras hay un merge o rebase en curso, un aviso con los archivos en conflicto y los botones **Continuar/Commit** y **Abortar**.
  - "Aplicar" escribe el resultado y lo marca como resuelto (`git add`).
- En el API, "ours" y "theirs" significan siempre lo que se ve en pantalla, también durante un rebase. Git los intercambia en ese caso y el backend lo corrige.
- Componente reutilizable `DiffView` para comparar dos versiones línea a línea, por ejemplo en los diffs de archivos.
- Contrato en `app/src/lib/api.ts`, en la sección "Git merge & conflict resolution (D24)".

## D25 · Annotate (git blame) en los editores ✅
- Botón **"Annotate"** en el editor de peticiones, como en IntelliJ. Muestra quién cambió cada línea o campo y cuándo.
- **Body y scripts:** una columna en el margen con el autor y la fecha relativa, y un color de fondo según la antigüedad (variables del tema `--blame-*`).
  - Al pasar el ratón se ve el commit completo. Al pulsar se abre la pestaña de Git.
  - Las líneas editadas y todavía sin commit se marcan como tales.
- **URL, headers y params:** una anotación pequeña por campo.
- **Backend:** `git blame --porcelain` sobre el archivo de trabajo, con las líneas sin commit incluidas.
  - `git_blame_request` convierte las líneas del TOML en campos de la petición usando las posiciones de `toml_edit`. Así, la línea 3 del body en el editor corresponde a la línea correcta del archivo.
- Contrato en `app/src/lib/api.ts`, en la sección "Git annotate / blame (D25)".

## D26 · Clonar repositorios desde la interfaz ✅
- **Dónde:** "Clonar repositorio…" en el "+" de la barra lateral y en la pantalla vacía.
- **Opciones:** URL (HTTPS o SSH), carpeta de destino y nombre (se rellena solo a partir de la URL), rama, clonado superficial con profundidad, y "Abrir como proyecto", que crea `.ilixpatch/config.toml` si falta.
- **Cómo se clona:** con el `git` del sistema, para que funcionen las claves SSH y los gestores de credenciales del usuario. Muestra el **progreso en vivo** (fases y porcentaje) y se puede **cancelar**, en cuyo caso se borra la carpeta a medio clonar.
- **Repos privados por HTTPS sin credenciales guardadas:** usuario y token opcionales. Se pasan a git por **variables de entorno** (`GIT_CONFIG_*` → `http.extraHeader`).
  - Nunca aparecen en los argumentos del proceso, en disco, en `.git/config` ni en los mensajes de error.
  - Tampoco se recuerdan en la app.
- **Seguridad:** solo se aceptan URLs `https`, `http`, `ssh`, `git@` y `file`. Se rechazan `ext::` y los argumentos que empiezan por `-`.
- Contrato en `app/src/lib/api.ts`, en la sección "Git clone (D26)".

## D27 · Detalle de commits, revert y reset por interfaz ✅
- En la pestaña **Git → Historial**, al seleccionar un commit se ve su **detalle**:
  - el mensaje completo, el autor y quien hizo el commit con sus emails y fechas, el hash con botón de copiar, los commits padre (se puede navegar a ellos) y las ramas o tags que apuntan a él;
  - la **lista de archivos cambiados**, con estado, +/− líneas y renombrados;
  - el **diff de cada archivo** respecto al primer padre: el resumen de qué campo cambió en las peticiones y el comparador lado a lado (`DiffView`).
- **Acciones:**
  - **Revertir commit** (seguro): crea un commit nuevo que lo deshace. Se puede elegir solo preparar los cambios sin hacer commit. Si hay conflictos, se usa el flujo de merge de D24.
  - **Restablecer la rama aquí** (`reset`) en modo soft, mixed o hard. El modo hard pide una confirmación reforzada, porque borra los cambios sin commit.
  - **Checkout** del commit (HEAD queda desvinculado de cualquier rama), **crear rama desde aquí**, **cherry-pick** y **restaurar un archivo** a esa versión.
- El historial se carga por páginas (`--skip`). Al pulsar en Annotate se abre directamente ese commit.
- **Seguridad:** los ids de commit se validan, y nunca se pasa a git un argumento que empiece por `-`.
- Contrato en `app/src/lib/api.ts`, en la sección "Git commit details & history actions (D27)".

## D28 · Entornos por proyecto y entornos globales ✅
> Sustituye en parte a D08: los entornos pasan de ser **de la colección** a ser **del proyecto**. Los de colección siguen funcionando por compatibilidad ("legacy").
- **Entornos del proyecto:** viven en el repo, en `.ilixpatch/environments/<nombre>.toml`, y se comparten con git.
  - Cada variable tiene clave, valor, **secreto** y **activada**.
  - Las **secretas** se guardan sin valor en el `.toml`, para que el equipo sepa que existen. El valor va en `<nombre>.secret.toml`, que se añade automáticamente a `.ilixpatch/.gitignore`.
  - Se accede con un **icono junto al nombre de cada proyecto** en el árbol, que abre la pestaña "Entornos".
- **Entornos globales y variables globales** (`pm.globals`):
  - Son del usuario y se guardan en el directorio de configuración de la app (permisos 0600). Nunca van a un repo.
  - Se accede con el botón **ENV** junto a "Proyectos".
  - La CLI usa el mismo directorio, con `--global-env` para los entornos globales; las variables globales se cargan siempre salvo con `--no-globals`.
- **Copiar en los dos sentidos:** proyecto → global y global → proyecto. Si el nombre ya existe, se crea "Nombre (2)". Los secretos siguen siendo secretos.
- **Selector de entorno:** agrupa los entornos del proyecto, los globales y los de colección legacy. La selección se recuerda por proyecto.
- **Prioridad de variables:** globales < variables de colección < entorno elegido < variables de ejecución o locales.
- Contrato en `app/src/lib/api.ts`, en la sección "Environments: project & global (D28)".
- **Selector de entorno en la cabecera de la petición**, al lado del nombre (no en la barra de la URL, que así gana ancho). En las pestañas del historial, que tienen otra cabecera, sigue junto a la URL.

## D29 · Red, HTTPS, certificados y proxy (como Postman) ✅
- **CORS no aplica**, igual que en Postman de escritorio: las peticiones las envía el motor en Rust (`reqwest`), no el navegador del webview.
  - **Regla:** nunca enviar peticiones de usuario con `fetch` desde la interfaz.
- **Ajustes globales**, en Ajustes con secciones propias. Se guardan en `network.toml` en el directorio de configuración del usuario (0600, porque puede contener contraseñas). Nunca van al repo.
  - Verificación SSL, que se puede desactivar con aviso visible.
  - Certificados raíz del sistema, CAs adicionales (PEM) y **certificados de cliente (mTLS) por host**, en PEM (cert + clave) o PFX/P12 con contraseña.
  - Proxy: del sistema, ninguno o personalizado (http, https o socks5), con usuario y contraseña y lista de hosts excluidos.
  - Redirecciones (seguirlas y máximo), timeout, TLS mínimo (1.2 o 1.3) y versión HTTP (auto, 1.1 o 2).
- **Por petición:** `[settings]` en el `.toml`, que sobrescribe la verificación SSL, las redirecciones, el timeout y la versión HTTP.
- **En la respuesta:**
  - versión HTTP, IP remota, cadena de redirecciones y proxy usado;
  - **certificado del servidor**: sujeto, emisor, validez con aviso si caduca pronto, SHA-256 y SAN.
- **Motor:** guarda clientes HTTP en caché según la configuración, para conservar el reuso de conexiones. Se reconfigura al guardar los ajustes.
- **CLI con los flags de Newman:** `--ssl-client-cert`, `--ssl-client-key`, `--ssl-client-passphrase`, `--ssl-client-cert-list`, `--ssl-extra-ca-certs`, `--insecure` e `--ignore-redirects`, más `--proxy` y `--no-proxy`. Solo usa `network.toml` con `--use-app-settings`.
- Contrato en `app/src/lib/api.ts`, en la sección "Network, HTTPS & proxy (D29)".

## D31 · Importador de Postman con elección de qué y dónde ✅
- **Formatos de entrada:** colecciones Postman v2.0 y v2.1, entornos, globales, el **ZIP de "Export data"** y carpetas con esos archivos. La v1 se rechaza con un error claro.
- **Asistente en una pestaña** (`OpenTab { kind: "import" }`), en tres pasos:
  1. **Origen:** arrastrar archivos o elegirlos, con ayuda sobre cómo exportar desde Postman.
  2. **Organizar:**
     - A la izquierda, el árbol de Postman con casillas, avisos e iconos de auth y scripts.
     - A la derecha, el proyecto de destino real con los elementos que se van a crear en un estilo distinto. **Se arrastra cada elemento donde se quiera**: como colección nueva, como carpeta dentro de algo que ya existe o como peticiones sueltas. Se puede renombrar, mover y quitar. Los entornos van al proyecto o a global.
     - Debajo, las opciones y un resumen en vivo.
  3. **Informe:** lo creado, con enlaces, y los avisos.
- **Conversión:**
  - Auth heredada de petición, carpeta y colección: Bearer y API key → headers; Basic → header, o script si usa variables. OAuth2 y otros → aviso.
  - Bodies: raw (json o texto), urlencoded, formdata → form con aviso (los archivos no se admiten todavía) y GraphQL → json.
  - Scripts de colección o carpeta → añadidos a cada petición, opcional.
  - Parámetros y headers desactivados → se descartan con aviso.
  - Variables de ruta `:id` → se sustituyen por su valor.
  - **Variables dinámicas** de Postman (`{{$guid}}`, `{{$timestamp}}`, `{{$randomInt}}`…) ya funcionan al enviar.
- **Opciones:** qué hacer si algo ya existe (renombrar, sobrescribir u omitir), convertir auth, conservar scripts y un **patrón para marcar variables secretas** (token, password, api key…), que van al `.secret.toml`.
- Crate `crates/importer`.
- **CLI:** `ilixpatch import postman <archivos> --project <dir>`.
- Contrato en `app/src/lib/api.ts`, en la sección "Postman importer (D31)".

## D36 · Errores descriptivos con causa probable y qué probar ✅
- `sendRequest` falla con un **`RequestError` estructurado**, no con texto: `kind`, `message`, `detail` y datos útiles (`host`, `port`, `url`, `timeout_ms`, `variables` y `proxy`).
- **El backend clasifica** el fallo recorriendo la cadena de errores (reqwest → hyper → io / rustls). Los tipos son:
  - URL no válida, variables sin definir y DNS;
  - conexión rechazada o cortada y timeout;
  - TLS: CA desconocida, certificado caducado, nombre que no coincide, certificado de cliente y error de handshake;
  - proxy, demasiadas redirecciones, body, petición omitida por script y cancelada.
- **La interfaz** muestra para cada tipo, en español e inglés:
  - un **título claro**, la **causa probable** con los datos del caso y una lista de **qué probar**;
  - **botones de acción**: desactivar SSL para esta petición, abrir los ajustes de HTTPS, certificados o proxy, subir el timeout, abrir los entornos…;
  - el detalle técnico en un desplegable con botón de copiar.
- **Runner y CLI:** cada fallo lleva su tipo (`error_kind`). La CLI muestra una línea de pista en inglés.

## D46 · Consola de red: lo que se envía de verdad por debajo ✅
- **Para qué:** depurar lo que no se ve en la pestaña de la petición: cabeceras finales, redirecciones, rondas de autenticación y peticiones de token.
- **Qué registra** cada intercambio HTTP (`engine::netlog::NetExchange`):
  - la **petición tal como sale**: variables resueltas, cabeceras de la autorización y las que añade el cliente HTTP (`Host`, `User-Agent`, `Accept`, `Accept-Encoding`, `Content-Length`), y vista previa del body;
  - la respuesta: estado, cabeceras, vista previa del body (hasta 64 KB), versión HTTP, IP remota, proxy y tiempo de ese intercambio, o el error.
- **Intercambios intermedios**, con su fase: cada salto de **redirección**, las rondas de **Digest** (`digest-response`) y **NTLM** (`ntlm-negotiate` y `ntlm-authenticate`) y las peticiones de **token de OAuth 2.0** (`oauth2-token`).
- **Cómo se captura:** el motor construye la petición (`RequestBuilder::build`) antes de enviarla con `Client::execute` y avisa a un observador (`Engine::with_observer`).
  - Solo se registran los envíos con `SendOptions::trace` (`Trace` = id y etiqueta). Los intercambios del mismo envío comparten el `trace`.
  - Llevan `trace` los envíos de las pestañas (etiqueta = nombre de la petición) y del Runner ("Runner · nombre"). **Las pruebas de carga no se registran**, para no inundar la consola ni gastar CPU. La CLI no tiene observador.
- **Dónde se guarda:** en la app (`netlog_commands`), un buffer circular de **500 entradas solo en memoria**. Nunca se escribe en disco porque contiene secretos. Cada entrada nueva llega a la interfaz con el evento `netlog://entry`.
- **Interfaz:** panel inferior **"Consola de red"** (⌘/Ctrl+Alt+C, o el botón del pie de la barra lateral), redimensionable.
  - Lista cronológica (hora, método, estado, fase, URL, origen y duración) con filtro y "Solo errores". Al lado, el detalle de la petición y la respuesta.
  - **Secretos ocultos por defecto:** `Authorization` (se ve el esquema: "Bearer ••••••"), cookies, `X-Api-Key`… Se muestran con "Mostrar secretos".
  - **"Copiar como cURL"** de cualquier entrada, con los valores reales.
- **Tiempos por intercambio** (`engine::timing`), mostrados como barra apilada en el detalle:
  - **DNS:** resolvedor propio (`dns_resolver`) sobre `getaddrinfo`, como el de reqwest por defecto, que mide la búsqueda.
  - **Conexión (TCP + TLS):** capa alrededor del conector de reqwest (`connector_layer`) que mide cada conexión nueva. Si no se llama, la conexión se **reutilizó** y se indica.
  - **Espera** (hasta el primer byte): de enviar la petición a recibir las cabeceras, sin contar DNS ni conexión. **Descarga:** lectura del body.
  - Las mediciones se asocian a cada envío con un `task_local` de tokio (`timing::timed`), así que funcionan con el cliente compartido y con peticiones en paralelo.
  - **TCP y TLS van juntos:** separarlos obligaría a sustituir el conector de reqwest. Con proxy, la conexión incluye el `CONNECT`.

## D45 · Memoria con muchas pestañas abiertas 🗓️ (aparcado: hoy no es un problema)
- **Estado actual:** solo se monta la pestaña activa (editor, visor, vista previa y vistas de Git o Runner), y los bodies grandes están en disco (D41). Por pestaña queda en memoria la última respuesta: cabeceras, tests, consola y el body si cabe en `inline_body_kb`. Ese body pequeño está duplicado en la interfaz y en el `ResponseStore` de Rust (máximo 64 entradas).
- **Peor caso estimado:** unas 100 pestañas con respuestas de ~1 MB suman unos 150–250 MB. Con respuestas normales de API son unos pocos MB.
- **Mejoras previstas, si llega a ser un problema:**
  1. no guardar en la interfaz el body de las pestañas inactivas que ya está en Rust, y releerlo al volver;
  2. mantener completas solo las últimas N respuestas (por ejemplo 20); del resto, guardar estado, tamaño y cabeceras, con "Reenviar";
  3. limitar la consola de scripts por respuesta (por ejemplo, las últimas 1.000 líneas).

## D44 · Vista previa de imágenes, PDF, audio, vídeo y HTML en la respuesta ✅ (v0.3.3)
- **Qué se previsualiza** (`lib/preview/kind.ts`):
  - imágenes PNG, JPEG, GIF, WebP, AVIF, BMP, ICO y SVG; PDF; audio MP3, WAV, OGG/Opus, AAC/M4A y FLAC; vídeo MP4, WebM, OGV y MOV; y HTML.
- **Cómo se detecta:** por `Content-Type`. Si es genérico (`application/octet-stream` o falta), por los primeros bytes del archivo (PNG, JPEG, `%PDF-`, `ftyp`, `OggS`…).
- **Selector "Vista previa | Binario"** (o "| Código" si el body es texto) en la pestaña Body. Se mantiene la vista hex o de texto.
  - Por defecto se abren renderizados los medios. El HTML se abre como código, que es lo que suele querer un cliente de APIs.
- **Cómo se muestra** (`MediaPreview`): el body se lee del `ResponseStore` en trozos de 4 MB y se pasa a una URL `blob:`, que se libera al cambiar de respuesta.
  - Imagen sobre fondo de cuadros, con "Ajustar / Tamaño real" y las dimensiones. PDF en `iframe` con el visor del webview. Audio y vídeo con los controles nativos.
  - **HTML en un `iframe` con `sandbox` vacío:** sin scripts, formularios ni acceso al origen de la app.
  - Hasta **150 MB**. Por encima no se carga en el webview.
- **"Abrir con la app del sistema"** (comando `response_open`): copia el body a `<app_cache_dir>/bodies/ilixpatch-body-open-<id>.<ext>` y lo abre con `tauri-plugin-opener`.
  - Sirve cuando el webview no sabe mostrarlo (por ejemplo, PDF en WebKitGTK de Linux) o el archivo es muy grande.
  - El archivo se borra en el siguiente arranque, porque la otra app puede seguir usándolo.
- Si el body se cortó en `max_body_mb`, se avisa de que la vista previa puede salir rota.
- 🗓️ **Más adelante:** CSV como tabla, Markdown renderizado, fuentes y ZIP (listar el contenido).

## D43 · Barra de URL coloreada y edición de variables al pasar el ratón ✅ (v0.3.2)
- **Colores con significado:** la URL se divide en partes (`lib/url/tokens.ts`) y cada una lleva un color del tema (`--syntax-*`):
  - esquema atenuado, host, puerto, `/` y separadores, `:parámetros` de ruta, claves y valores del query, y `#fragmento`;
  - **variables:** definida en color de acento, **sin definir en rojo con subrayado ondulado** y dinámica (`{{$guid}}`…) en otro color.
- **Cómo se pinta:** detrás del `textarea` hay una copia coloreada con la misma fuente, padding y salto de línea, y el `textarea` tiene el texto transparente. La edición, el pegado de cURL y Enter funcionan igual que antes. Las variables no llevan padding ni borde para que el texto no se descuadre.
- **Al pasar el ratón por una variable** (350 ms) se abre una ventana (`VarPopover`) con:
  - de dónde sale el valor (Entorno, Colección o Globales), si es un valor de sesión fijado por un script, y en qué otros ámbitos está definida pero no se usa;
  - un campo para **cambiar el valor** y un selector **"Guardar en"** para elegir el ámbito. Si se cambia de ámbito, por defecto **se mueve** (se quita del ámbito anterior). Se puede desmarcar para copiarla.
  - Para una variable sin definir, sirve para crearla.
- **Guardado** (`lib/vars/edit.ts`): escribe en el archivo del ámbito (globales, `collection.toml` o el entorno de proyecto o global), conserva `secret` y el resto de variables, borra el valor de sesión de los ámbitos tocados para que se aplique el guardado, y recarga el proyecto o los globales.
  - Los entornos antiguos de colección no se editan desde aquí.
- La ventana se mantiene abierta mientras tiene el foco y se cierra con Esc, Cancelar, Guardar o un clic fuera.

## D42 · Actualizaciones de dependencias (Dependabot) ✅ (v0.3.1 y v0.3.5; TypeScript 7 en espera)
- **Grupos de Dependabot** (`.github/dependabot.yml`), para que lo que debe ir junto llegue en un solo PR:
  - **RustCrypto** (sha2, pkcs8, rsa, p256…) siempre juntas, también en versiones mayores, porque comparten traits y subir solo una duplica dependencias;
  - **Tauri** (crates y paquetes `@tauri-apps/*`);
  - **Svelte/SvelteKit/Vite**, **CodeMirror** y las acciones de GitHub.
- **Primeros PRs de Dependabot:** no se fusionan tal cual, porque están hechos sobre un `main` antiguo y chocarían con Authorization. Se aplican a mano por grupos, con la verificación completa en cada uno:
  1. ✅ v0.3.1: `base64` 0.23, `miniz_oxide` 0.9 y `toml` 1.x. Los tests de ida y vuelta comprueban que el formato de los archivos no cambia.
  2. ✅ v0.3.5: **SvelteKit 3 y adapter-static 4.**
     - La configuración pasa de `svelte.config.js` (que se elimina) a `sveltekit({ preprocess, adapter, alias })` en `vite.config.js`.
     - SvelteKit 3 sustituye `$lib` por `#lib`. Se mantiene `$lib` con `alias: { $lib: "src/lib" }`, la vía que ofrece SvelteKit, en vez de reescribir 304 imports en 83 archivos.
  3. ✅ v0.3.5: **familia RustCrypto entera**: sha1/sha2/md-5/md4 0.11, hmac 0.13, rand_core 0.10, p256/p384/p521 0.14, pkcs8 0.11 y **rsa 0.10**.
     - El PR de Dependabot no compilaba porque no subía `rsa`, y mezclaba `digest` 0.10 y 0.11.
     - Cambios: `rsa` necesita la feature `pkcs1v15`; `RsaPrivateKey` pasa a `PrivateKey`; `pss::BlindedSigningKey` pasa a `pss::SigningKey` (ahora siempre aplica blinding); `Mac::new_from_slice` pasa a `KeyInit::new_from_slice`; el RNG propio implementa `TryRng<Error = Infallible>` y `TryCryptoRng` (rand_core 0.10); para claves PKCS#8 cifradas se usa `pkcs8::EncryptedPrivateKeyInfoRef`.
     - Lo validan los tests con vectores conocidos: JWT (RFC 7515), NTLM (MS-NLMP), OAuth1 (RFC 5849), PKCE (RFC 7636), firmas RS/PS/ES y claves PEM/PKCS#8 cifradas.
     - Solo `tauri-codegen` (macro de compilación) sigue usando `sha2` 0.10.
  4. 🗓️ **TypeScript 7:** en espera hasta que SvelteKit y svelte-check lo soporten oficialmente. Hoy exige tener instalados TS 7 y TS 6 a la vez y activar el modo experimental `--tsgo`.

## D41 · Respuestas grandes: streaming, archivos temporales y lectura por fragmentos ✅ (v0.3.0)
> Completa D09 ("los datos grandes se quedan en Rust"), que estaba pendiente: hasta ahora el body entero se leía a un `Vec<u8>` y viajaba completo a la interfaz.
- **Streaming real** con `reqwest::Response::bytes_stream()`. Nunca se carga el body entero en memoria por defecto.
  - **Memoria hasta un umbral** (por defecto ~1 MB). Si se supera, se **vuelca a un archivo temporal** (`tempfile`, en la caché de la app) mientras se va recibiendo.
  - El tamaño se calcula incrementalmente. Hay un **límite máximo** configurable: al pasarlo se para la descarga y queda la respuesta parcial con un aviso.
- **`ResponseStore` en Rust** con un `body_id` por respuesta, en memoria o en archivo. Se libera al cerrar la pestaña o enviar de nuevo, con LRU por tamaño total, y **borra los temporales** al salir (y al arrancar, los que hayan quedado huérfanos).
- **La interfaz pide fragmentos bajo demanda:**
  - `send_request` devuelve los metadatos más una **vista previa** (los primeros KB) y el `body_id`.
  - Comandos `response_read(body_id, offset, len)` por bytes y `response_lines(body_id, start, count)` por líneas, con un **índice de líneas** construido en Rust.
  - **JSON grande:** se formatea **en streaming en Rust** a otro temporal, sin parsear todo en memoria, para poder verlo por fragmentos.
  - **Buscar** en el body se hace en Rust (`response_search`, con regex y offsets) en lugar de en el editor.
  - **Visor virtualizado** para bodies por encima del umbral. Por debajo se sigue usando CodeMirror.
  - **"Guardar respuesta en archivo…"** copia desde el temporal sin pasar por la interfaz.
- **Cancelación efectiva:** el bucle del stream comprueba el `CancellationToken`. Al cancelar se **suelta el stream**, con lo que se cierra la conexión, y **se borra el temporal parcial** en ese momento. Lo mismo al cerrar la pestaña o la app.
- **Scripts y tests:** `pm.response.text()` y `json()` cargan el body solo si hace falta, hasta un límite. Por encima, el script recibe un aviso y el body truncado.
- **Runner y pruebas de carga:** por defecto **descartan el body** (solo cuentan bytes), salvo que haya scripts de test que lo necesiten. Así una prueba de carga no acumula memoria.
- **Historial:** guarda como máximo `max_body_kb` (D22), leído del temporal.
- **Detalles de implementación (app):**
  - Los temporales van a `<app_cache_dir>/bodies` (prefijo `ilixpatch-body-`). Se borran los huérfanos al arrancar y todo el almacén en `RunEvent::Exit`.
  - `ResponseStore` (crate `bodystore`) con LRU: **4 GB** de archivos o **64** bodies como máximo. El body recién añadido nunca se expulsa.
  - `SendResult.body` lleva el body entero si cabe en `inline_body_kb`; si no, una **vista previa de 64 KB** cortada en un límite UTF-8, y `body_handle.preview_truncated = true`.
  - El historial lee el body fuera del runtime async (`spawn_blocking`) y solo hasta `max_body_kb`, nunca el temporal entero.
  - Tests de extremo a extremo en `crates/engine/src/large_body_tests.rs`: 50 MB a temporal, corte en `max_body_mb`, descarte y cancelación a mitad (cierra la conexión y no deja temporal).

## D40 · Releases sin firma, y firma opcional ✅
- **No hacen falta claves de desarrollador** para publicar. Sin secretos, la release genera apps **sin firmar**:
  - **macOS:** firma ad-hoc (`bundle.macOS.signingIdentity = "-"`). Así Gatekeeper muestra "desarrollador no identificado" (se abre con clic derecho → Abrir) en lugar de "está dañada".
  - **Windows:** SmartScreen pide "Más información → Ejecutar de todas formas".
- **Error que se corrigió:** un secreto que no existe llega a GitHub Actions como **variable vacía**. Tauri intentaba importar un certificado vacío o notarizar sin Apple ID, y fallaba. Ahora un paso previo exporta las variables `APPLE_*` **solo si el secreto tiene valor**: certificado e identidad para firmar, y además Apple ID, contraseña y Team ID para notarizar.
- **Windows y `result_large_err`:** en Windows un `PathBuf` ocupa 32 bytes, así que los errores con varias rutas pueden pasar el umbral de 128 bytes de clippy solo ahí. Regla: **meter en `Box` los errores grandes** (toml, serde, reqwest) dentro de los enums de error.
- Windows vuelve a estar en `release.yml`, tanto el instalador `.msi`/`.exe` como `ilixpatch.exe`.
- **Release con el CI como requisito y publicación automática** (desde v0.3.7):
  - `ci.yml` es reutilizable (`workflow_call`) y `release.yml` lo ejecuta primero. Si algún test falla, no se compila nada.
  - Cuando todas las builds de la app y de la CLI salen bien, el job `publish` publica el borrador (`gh release edit --draft=false --latest`). Si alguna falla, queda en borrador para revisarlo.
  - Motivo: las releases se quedaban en borrador, así que el badge "Release" del README seguía mostrando una versión antigua, y un tag podía generar una release con tests rotos.

## D39 · Authorization con todos los métodos de Postman ✅
- **Tipos soportados:**
  - Inherit (por defecto) y No Auth;
  - API Key (header o query), Bearer, JWT Bearer (HS, RS, PS y ES), Basic;
  - **Digest** (MD5 y SHA-256, también `-sess`, con qop): con reto y respuesta, o directo si se conocen realm y nonce;
  - OAuth 1.0 (HMAC-SHA1/256/512, RSA-SHA y PLAINTEXT, en header, query o body);
  - **OAuth 2.0**: Authorization Code con y sin PKCE, Implicit, Password y Client Credentials;
  - Hawk, **AWS SigV4** (en headers o URL prefirmada), **NTLMv2**, Akamai EdgeGrid y ASAP.
- **Modelo:** `ilixpatch_core::auth::Auth`, una enum con el campo `type`. Se guarda en `[auth]` del `.toml` de la petición, en `collection.toml` y en un `folder.toml` opcional por carpeta.
  - **Herencia:** petición → carpetas → colección, como en Postman. `no_auth` corta la herencia.
  - Todos los campos admiten `{{variables}}`. Para los secretos se recomiendan las variables secretas del entorno.
- **Crate `crates/auth`:** funciones puras y deterministas (reloj y aleatoriedad inyectables), probadas con los vectores publicados en los RFC y en la documentación de AWS, Hawk y MS-NLMP.
  - El runner aplica la firma justo antes de enviar.
  - El motor gestiona los retos de Digest y NTLM. NTLM va ligado a la conexión, así que se usa la misma conexión en todo el intercambio.
- **OAuth 2.0:**
  - El navegador del sistema se abre con **retorno a un servidor local en 127.0.0.1** y PKCE S256.
  - Los tokens se guardan **solo en el equipo del usuario** (`oauth2-tokens.json`, permisos 0600), nunca en el repo.
  - Renovación automática con refresh token, y petición automática del token en los flujos Client Credentials y Password (también desde la CLI).
- **Interfaz:**
  - pestaña **Authorization** en la petición, con un formulario por tipo, ayuda y aviso si un secreto se escribe tal cual en un proyecto con git;
  - pestaña de **ajustes de colección o carpeta** (Authorization y Variables);
  - gestión de tokens OAuth2 ("Get New Access Token", refrescar, borrar).
- **Firmas verificadas** con los vectores publicados:
  - RFC 7617 (Basic), RFC 2617 y RFC 7616 (Digest), RFC 5849 (OAuth 1.0) y RFC 7636 (PKCE);
  - MS-NLMP §4.2.4 (NTLMv2), la suite oficial de AWS SigV4, el README de Hawk, los 13 casos de edgegrid-python (Akamai) y jwt.io.
  - Criptografía en **Rust puro (RustCrypto)**, porque `ring` y `jsonwebtoken` no firman ES512.
- **Peticiones de token de OAuth2** usan el mismo motor que los envíos (`Engine::client_for_url`): proxy, CAs y certificado de cliente del host, tanto en la app (leyendo los ajustes en el momento) como en la CLI (con sus flags).
- **Aleatoriedad:** nonces, salts, PKCE, `state` y *blinding* RSA salen siempre del generador aleatorio del sistema. Si este no está disponible, la app **falla de forma visible** en vez de usar valores predecibles.
- **Importador de Postman y cURL:** la autenticación se lleva a este modelo, en lugar de convertirla en headers.
  - cURL: `-u` → Basic, `--digest`, `--ntlm`, `--aws-sigv4` y `--oauth2-bearer`.

## D38 · Importar y exportar cURL desde la barra de URL ✅
- **Pegar un cURL** en la barra de URL (o escribirlo y pulsar Enter) **rellena la petición entera**, como en Postman: método, URL, headers, body y ajustes. El nombre y los scripts se conservan.
  - **Compatible con** "Copy as cURL" de Chrome y Firefox (bash `\`, cmd de Windows `^` y PowerShell `` ` ``, comillas `'…'`, `"…"` y `$'…'`) y con lo que exporta Postman.
  - **Flags:** `-X`, `-H`, `-d/--data*`, `--data-urlencode`, `--json`, `-F` (sin archivos), `-u` (→ Basic), `-A`, `-e`, `-b`, `-G`, `-I`, `-k` (→ sin SSL), `-L`, `--max-redirs`, `-m` (→ timeout), `--http1.1`/`--http2` y `--url`. Los de salida o depuración (`-s`, `-v`, `-o`, `--compressed`…) se ignoran sin avisar.
  - Proxy, certificados y archivos generan un **aviso** visible bajo la barra, con lo que no se importó.
  - El body JSON se detecta y se formatea. Los formularios se importan como texto editable con su `Content-Type`.
- **Botón "cURL"** en la fila de pestañas de la petición: copiar como cURL **con las variables resueltas** del entorno activo, **con `{{variables}}`** o **en una sola línea**. Los ajustes por petición se convierten en flags (`--insecure`, `--location`, `--max-time`…).
- Un solo módulo, `app/src/lib/curl.ts`, que también usa el historial ("Copiar como cURL"). Tiene autotest propio (`node src/lib/curl.selftest.mjs`) incluido en la CI.

## D37 · Añadir proyectos solo desde el botón "+" ✅
- "Abrir carpeta", "Clonar repositorio" e "Importar desde Postman" están **solo en el menú del "+"** de la cabecera de la barra lateral. Se quitan los botones sueltos que sobrecargaban la interfaz.
- El "+" usa el color de acento para que se encuentre fácilmente. La pantalla vacía ya no tiene botones: solo explica que se use el "+".

## D35 · Campo de URL multilínea ✅
- La URL se edita en un `textarea` que **parte las URLs largas en varias líneas** y crece con el contenido hasta unas 6 líneas. A partir de ahí aparece scroll. También se reajusta al cambiar el ancho de la ventana.
- Como una URL no puede contener saltos de línea:
  - **Enter envía** la petición (⌘/Ctrl+Enter también);
  - al pegar texto con saltos de línea, **se quitan** junto con los espacios de alrededor, y el cursor se queda en su sitio. Por ejemplo, una URL pegada en tres líneas queda en una sola.

## D34 · Cancelar una petición colgada ✅
- Cada envío lleva un `requestId`. `cancel_request(requestId)` activa un `CancellationToken` y `send_request` compite contra él con `tokio::select!`.
  - Al ganar la cancelación, **se descarta el futuro de la petición y la conexión HTTP se cierra en el momento**, sin esperar al timeout ni cerrar la app.
  - Un test con un servidor que nunca responde comprueba que la cancelación es inmediata y que el socket se cierra.
- **En la interfaz:** mientras se envía, "Enviar" pasa a ser **"Cancelar"**, y el panel de respuesta muestra un **cronómetro** con su propio botón Cancelar. El atajo es **⌘. / Ctrl+.**, el habitual para cancelar en macOS.
- Una petición cancelada **no borra la respuesta anterior**: se mantiene en pantalla con el aviso "Petición cancelada". No se guarda en el historial.
- El runner y las pruebas de carga ya tenían cancelación propia (D18 y D19).

## D33 · Buscadores en el árbol, las pestañas y el historial ✅
> **Atajos:** ⌘/Ctrl+P (o ⌘/Ctrl+Shift+F fuera del editor) para buscar en la barra lateral visible (árbol o historial) y ⌘/Ctrl+E para las pestañas. Filtros `method:post` e `in:<colección>`. Con 20.000 elementos, una búsqueda de 3 palabras tarda unos 3,5 ms.
- **Un componente reutilizable `SearchBox`** con icono, botón de borrar, contador de resultados y pista del atajo. Se usa en los tres sitios.
- **Árbol de proyectos:**
  - filtra en vivo por nombre de petición, método, URL y nombres de carpeta, colección y proyecto;
  - no distingue mayúsculas ni acentos, y varias palabras se combinan con AND;
  - abre automáticamente las carpetas con coincidencias sin alterar las que el usuario tiene abiertas, y resalta lo encontrado;
  - opción "buscar en el contenido" (headers, query, body y scripts).
- **Pestañas abiertas:** filtra por nombre, URL, método, ubicación y tipo de pestaña. En el riel plegado aparece un icono de búsqueda.
- **Historial:** ya tenía búsqueda en el servidor (URL, nombre y método). Ahora usa el mismo componente y el mismo estilo.
- **Rendimiento:** se precalcula un índice normalizado al cargar los proyectos, no en cada pulsación. Así va fluido con miles de peticiones.
- **Teclado:** atajo para enfocar cada buscador, Esc para limpiar, flechas para moverse por los resultados y Enter para abrir.

## D32 · Icono: un pato con botas de agua ✅
- **Diseño original** en SVG: un patito amarillo de perfil con pico naranja y **botas de agua rojas**, sobre una baldosa redondeada con el verde de la marca (D11).
- **Fuente única:** `app/src-tauri/icons/icon.svg`. Todos los tamaños se generan con `cd app && npx tauri icon src-tauri/icons/icon.svg`: `.icns` de macOS, `.ico` y logos de la Store de Windows, y PNG de Linux.
  - Las carpetas de Android e iOS se eliminan porque no se usan.
- También es el **favicon** y el logo del "Acerca de" (`app/static/icon.svg`).

## D30 · Nombre del proyecto: **ilixPatch** ✅ (renombrado aplicado el 11-11-2026)
- **Nombre elegido:** **ilixPatch**. Comprobado el 11-11-2026: libre en crates.io y npm, sin repos en GitHub con ese nombre, y `ilixpatch.com/.dev/.io` sin DNS (confirmar en un registrador).
- **Convenciones:**

| Dónde | Forma |
|---|---|
| Nombre visible | `ilixPatch` |
| Crates, paquetes y CLI | `ilixpatch`, `ilixpatch-core`, `ilixpatch_core` |
| Carpeta del proyecto | `.ilixpatch/` |
| Identificador de la app | `dev.ilixpatch.app` |
| Variables de entorno | `ILIXPATCH_*` |
| Claves de localStorage | `ilixpatch.*` |
| Repositorio | `github.com/jst7/ilixPatch` |

- **Renombrado aplicado en una sola pasada** en 88 archivos, más la carpeta `.ilixpatch/` del ejemplo. Verificado con 154 tests, clippy, `npm run check`/`build` y la CLI. El binario de la CLI se llama `ilixpatch`. La carpeta local del repo puede seguir llamándose `patchman/` hasta que se publique en `github.com/jst7/ilixPatch`.
- Motivo original, que se conserva como contexto:

### Motivo (análisis previo)
- **Riesgos del nombre "Patchman"**, revisados el 11-11-2026. No es asesoramiento legal.
  - Se parece a **Postman**, que es marca registrada, y es la misma categoría de producto.
  - **Ya existe** `praneet-suresh/Patchman` (148 ★), un cliente de APIs para macOS descrito como "Decluttered Postman".
  - Hay otros usos del nombre: `furlongm/patchman` (monitorización de parches para Linux, 458 ★) y la web patchman.co.
  - Libre: `patchman` en crates.io y en npm.
- **Recomendación:** renombrar **antes del primer commit o release**, cuando el coste es mínimo.
- **El nombre nuevo debe:** no contener "post" ni terminar en "-man", estar libre en GitHub, crates.io, npm y un dominio, y no aparecer en EUIPO ni USPTO.
- **Qué hay que cambiar al renombrar:**
  - nombres de crates y del paquete npm, e identificador de la app `dev.patchman.app`;
  - **carpeta `.patchman/` de los proyectos** (parte del formato);
  - textos, About, README y el script de licencias.

---

## Cómo añadir una decisión
Copia este formato al final del documento:
```
## DXX · Título <estado>
Qué se decide.
- **Por qué:** …
- **Alternativas:** …
```
