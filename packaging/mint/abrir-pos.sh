#!/bin/sh
# Uso: ./abrir-pos.sh [ruta a una base POSrust]
set -eu
carpeta=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
if [ "$#" -gt 1 ]; then
    printf 'Uso: %s [ruta a la base SQLite]\n' "$0" >&2
    exit 2
fi

base=${1:-"$carpeta/data/pos.db"}
if [ ! -f "$base" ]; then
    mensaje="No se encontró la base POSrust: $base. Coloca una copia en data/pos.db o indica su ruta al abrir."
    printf '%s\n' "$mensaje" >&2
    if [ "$#" -eq 0 ] && command -v zenity >/dev/null 2>&1; then
        zenity --error --title='POSrust' --text="$mensaje" || :
    fi
    exit 1
fi

exec "$carpeta/pos" --db "$base"
