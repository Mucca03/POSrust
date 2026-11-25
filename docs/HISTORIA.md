# Historia de POSrust

Resumen de los hitos del desarrollo anterior al primer commit. No representa
versiones antiguas recuperadas de Git.

1. **Diseño:** una caja nativa con interfaz OpenGL, SQLite local y aritmética
   decimal para dinero y cantidades.
2. **Núcleo:** productos, inventario, impuestos, descuentos, pagos mixtos,
   devuelta, permisos y arqueo, con pruebas de invariantes.
3. **Persistencia:** migración de exportaciones compatibles, transacciones
   atómicas, conservación de tickets previos, respaldos y reportes CSV.
4. **Caja y periféricos:** interfaz táctil y de teclado, lector HID, varias
   ventas abiertas, catálogo, herramientas y cierre. Impresión manual y
   apertura del cajón independientes.
5. **Preparación para distribuir:** binario para Linux Mint x86_64 y separación
   del código fuente respecto de bases, informes y archivos privados.
