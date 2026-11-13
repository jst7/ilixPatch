# CLI `ilixpatch`

`ilixpatch` es la herramienta de línea de comandos de ilixPatch. Ejecuta las mismas colecciones que la app de escritorio,
sin interfaz gráfica, y está pensada para la terminal y para CI. Sus flags son **compatibles con Newman**, así que un
pipeline que usa `newman run` se puede migrar cambiando poco más que el nombre del comando.

```text
ilixpatch <COMANDO> [OPCIONES]

Comandos:
  run     Ejecuta un proyecto, colección, carpeta o petición (flags compatibles con Newman)
  load    Prueba de carga con usuarios virtuales
  import  Importa desde otras herramientas (Postman, OpenAPI / Swagger)
  ls      Lista las peticiones y entornos de una colección o proyecto
```

`ilixpatch --help`, `ilixpatch <comando> --help` y `ilixpatch --version` muestran la ayuda y la versión.

## Índice
- [Instalación](#instalación)
- [Qué se puede ejecutar](#qué-se-puede-ejecutar)
- [`ilixpatch run`](#ilixpatch-run)
- [`ilixpatch load`](#ilixpatch-load)
- [`ilixpatch import postman`](#ilixpatch-import-postman)
- [`ilixpatch import openapi`](#ilixpatch-import-openapi)
- [`ilixpatch ls`](#ilixpatch-ls)
- [Opciones de red (TLS, mTLS, proxy)](#opciones-de-red-tls-mtls-proxy)
- [Entornos y variables](#entornos-y-variables)
- [Scripts (`pm.*`)](#scripts-pm)
- [Autorización](#autorización)
- [Diagnóstico de errores](#diagnóstico-de-errores)
- [Códigos de salida](#códigos-de-salida)
- [Variables de entorno del sistema](#variables-de-entorno-del-sistema)
- [Ejemplo en CI](#ejemplo-en-ci)

## Instalación
- **Binarios:** en la página de [Releases](https://github.com/jst7/ilixPatch/releases) hay un binario de la CLI para cada plataforma.
- **Desde el código:**
  ```sh
  cargo build --release -p ilixpatch-cli      # → target/release/ilixpatch
  cargo run -p ilixpatch-cli -- run examples/demo-project --env dev
  ```
- `./init.sh --cli` ejecuta la CLI contra el proyecto de ejemplo.

## Qué se puede ejecutar
El argumento `<path>` de `run`, `load` y `ls` puede ser:

| Ruta | Qué se ejecuta |
|---|---|
| Un **proyecto** (directorio con `.ilixpatch/config.toml`) | Todas sus colecciones |
| Una **colección** (directorio con `collection.toml`) | Todas sus peticiones |
| Una **carpeta** dentro de una colección | Las peticiones de esa carpeta y sus subcarpetas |
| Un fichero de **petición** `.toml` | Solo esa petición |

Las peticiones se ejecutan en el orden del árbol, igual que en el Runner de la app.

## `ilixpatch run`
Ejecuta las peticiones con sus scripts de pre-request y de test, y genera informes.

```sh
ilixpatch run examples/demo-project --env dev

ilixpatch run examples/demo-project/jsonplaceholder/users \
  -e dev -d examples/demo-project/data/users.csv \
  -r cli,junit,html \
  --reporter-junit-export results/junit.xml \
  --reporter-html-export results/report.html
```

### Selección
| Flag | Descripción |
|---|---|
| `--folder <nombre>` | Ejecuta solo las peticiones de esa carpeta (o la petición con ese nombre). Se puede repetir |
| `-f, --filter <texto>` | Ejecuta solo las peticiones cuya ruta contiene ese texto (en un proyecto la ruta incluye la colección: `colección/carpeta/petición`) |

Si ningún filtro coincide, la CLI termina con `no requests matched` (código 2).

### Entorno y variables
| Flag | Descripción |
|---|---|
| `-e, --env <nombre\|fichero>` | Entorno del proyecto (`.ilixpatch/environments/`); si no existe, entorno *legacy* de la colección (`<colección>/environments/`); si tampoco, ruta a un `.toml` o a un entorno de Postman `.json` |
| `--global-env <nombre>` | Usa un entorno **global** de la app de escritorio. Incompatible con `--env` |
| `--env-var clave=valor` | Define o sobrescribe una variable del entorno. Se puede repetir |
| `--global-var clave=valor` | Define una variable global (`pm.globals`). Se puede repetir |
| `--no-globals` | No carga el `globals.toml` de la app como `pm.globals` |
| `--export-environment <ruta>` | Al terminar, escribe el entorno final (con los cambios de los scripts) en formato JSON de Postman |

### Iteraciones y datos
| Flag | Descripción |
|---|---|
| `-n, --iteration-count <n>` | Número de iteraciones. Por defecto, el número de filas del fichero de datos, o 1 |
| `-d, --iteration-data <fichero>` | CSV (con cabecera) o JSON (array de objetos). Una iteración por fila; cada columna está disponible como `{{columna}}` y en `pm.iterationData` |

Si hay más iteraciones que filas, las filas se reutilizan como en Newman.

### Control de la ejecución
| Flag | Descripción |
|---|---|
| `--delay-request <ms>` | Pausa entre peticiones (por defecto 0) |
| `--timeout-request <ms>` | Timeout por petición; `0` = sin límite (por defecto) |
| `--bail` | Para en la primera petición o test que falle |
| `--suppress-exit-code` | Termina siempre con código 0, aunque haya fallos |
| `-v, --verbose` | Muestra las cabeceras y el cuerpo de cada respuesta |

`Ctrl-C` cancela la ejecución de forma ordenada (se imprime el resumen y los informes); un segundo `Ctrl-C` aborta al momento.

### Informes
| Flag | Descripción |
|---|---|
| `-r, --reporters cli,json,junit,html` | Reporters a usar, separados por comas. Por defecto `cli` |
| `--reporter-json-export <ruta>` | Fichero del informe JSON |
| `--reporter-junit-export <ruta>` | Fichero del informe JUnit XML (para GitHub Actions, GitLab, Jenkins…) |
| `--reporter-html-export <ruta>` | Fichero del informe HTML (incluye los cuerpos de las respuestas) |

- Indicar una ruta `--reporter-X-export` activa ese reporter aunque no esté en `-r`.
- Si se activa un reporter sin ruta, el fichero se escribe en `ilixpatch/run-<timestamp>.<ext>`.
- Los directorios de destino se crean si no existen.

El reporter `cli` muestra en vivo cada petición (método, URL, estado, tamaño, tiempo), los `console.log` de los
scripts y el resultado de cada test. Al final imprime un resumen al estilo de Newman:

```text
┌────────────────────┬──────────┬────────┐
│                    │ executed │ failed │
├────────────────────┼──────────┼────────┤
│ iterations         │        3 │      0 │
│ requests           │       12 │      1 │
│ test-scripts       │       12 │      0 │
│ prerequest-scripts │        4 │      0 │
│ assertions         │       30 │      2 │
├────────────────────┴──────────┴────────┤
│ total run duration: 1.8s               │
│ total data received: 24.3kB            │
│ average response time: 142ms [...]     │
└────────────────────────────────────────┘
```

seguido de una tabla con cada fallo numerado (tipo, petición/test y detalle).

Los colores se desactivan si la salida no es una terminal o si está definida `NO_COLOR`.

Además acepta todas las [opciones de red](#opciones-de-red-tls-mtls-proxy).

## `ilixpatch load`
Prueba de carga: N usuarios virtuales (VUs) recorren en bucle las peticiones del objetivo durante un tiempo.

```sh
ilixpatch load examples/demo-project/jsonplaceholder --vus 5 --duration 30s \
  --profile ramp-up --ramp-up 10s --think-time 100ms \
  --max-p95 500 --max-error-rate 0.01
```

| Flag | Descripción |
|---|---|
| `--vus <n>` | Número de usuarios virtuales (por defecto 1) |
| `--duration <t>` | Duración de la prueba (por defecto `10s`) |
| `--profile <perfil>` | `fixed` (por defecto), `ramp-up`, `spike` o `peak` |
| `--ramp-up <t>` | Tiempo hasta alcanzar todos los VUs en `ramp-up` / `peak` (por defecto, el 25 % de la duración) |
| `--think-time <t>` | Pausa de cada VU entre peticiones (por defecto `0ms`) |
| `--timeout <t>` | Timeout por petición (por defecto `30s`; `0` = sin límite) |
| `--no-scripts` | No ejecuta scripts de pre-request ni de test (máximo rendimiento) |
| `--max-p95 <ms>` | Umbral de CI: falla (código 1) si la latencia p95 global lo supera |
| `--max-error-rate <0..1>` | Umbral de CI: falla (código 1) si la tasa de errores lo supera |
| `-e, --env`, `--global-env`, `--env-var`, `--global-var`, `--no-globals`, `-k` | Igual que en `run` |

Las duraciones aceptan `500ms`, `30s`, `1.5s`, `2m`, `1h` o un número de segundos.

**Perfiles de carga:**
- `fixed`: todos los VUs desde el principio.
- `ramp-up`: sube linealmente hasta `--vus` y se mantiene.
- `spike`: 20 % de los VUs, y el 100 % durante el 20 % central de la prueba.
- `peak`: sube, se mantiene y baja.

**Qué cuenta como error:** un error de red, un estado fuera de 2xx/3xx o un test que falla.

**Salida:** una línea por segundo con el tiempo, VUs activos, peticiones, req/s, latencia media, p95 y errores;
al final, una tabla por petición (count, errors, avg, p95) y la latencia global p50/p90/p95/p99/max, el throughput y
la tasa de errores. Si se indicaron umbrales, se muestra ✓/✗ para cada uno. Si no se completó ninguna petición, la
prueba falla.

`load` también acepta las [opciones de red](#opciones-de-red-tls-mtls-proxy). Por favor, sé prudente con las APIs públicas.

## `ilixpatch import postman`
Importa trabajo de Postman a un proyecto de ilixPatch.

```sh
ilixpatch import postman my-collection.postman_collection.json dev.postman_environment.json \
  --project ./my-project [--into ./my-project/api] [--env-scope project|global] [--on-conflict rename|overwrite|skip]
```

| Argumento / flag | Descripción |
|---|---|
| `<paths…>` | Uno o varios ficheros `.json` (colecciones, entornos, globals), `.zip` de *Export data* o carpetas |
| `--project <dir>` | Proyecto de destino (obligatorio). Si aún no es un proyecto, se inicializa |
| `--into <dir>` | Importa cada colección como carpeta dentro de esta colección o carpeta, en lugar de como colección nueva |
| `--env-scope project\|global` | Dónde van los entornos: al proyecto (por defecto) o como globales de la app |
| `--on-conflict rename\|overwrite\|skip` | Qué hacer si un fichero o carpeta ya existe (por defecto `rename`) |
| `--no-scripts` | No importa los scripts de pre-request ni de test |

Capacidades:
- Colecciones v2.0 y v2.1, entornos, globals y el `.zip` de *Export data*.
- La autorización (incluida la heredada) se convierte.
- Los scripts de colección y carpeta se pueden integrar en las peticiones.
- Las variables que parecen secretos van a ficheros ignorados por git (`*.secret.toml`).

Al terminar imprime cuántas colecciones, carpetas, peticiones y entornos se importaron, los ficheros creados y los
avisos de conversión. Termina con código 1 si algún fichero de entrada no se pudo leer.

## `ilixpatch import openapi`
Importa especificaciones **OpenAPI 3.0 / 3.1** y **Swagger 2.0**, en JSON o YAML, desde ficheros, carpetas o URLs.
`swagger` es un alias del subcomando.

```sh
ilixpatch import openapi https://petstore3.swagger.io/api/v3/openapi.json --project ./my-project
ilixpatch import openapi api.yaml --project ./my-project --into ./my-project/api --group-by path
```

| Argumento / flag | Descripción |
|---|---|
| `<FILE\|URL>…` | Uno o varios ficheros (`.json`, `.yaml`, `.yml`), carpetas que los contengan o URLs `http(s)://` |
| `--project <dir>` | Proyecto de destino (obligatorio). Si aún no es un proyecto, se inicializa |
| `--into <dir>` | Importa cada especificación como carpeta dentro de esta colección o carpeta |
| `--group-by tags\|path` | Carpetas: una por tag (por defecto) o una por segmento de la ruta de la URL |
| `--on-conflict rename\|overwrite\|skip` | Qué hacer si un fichero o carpeta ya existe (por defecto `rename`) |
| `-k`, `--proxy`, `--ssl-extra-ca-certs`, … | Para descargar URLs, las mismas [opciones de red](#opciones-de-red-tls-mtls-proxy) que `run` |

Qué se obtiene:
- **Una colección por especificación**, con la variable de colección `baseUrl` (el primer servidor, con sus variables
  sustituidas). Si el servidor es relativo y la especificación se descargó, se resuelve contra su URL; si es un fichero,
  hay un aviso para que definas `baseUrl`.
- **Una petición por operación**, con el `summary` (o el `operationId`) como nombre:
  - Parámetros de ruta: su ejemplo, o `{{nombre}}`.
  - Query, headers y cookies: los obligatorios siempre (con su ejemplo, o `{{nombre}}`); los opcionales, solo si la
    especificación les da un valor.
  - Body: el ejemplo de la especificación o, si no lo hay, uno **generado a partir del esquema** (`$ref`, `allOf`,
    `oneOf`, formatos…).
  - `Accept` según la respuesta 2xx.
- **Auth** a partir de `security`: Bearer, Basic, Digest, API key y OAuth 2.0 (prefiere *client credentials*, que la CLI
  puede obtener sola). Las credenciales quedan como `{{bearerToken}}`, `{{clientSecret}}`… para que las definas en un
  entorno: no se escriben en `collection.toml`, que se sube a git.
- **Avisos** de lo que no se pudo convertir: `$ref` a otros ficheros, servidores adicionales, operaciones TRACE,
  esquemas de seguridad no admitidos, subida de ficheros…

La descarga sigue redirecciones, falla con estados que no son 2xx y admite hasta 200 MB. La especificación de GitHub
(11 MB de YAML, 1.280 operaciones) se importa en menos de un segundo.

## `ilixpatch ls`
Lista el contenido de un proyecto o colección sin ejecutar nada:

```sh
$ ilixpatch ls examples/demo-project
project: Demo project
project environments: dev
JSONPlaceholder (format v1)
  GET     posts/List posts
  POST    posts/Create post
  GET     posts/Get post
  GET     users/List users
  GET     users/Get user
  environments: -
Todos (format v1)
  GET     Get todo
  GET     open/List open todos
  environments: -
```

Útil para comprobar qué rutas usar con `--folder` / `--filter` y qué entornos hay disponibles.

## Opciones de red (TLS, mTLS, proxy)
Comunes a `run` y `load`:

| Flag | Descripción |
|---|---|
| `-k, --insecure` | Acepta certificados TLS inválidos o autofirmados |
| `--ssl-extra-ca-certs <pem>` | CAs adicionales de confianza (bundle PEM) |
| `--ssl-client-cert <pem\|p12>` | Certificado de cliente (mTLS) para todos los hosts. PEM (con `--ssl-client-key`) o PKCS#12 (`.p12` / `.pfx`) |
| `--ssl-client-key <pem>` | Clave privada del certificado PEM. Requiere `--ssl-client-cert` |
| `--ssl-client-passphrase <p>` | Contraseña de la clave o del PKCS#12 |
| `--ssl-client-cert-list <json>` | Lista de certificados por host, formato Newman (ver abajo). Tiene prioridad sobre `--ssl-client-cert` |
| `--ignore-redirects` | No sigue las redirecciones |
| `--proxy <url>` | Envía todo por este proxy (`http`, `https`, `socks5`, `socks5h`) |
| `--proxy-bypass <hosts>` | Hosts que no usan el proxy (sintaxis `NO_PROXY`). Requiere `--proxy`; se puede repetir |
| `--no-proxy` | Ignora los proxies del sistema y de las variables de entorno |
| `--use-app-settings` | Parte de los ajustes de red de la app (`network.toml`); los flags anteriores los sobrescriben |

Por defecto la CLI usa el proxy del sistema / `HTTP(S)_PROXY`, verifica TLS y sigue las redirecciones.

Formato de `--ssl-client-cert-list` (las rutas relativas son relativas al fichero de la lista; `cert`/`key`/`pfx`
pueden ser una cadena o `{"src": "..."}`):

```json
[
  {
    "name": "api",
    "matches": ["https://api.example.com/*", "https://*.corp.com/*"],
    "cert": "./client.crt",
    "key": "./client.key",
    "passphrase": "secreto"
  },
  { "name": "p12", "matches": "https://p12.example.com/*", "pfx": "bundle.p12" }
]
```

## Entornos y variables
**Precedencia** al resolver `{{variable}}` (de mayor a menor):

```text
local (pm.variables) > datos de iteración > entorno > colección > globales
```

- Los cambios que hacen los scripts (`pm.environment.set`, `pm.globals.set`, …) persisten durante toda la ejecución.
- `--env-var` sobrescribe valores del entorno elegido; `--global-var` se aplica encima de `globals.toml`.
- En un proyecto con varias colecciones, `--env` se aplica a todas las que definen ese entorno.
- Los ficheros `*.secret.toml` (ignorados por git) sobrescriben al entorno correspondiente.

**Compartido con la app de escritorio.** Los entornos globales y los globals viven en el directorio de configuración
de la app:

| SO | Directorio |
|---|---|
| macOS | `~/Library/Application Support/dev.ilixpatch.app` |
| Linux | `~/.config/dev.ilixpatch.app` |
| Windows | `%APPDATA%\dev.ilixpatch.app` |

con `environments/<nombre>.toml` (usados con `--global-env`), `globals.toml` y `network.toml` (usado con
`--use-app-settings`). En CI se puede apuntar a otro directorio con `ILIXPATCH_CONFIG_DIR`.

## Scripts (`pm.*`)
`run` y `load` ejecutan los scripts de **pre-request** y de **test** con una API compatible con Postman/Newman, así
que los scripts existentes funcionan sin cambios:

- **Tests y aserciones:** `pm.test`, `pm.test.skip`, `pm.expect` (Chai), `pm.response.to.have.status(...)`,
  `.header(...)`, `.jsonBody(...)`, etc.
- **Respuesta:** `pm.response.code`, `.status`, `.headers`, `.json()`, `.text()`, `.responseTime`, `.responseSize`.
- **Petición:** `pm.request.url`, `.method`, `.headers`, `.body` (modificables en pre-request).
- **Variables:** `pm.variables`, `pm.environment`, `pm.collectionVariables`, `pm.globals`, `pm.iterationData`
  (`get`, `set`, `has`, `unset`, `toObject`, `replaceIn`).
- **Información:** `pm.info.requestName`, `.iteration`, `.iterationCount`, `.eventName`; `pm.environment.name`.
- **Flujo:** `postman.setNextRequest(nombre | null)` / `pm.execution.setNextRequest`, y `pm.execution.skipRequest()`.
- **Utilidades:** `console.log`/`warn`/`error` (aparecen en la salida del reporter `cli`), `atob`/`btoa`,
  `setTimeout`, `require('chai')`.

Limitaciones actuales: `pm.sendRequest` todavía no está soportado y `require()` solo ofrece `chai`.

## Autorización
`run` y `load` aplican automáticamente la autorización de cada petición, **después** del script de pre-request y de
resolver las variables. Tipos soportados (todos los de Postman):

`api_key`, `bearer`, `jwt`, `basic`, `digest`, `oauth1`, `oauth2`, `hawk`, `awsv4`, `ntlm`, `akamai`, `asap`,
además de `inherit` y `no_auth`.

- Se hereda de la carpeta (`folder.toml`) y de la colección (`collection.toml`); `no_auth` corta la herencia.
- Todos los campos admiten `{{variables}}`.
- Una auth explícita sustituye a una cabecera `Authorization` manual (con un aviso, como en Postman).
- **Digest** responde al reto `401` del servidor (o envía la cabecera directamente si se indican `realm` y `nonce`).
- **NTLM** hace el handshake sobre una única conexión HTTP/1.1 keep-alive, así que no funciona a través de proxies que cierran conexiones.
- **OAuth 2.0:**
  - Si la petición tiene `access_token` (p. ej. `access_token = "{{token}}"` con `--env-var token=…`), se usa tal cual.
  - Si no, la CLI usa el mismo almacén de tokens que la app (`oauth2-tokens.json` en el directorio de datos de la
    app, o `ILIXPATCH_DATA_DIR`): renueva los tokens caducados y obtiene por sí sola los de `client_credentials` y
    `password`.
  - Los flujos interactivos (authorization code, PKCE, implicit) necesitan un token obtenido antes en la app.
  - Las peticiones de token usan las mismas opciones de red que la ejecución (proxy, CAs, certificado de cliente).

```toml
# collection.toml (o una petición .toml, o <carpeta>/folder.toml)
[auth]
type = "bearer"
token = "{{token}}"
```

## Diagnóstico de errores
Cuando una petición no se puede enviar, el reporter `cli` clasifica el fallo y muestra la causa probable y una pista:

| Error | Pista |
|---|---|
| URL inválida | Usa una URL absoluta con esquema |
| Variables no definidas | Elige un entorno (`-e`), defínelas en la colección o usa `--env-var` |
| DNS | Revisa el nombre del host, la red/VPN o la URL base del entorno |
| Conexión rechazada / reseteada | ¿Está el servidor arrancado? ¿http frente a https en ese puerto? |
| Timeout | Aumenta `--timeout-request` |
| Certificado no confiable / caducado / nombre distinto | `--ssl-extra-ca-certs`, o `-k/--insecure` |
| Handshake TLS | ¿Ese puerto es realmente HTTPS? |
| Certificado de cliente rechazado | `--ssl-client-cert` / `--ssl-client-key` / `--ssl-client-cert-list` |
| Error de proxy | Revisa `--proxy` o `HTTP(S)_PROXY`, o usa `--no-proxy` |
| Demasiadas redirecciones | Busca un bucle, o usa `--ignore-redirects` |

## Códigos de salida
| Código | Significado |
|---|---|
| `0` | Todo correcto (o `--suppress-exit-code`) |
| `1` | `run`: alguna petición dio error, algún test falló o la ejecución se canceló. `load`: se superó un umbral o no se completó ninguna petición. `import`: algún fichero no se pudo leer |
| `2` | Error de uso: flags inválidos, ruta inexistente, entorno no encontrado, ningún filtro coincide… |
| `130` | Abortado con un segundo `Ctrl-C` |

Como en Newman, un estado no-2xx **sin tests** no cuenta como fallo.

## Variables de entorno del sistema
| Variable | Efecto |
|---|---|
| `ILIXPATCH_CONFIG_DIR` | Directorio de configuración (entornos globales, `globals.toml`, `network.toml`) en lugar del de la app |
| `ILIXPATCH_DATA_DIR` | Directorio de datos (almacén de tokens OAuth 2.0) en lugar del de la app |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | Proxy del sistema (ignorados con `--no-proxy` o sustituidos por `--proxy`) |
| `NO_COLOR` | Desactiva los colores |

## Ejemplo en CI
GitHub Actions:

```yaml
- name: API tests
  run: |
    ilixpatch run . -e staging \
      --env-var api_key=${{ secrets.API_KEY }} \
      -r cli,junit --reporter-junit-export results/junit.xml
  env:
    ILIXPATCH_CONFIG_DIR: ${{ runner.temp }}/ilixpatch

- name: Smoke load test
  run: ilixpatch load ./api/health --vus 10 --duration 1m --max-p95 300 --max-error-rate 0.01
```
