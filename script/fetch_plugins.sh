#!/usr/bin/env bash
# Ambil source plugin publik wamsoft ke src/plugins/ext/ (tidak mengubah file lain).
# Pakai:  bash script/fetch_plugins.sh [layerEx windowEx scriptsEx]
# Variabel: REF=<tag/branch> untuk menyematkan versi.
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXT="$ROOT/src/plugins/ext"
BASE="${WAMSOFT_BASE:-https://github.com/wamsoft}"
# repo di /storage/emulated/0 dimiliki uid lain -> git menolak ("dubious ownership"); abaikan hanya untuk perintah ini
G="git -c safe.directory=*"
mkdir -p "$EXT"
NAMES=("$@"); [ ${#NAMES[@]} -gt 0 ] || NAMES=(layerEx)
for n in "${NAMES[@]}"; do
  dst="$EXT/$n"
  if [ -d "$dst" ]; then echo "## sudah ada: $n"; continue; fi
  echo "## clone $BASE/$n.git ${REF:+($REF)}"
  $G clone --recursive --depth 1 ${REF:+--branch "$REF"} "$BASE/$n.git" "$dst"
  echo "$n @ $($G -C "$dst" rev-parse HEAD)" >> "$EXT/PINNED.txt" || true
  echo "## isi $n:"; ( cd "$dst" && find . -path ./.git -prune -o -type f -print | head -30 )
  c=$(find "$dst" -path "$dst/.git" -prune -o \( -name '*.cpp' -o -name '*.c' \) -print | wc -l)
  echo "## file .cpp/.c di $n: $c"
  [ "$c" -gt 0 ] || echo "!! $n tidak berisi source C/C++ -- plugin harus direkonstruksi dari binary (Tahap 4)"
done
echo "## modul NCB yang ditemukan:"
grep -rhoE 'NCB_MODULE_NAME[ ]+TJS_W\("[^"]+"\)' "$EXT" 2>/dev/null | sort -u || true
echo "Lanjut: bash script/check_layerex_parity.sh"
