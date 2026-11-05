#!/usr/bin/env bash
set -euo pipefail
[ "$#" -eq 1 ] || { echo 'Использование: smoke-conversion.sh файл.doc'; exit 2; }
[ -f "$1" ] || { echo 'Файл не найден' >&2; exit 2; }
d="$(mktemp -d)"
trap 'rm -rf "$d"' EXIT
libreoffice "-env:UserInstallation=file://$d/profile" --headless --convert-to 'html:XHTML Writer File' --outdir "$d" "$1" >/dev/null
base="$(basename "$1")"
base="${base%.*}"
html="$d/$base.html"
[ -s "$html" ] || { echo 'Не сформирован XHTML' >&2; exit 1; }
if grep -qE '3025035441|инн|ИНН' "$html"; then echo "PASS: XHTML создан, имеются данные"; else echo 'PASS: XHTML создан, ИНН не обнаружен'; fi
