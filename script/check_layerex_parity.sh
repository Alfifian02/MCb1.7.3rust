#!/usr/bin/env bash
# Cek: tiap nama method dari libgame.so 1.3.9 -> ada di source (src/plugins/**)?
# Default (ketat): nama dihitung ADA hanya bila muncul sebagai pendaftaran: NCB_METHOD(nama), TJS_W("nama") atau Layer.nama.
# Versi lama hanya mencari kata di seluruh teks (termasuk komentar) sehingga method yang belum ditulis tetapi disebut di komentar
# dilaporkan OK. Mode lama: LOOSE=1 bash script/check_layerex_parity.sh
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HERE="$(cd "$(dirname "$0")" && pwd)"
LIST="${1:-$HERE/layerex_expected_methods.txt}"
SRC="$ROOT/src/plugins"
TMP="$(mktemp 2>/dev/null || echo "$HERE/.parity.tmp")"
n_ext=$(find "$SRC/ext" -type f \( -name '*.cpp' -o -name '*.c' \) 2>/dev/null | wc -l)
echo "file C/C++ di src/plugins/ext: $n_ext"
grep -v '^#' "$LIST" | while IFS=: read -r mod names; do
  for m in $names; do
    if [ -n "$LOOSE" ]; then pat="$m"; opt="-rqw"; else pat="NCB_METHOD\\($m\\)|TJS_W\\(\"$m\"\\)|Layer\\.$m([^A-Za-z0-9_]|$)"; opt="-rqE"; fi
    if grep $opt --include='*.cpp' --include='*.h' --include='*.hpp' -- "$pat" "$SRC" 2>/dev/null; then
      echo "OK      $mod::$m"
    else
      echo "KOSONG  $mod::$m"
    fi
  done
done | sort > "$TMP"
grep -c '^OK' "$TMP" | sed 's/^/ada    : /'
grep -c '^KOSONG' "$TMP" | sed 's/^/kosong : /'
grep '^OK' "$TMP"
[ -n "$FULL" ] && grep '^KOSONG' "$TMP"
echo "(FULL=1 bash script/check_layerex_parity.sh  -> tampilkan juga daftar KOSONG)"
rm -f "$TMP" "$HERE/.parity.tmp"
