# Tools: importación y transferencia

Haz la primera prueba sobre una copia. La vista previa no guarda cambios. Al
confirmar, el programa valida de nuevo, aplica las filas válidas y registra
las rechazadas con su motivo. Un fallo de base de datos revierte el lote.
«Clear Import Log» archiva los resultados visibles sin borrar la auditoría.

## Productos

CSV UTF-8 de hasta 1 MB y 1.000 filas. Usa `;` o `,`; con coma decimal
se recomienda `;`. Los códigos se leen como texto y conservan ceros iniciales.
No uses separadores de miles. Abre el archivo como texto para comprobar que tu
hoja de cálculo no haya quitado esos ceros.

```csv
accion;codigo;referencia;nombre;categoria;precio;costo;unidad;cantidad
alta;00012345;ARROZ-1;Arroz 1 kg;Alimentos;6400,50;4300;unidad;
precio;00012345;;;;6800;;;
entrada;00012345;;;;;4300;;12
```

- **`alta`**: requiere código, referencia, nombre, categoría y precio. La
  categoría debe existir; usa su nombre o ID. El costo es opcional. Puedes
  añadir las columnas `impuesto` (ID de categoría fiscal, `100` = Sin IVA
  por defecto) y `catalogo` (`1` visible, `0` oculto). La unidad es `unidad` o
  `kg`. El alta no crea existencias.
- **`precio`**: busca por código exacto y solo cambia el precio de catálogo; usa
  la misma modalidad de IVA configurada en POSrust. No cambia tickets previos.
- **`entrada`**: suma la cantidad al inventario y registra el movimiento;
  no sustituye el saldo. En kg acepta hasta tres decimales. Si omites el costo
  se usa el de la ficha. No vuelvas a confirmar un CSV de entradas ya aplicado:
  cada confirmación registra una nueva entrada.

**Products Scanner Import** registra entradas para productos por unidad usando
un lector USB en modo teclado con Enter al final. Cada lectura equivale a una
unidad; se pueden quitar lecturas antes de confirmar. Usa CSV para kg.

## Clientes

```csv
codigo;nombre;nit;telefono;correo
C-001;Cliente de ejemplo;123456789;;
```

Solo son obligatorios `codigo` y `nombre`. Se rechazan códigos o NIT no vacíos
duplicados. No se editan fichas existentes.

## Transfer Database

Indica una carpeta existente (por ejemplo, una USB ya montada). Se genera
una copia SQLite con nombre nuevo y se verifica su integridad antes de
publicarla. No reemplaza la base activa ni el respaldo original.
