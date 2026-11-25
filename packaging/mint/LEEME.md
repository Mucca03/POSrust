# POSrust para Linux Mint 22.3 (x86_64)

Extrae el paquete en una carpeta local en el PC de caja, por ejemplo
`~/POSrust`. No ejecutes el programa ni abras la base desde el USB.
No hace falta instalar Rust ni un servidor MySQL para ejecutar este paquete.
En Cinnamon se usan las bibliotecas gráficas del sistema. Si al abrir aparece
un error de X11 u OpenGL y faltan dependencias, en el equipo Mint puedes instalarlas con:

```sh
sudo apt install libgl1 libgl1-mesa-dri libegl1 libegl-mesa0 \
  libxkbcommon0 libxkbcommon-x11-0 libx11-6 libx11-xcb1 libxi6 \
  libxcursor1 libxrandr2 libwayland-client0
```

## Si la base de la tienda todavía está en MySQL/MariaDB

Obtén una exportación `.sql` actualizada de la **tienda**, no de la base de
ensayo, y consérvala aparte. No copies el directorio interno del motor.
El importador requiere el esquema heredado compatible. Si generas el respaldo
con `mysqldump` o `mariadb-dump`, usa UTF-8, fechas `TIMESTAMP` en UTC y
sentencias `INSERT` con nombres de tabla entre acentos graves, sin listas
explícitas de columnas. Comprueba que el exportador terminó sin errores antes
de importar. Desde la carpeta extraída:

```sh
mkdir -p data
./pos-import --dump "/ruta/respaldo-tienda.sql" \
  --db ./data/pos.db --report ./data/verificacion.txt
./abrir-pos.sh
```

El importador compara el número de filas leídas en las tablas compatibles
con el resultado de SQLite y comprueba la integridad de la copia.
Lee el `.sql` sin modificarlo y se niega a reemplazar una
base de destino existente. Si quieres ensayar primero, importa en un
archivo distinto y ábrelo indicando su ruta. El `.sql`, la SQLite y el informe
pueden contener información privada; no los subas al repositorio.

## Si la base ya es de POSrust

En el equipo de origen, usa **Utilities → Tools → Transfer Database** para
exportar una copia consistente. Pásala al otro equipo, guárdala en una carpeta
local y ábrela así:

```sh
./abrir-pos.sh "/ruta/copia-posrust.db"
```

Si la guardas en `data/pos.db`, `./abrir-pos.sh` la abre directamente.
**System → Configuration → Database Setup** permite verificar una copia en
solo lectura. Las bases no se fusionan: al cambiar de equipo, deja de registrar
ventas en el origen antes de hacer la copia que pasará a ser la operativa.

Prueba el lector, la impresora y el cajón en una copia antes de empezar a
registrar ventas reales. Si configuraste un logo externo, cópialo también;
la base solo guarda la ruta.
