# POSrust

POS nativo en Rust para una caja única, diseñado para equipos con 4 GB de RAM.
Usa SQLite y una interfaz OpenGL, sin Electron ni WebView.

## Estado

Incluye ventas, movimientos y cierre de caja, consulta e impresión manual,
búsqueda y edición de productos, entradas de inventario, reportes CSV y
configuración de periféricos. Falta la validación física en los equipos objetivo.
Consulta los [hitos del desarrollo](docs/HISTORIA.md).

## Requisitos

- Rust estable actual (probado con 1.100).
- Linux con sesión gráfica y OpenGL (objetivo: Linux Mint). Windows es un objetivo, aún no validado.
- No requiere MySQL, MariaDB, Node, WebView ni Java.

Para trasladar una base o abrir el paquete precompilado, consulta
la [guía para Linux Mint 22.3 x86_64](packaging/mint/LEEME.md).

## Comandos

```bash
cargo test --workspace
cargo run -p pos-import -- --help
```

Importación de prueba (use rutas nuevas):

```bash
cargo run --release -p pos-import -- \
  --dump /ruta/respaldo.sql \
  --db ./data/pos-prueba.db \
  --report ./data/verificacion.txt
```

El importador es opcional si ya tienes una SQLite de POSrust. Lee exportaciones
con el esquema heredado compatible; no es un conversor de cualquier esquema
MySQL. Conserva las diferencias históricas y, por defecto, no reemplaza una base
existente. No subas bases, reportes ni tirillas a un repositorio.

Para probar la interfaz con la base de prueba recién importada:

```bash
cargo run --release -p pos -- --db ./data/pos-prueba.db
```

La interfaz sí guarda las ventas y los cambios de caja en la base que se le
indica. Úsala con la base de prueba, no con la operativa. Elige un usuario
para entrar con un clic. Se conserva su rol, pero no se pide contraseña: quien
tenga acceso físico a la caja puede elegir cualquier perfil visible. El
respaldo conserva intactos los datos de acceso. En ventas, **Espacio** o **F6** abre el cobro,
**+ / −** cambia las unidades de la línea seleccionada, **F1** enfoca el lector,
**F2** abre la búsqueda de productos y **F3** edita cantidad o peso. Con el
permiso de cambiar precios, también permite ajustar el precio con IVA de esa
línea, sin cambiar el catálogo.
Si estás digitando una búsqueda, Espacio y los signos siguen siendo texto.
El cobro en COP muestra las denominaciones colombianas, el efectivo recibido,
la devuelta y el teclado numérico.
Después de cobrar aparece la ventana **«Vueltas»**. **Continuar** (o **Enter**)
vuelve a la venta vacía; no se registra otro ticket ni se imprime al cerrarla.
**Nueva venta** deja pendiente el ticket actual y **Ventas abiertas** permite
retomarlo. Son borradores en memoria, no ventas registradas; debes cobrarlos o
cancelarlos antes de cerrar caja o salir normalmente. **F8** ejecuta
**«Último pago»**, también disponible en la barra superior.
El ticket ocupa la parte superior, con las acciones de línea
a su izquierda y el lector y teclado a su derecha; el catálogo queda abajo,
con categorías a la izquierda. La pestaña izquierda (o **F10**) abre el menú
lateral. **Edit Sales** consulta y
reimprime sin modificar ventas históricas; **Payments** registra entradas y
salidas, y **Close Cash** muestra el arqueo, el desglose por medio de pago y
por categoría de impuesto; permite cerrar y luego imprimir el resumen.
En **Stock → Products** puedes filtrar por código, nombre, referencia, categoría,
costo y precio, navegar resultados y crear o editar fichas. La pestaña
**Stock** muestra las existencias del producto. Crear o editar una ficha no
registra movimientos: usa **Stock Management** para registrar una entrada.
**Inventory Current** permite consultar existencias. **Maintenance** consulta
usuarios, ubicación e impuestos. **Tools** ofrece importación de productos y
clientes por CSV, entrada de stock por lector, reportes de importación,
reportes de ventas y una copia SQLite verificada para transferir.
Consulta [formatos y ejemplos de importación](docs/TOOLS.md).
Con el perfil Administrador, **Utilities → Printers** configura el destino,
el ancho, el corte y el cajón sin reiniciar. En **Contenido del recibo** puedes
editar los datos del negocio, el encabezado y el pie, elegir campos opcionales
y ver e imprimir una muestra sin registrar una venta. Al guardar, el formato
se aplica a **Último pago (F8)** y a las reimpresiones. Sin impresora
configurada, la tirilla se guarda como archivo `.bin` en `tirillas/` junto a
la base. Con el **Cajón** habilitado y un destino RAW/ESC/POS, cualquier pago
confirmado envía un pulso independiente antes de mostrar **Vueltas**. La caja
espera como máximo un segundo por la respuesta del envío; si no llega, muestra
las vueltas con un aviso y el pago permanece guardado. **Último pago** solo
imprime. Usa **Probar cajón sin imprimir** para probar sin una venta.
El equipo que recibe el pulso debe interpretar ESC/POS y accionar el cajón.
Si la impresora de recibos no lo admite, puedes configurar otro destino RAW
compatible para el cajón; enviar a una cola no confirma la apertura física.

**System → Configuration** tiene tres pestañas:

- **General:** personaliza el acceso con un **Logo** PNG local (hasta 2048×2048
  y 4 MiB) y un **Text** de bienvenida que puedes escribir o cargar desde un
  archivo UTF-8. Las rutas relativas se buscan junto a la base. El logo es un
  archivo externo: consérvalo al mover el POS a otro PC.
- **Peripherals:** pantalla de cliente opcional en una segunda ventana,
  ajuste de impresora principal y cajón (compartido con **Utilities → Printers**),
  prueba de lector USB HID sin venta y peso manual en kg.
- **Database Setup:** identifica la SQLite operativa, comprueba esa base u otra
  copia en solo lectura y permite configurar o crear respaldos. No reemplaza
  la base en uso. Para cambiar de copia, cierra y vuelve a iniciar con `--db`.

Opciones de impresión (tienen prioridad sobre los ajustes guardados): `--paper 58` o `--paper 80` y
`--printer cups:NOMBRE_DE_COLA`, `--printer tcp:IP:9100`,
`--printer "windows_spool:NOMBRE DE IMPRESORA"` o `--printer file:RUTA_DE_CARPETA`.

## Estructura

```
crates/pos-core/    Dominio puro: dinero, ticket, impuestos, descuentos,
                    pagos, caja, stock y permisos. Sin IO, sin UI, sin SQLite.
crates/pos-store/   (Fase 3) SQLite, migraciones, repositorios, importador.
crates/pos-hardware/ ESC/POS 58/80 mm, archivo/TCP/CUPS/Windows RAW y pulso de cajón.
crates/pos-ui/      (Fase 4) pantallas egui.
apps/pos/           (Fase 4) binario principal.
apps/pos-import/    (Fase 3) CLI de importación y verificación.
```

Regla de dependencias: `pos-core` no conoce a nadie; `pos-store` y
`pos-hardware` dependen de `pos-core`; `pos-ui` depende de los tres; las apps
cablean todo.

## Decisiones clave

- **Dinero:** `rust_decimal::Decimal`, nunca `f64`. En SQLite se guarda como
  texto decimal exacto y las sumas se hacen en Rust.
- **Base de datos:** SQLite embebido. El respaldo MySQL original **no se
  modifica ni se limpia**; se importa a una base nueva conservando IDs,
  historial y anomalías.
- **Fechas:** hora local de Colombia (`America/Bogota`, UTC-05:00).
- **Impresión:** ESC/POS propio, 58/80 mm. La venta se guarda primero; la
  tirilla se imprime con el botón **“Último pago”**, no automáticamente.
- **Interfaz:** egui/eframe con OpenGL y controles para teclado o pantalla táctil.
- **Caja:** se conservan las sesiones históricas y se continúa la numeración
  del terminal en la base. Redondeo de efectivo por defecto 0.
- **Respaldos:** semanales mientras se usa la app, en `backups/` junto a la base, más botón manual.

## Pruebas

El workspace prueba, entre otros:

- dinero, cantidades, tasas y formatos COP;
- IVA incluido/excluido, descuentos de línea y total;
- expansión a líneas persistibles y ajuste de redondeo;
- pagos mixtos, devuelta y validaciones;
- conteo de denominaciones y cierre de caja;
- stock negativo y movimientos de venta/entrada;
- permisos por rol;
- invariantes de totales y pagos con `proptest`;
- preservación del dump, reportes CSV, cobros mixtos persistidos y
  aislamiento de las sesiones históricas;
- codificación de acentos, anchos de papel, corte, tirillas y pulso ESC/POS;
- escaneo HID simulado, peso manual, y confirmación de que guardar una
  venta no imprime: F8 genera la tirilla y, si se repite, la marca como reimpresión;
- posición de los controles a 820 × 600 y 1920 × 1080, menú según permisos y
  flujo táctil simulado desde la selección del producto hasta confirmar el cobro.

## Próximo paso

Completar la edición de productos y categorías y ampliar mantenimiento.
Validar la compilación y ejecución en Windows, el consumo en el PC objetivo
y la impresión física con impresora y cajón.
