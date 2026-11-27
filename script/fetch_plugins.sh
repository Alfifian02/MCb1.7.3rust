#!/usr/bin/env bash
# Ambil source plugin publik ke src/plugins/ext/ (tidak mengubah file lain).
# Pakai:  bash script/fetch_plugins.sh [nama ...]        (tanpa argumen: layerEx)
# Alamat dicari di script/ext_sources.tsv (kolom 1=nama, 2=url); bila tidak ada -> $WAMSOFT_BASE/<nama>.git
# Variabel: REF=<tag/branch> menyematkan versi · WAMSOFT_BASE=<url dasar>
# Satu nama yang gagal (repo tidak ada) TIDAK menghentikan nama lain; ringkasan ada di akhir.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HERE="$(cd "$(dirname "$0")" && pwd)"
EXT="$ROOT/src/plugins/ext"
BASE="${WAMSOFT_BASE:-https://github.com/wamsoft}"
SRCLIST="${EXT_SOURCES:-$HERE/ext_sources.tsv}"
# repo di /storage/emulated/0 dimiliki uid lain -> git menolak ("dubious ownership"); abaikan hanya untuk perintah ini
G="git -c safe.directory=*"
export GIT_TERMINAL_PROMPT=0   # repo yang tidak ada jangan minta kredensial
mkdir -p "$EXT"
NAMES=("$@"); [ ${#NAMES[@]} -gt 0 ] || NAMES=(layerEx)
ok=(); gagal=(); ada=()
for n in "${NAMES[@]}"; do
  dst="$EXT/$n"
  if [ -d "$dst" ]; then echo "## sudah ada: $n"; ada+=("$n"); continue; fi
  url=""
  [ -f "$SRCLIST" ] && url="$(awk -F'\t' -v n="$n" '$0!~/^#/ && $1==n {print $2; exit}' "$SRCLIST")"
  [ -n "$url" ] || url="$BASE/$n.git"
  echo "## clone $url ${REF:+($REF)}"
  if ! $G clone --recursive --depth 1 ${REF:+--branch "$REF"} "$url" "$dst"; then
    echo "!! gagal clone $n -- lewati"; rm -rf "$dst"; gagal+=("$n"); continue
  fi
  echo "$n @ $($G -C "$dst" rev-parse HEAD)" >> "$EXT/PINNED.txt" || true
  echo "## isi $n:"; ( cd "$dst" && find . -path ./.git -prune -o -type f -print | head -30 )
  c=$(find "$dst" -path "$dst/.git" -prune -o \( -name '*.cpp' -o -name '*.c' \) -print | wc -l)
  echo "## file .cpp/.c di $n: $c"
  [ "$c" -gt 0 ] || echo "!! $n tidak berisi source C/C++ -- plugin harus direkonstruksi dari binary (Tahap 4)"
  lic=$(find "$dst" -path "$dst/.git" -prune -o -maxdepth 2 -type f \( -iname 'LICEN[SC]E*' -o -iname 'COPYING*' -o -iname 'readme*' \) -print)
  echo "## berkas lisensi/readme di $n: ${lic:-tidak ada}"
  ok+=("$n")
done
echo "## modul NCB yang ditemukan:"
grep -rhoE 'NCB_MODULE_NAME[ ]+TJS_W\("[^"]+"\)' "$EXT" 2>/dev/null | sort -u || true
echo "## ringkasan: berhasil=${#ok[@]} (${ok[*]}) · sudah ada=${#ada[@]} · GAGAL=${#gagal[@]} (${gagal[*]})"
echo "Hapus clone yang tidak dipakai build (mis. layerEx, windowEx) -- lihat docs/plugins.md 'Risiko build ext'."
echo "Lanjut: bash script/check_layerex_parity.sh"
[ ${#gagal[@]} -eq 0 ]
