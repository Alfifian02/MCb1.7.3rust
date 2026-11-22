#!/data/data/com.termux/files/usr/bin/bash
# k2recon.sh - dump disassembly satu method (+ callee-nya) dari libgame.so arm64, hanya pakai objdump.
# Pakai:  bash script/k2recon.sh <namaMethod|0xALAMAT> [depth=2] > recon_<nama>.txt
#         bash script/k2recon.sh @<kelompok> [depth=0] > recon_<kelompok>.txt   (MODE BATCH, lihat bawah)
# Batch:  @btoa @image @alpha @shimmer @save @raster @shrinkcopy @all  -> semua implementasi satu kelompok
#         dari script/layerex_impl_map.tsv (dibuat script/k2map.py). Env ONLY="turnAlpha fillAlpha" memilih method tertentu.
# Env  :  PTR=1 (ikuti pointer adrp+add ke fungsi, default aktif; PTR=0 matikan)  LIB=~/k2_out/libgame.so  MAXF=40 (maks jumlah fungsi di-dump)  MAXSZ=6000 (abaikan callee > byte)
# Perlu: pkg install binutils   (objdump native aarch64 -> hanya jalan di HP arm64)
HERE="$(cd "$(dirname "$0")" && pwd)"
LIB="${LIB:-$HOME/k2_out/libgame.so}"
MAP="$HERE/layerex_binary_map.txt"
FUNCS="$HERE/funcs_arm64.txt"
MAXF="${MAXF:-}"; MAXSZ="${MAXSZ:-6000}"; IMPL="$HERE/layerex_impl_map.tsv"
q="$1"; depth="${2:-2}"
[ -n "$q" ] || { sed -n '2,9p' "$0"; exit 1; }
[ -f "$LIB" ] || { echo "LIB tidak ada: $LIB (jalankan k2tool.sh extract)" >&2; exit 1; }
declare -A seen; count=0

fnext() { awk -v a="$1" '$1>a{print $1; exit}' "$FUNCS"; }          # awal fungsi berikutnya (desimal)
isfn()  { awk -v a="$1" '$1==a{f=1; exit} END{exit !f}' "$FUNCS"; }  # apakah a awal fungsi

dump() {  # $1 alamat hex tanpa 0x, $2 sisa depth
  local hx="$1" d="$2" s e sz tmp t
  [ -n "${seen[$hx]}" ] && return
  seen[$hx]=1
  s=$((16#$hx)); isfn "$s" || return
  e=$(fnext "$s"); sz=$((e - s))
  [ "$sz" -le "$MAXSZ" ] || { echo "### lewati 0x$hx (ukuran $sz > MAXSZ)"; return; }
  count=$((count + 1)); [ "$count" -le "$MAXF" ] || return
  echo; echo "=== fungsi 0x$hx  ukuran=$sz  sisa_depth=$d ==="
  tmp="$(mktemp 2>/dev/null || echo "$HERE/.k2recon.tmp")"
  objdump -d -C --no-show-raw-insn --start-address="$s" --stop-address="$e" "$LIB" \
    | grep -E '^ +[0-9a-f]+:' > "$tmp"
  cat "$tmp"
  if [ "$d" -gt 0 ]; then
    # callback/fungsi yang disimpan sebagai pointer (adrp+add) -> penting utk registrasi ncbind
    if [ "${PTR:-1}" = 1 ]; then
      for pi in $(sed -E 's/<[^>]*>//g' "$tmp" | awk '
        $2=="adrp"{r=$3; sub(/,$/,"",r); pg[r]=$4}
        $2=="add"{rd=$3; rn=$4; sub(/,$/,"",rd); sub(/,$/,"",rn);
                  if (rn in pg && substr($5,1,1)=="#") {print pg[rn] ":" substr($5,2); delete pg[rn]} }'); do
        t=$(printf '%x' $(( ${pi%%:*} + ${pi##*:} )))
        dump "$t" $((d - 1))
      done
    fi
    for t in $(grep -oE '\bbl[[:space:]]+(0x)?[0-9a-f]+' "$tmp" | awk '{sub(/^0x/,"",$2); print $2}' | sort -u); do
      dump "$t" $((d - 1))
    done
  fi
  rm -f "$tmp"
}

group=""
if [[ "$q" == @* ]]; then
  # --- MODE BATCH: semua implementasi satu kelompok, kolom: modul method reg ukuranreg impl ukuranimpl catatan
  [ -f "$IMPL" ] || { echo "tidak ada $IMPL (jalankan: python3 script/k2map.py)" >&2; exit 1; }
  group="$(echo "${q#@}" | tr 'A-Z' 'a-z')"; depth="${2:-0}"
  rows="$(awk -F'\t' -v g="$group" -v only="${ONLY:-}" '
    BEGIN{n=split(only,o," "); for(i=1;i<=n;i++) want[o[i]]=1}
    /^#/ || $5 !~ /^0x/ {next}
    { m=tolower($1); sub(/^layerex/,"",m)
      if (g!="all" && m!=g) next
      if (n>0 && !($2 in want)) next
      print $2 "\t" substr($5,3) "\t" $6 }' "$IMPL")"
  [ -n "$rows" ] || { echo "kelompok kosong/tidak dikenal: $q" >&2; exit 1; }
  [ -n "$MAXF" ] || MAXF=$(( $(echo "$rows" | wc -l) + 8 ))
  echo "# k2recon BATCH: $q depth=$depth  LIB=$LIB  method=$(echo "$rows" | wc -l)  MAXF=$MAXF MAXSZ=$MAXSZ"
  echo "# daftar: nama  impl  ukuran"; echo "$rows" | awk -F'\t' '{printf "#   %-28s 0x%s %s\n",$1,$2,$3}'
  while IFS=$'\t' read -r nm addr sz; do
    [ "$sz" -le "$MAXSZ" ] 2>/dev/null || { echo; echo "### LEWATI $nm 0x$addr (ukuran $sz > MAXSZ=$MAXSZ; ulangi dengan MAXSZ=$sz)"; continue; }
    echo; echo "##### METHOD $nm  impl=0x$addr  ukuran=$sz"
    dump "$addr" "$depth"
  done <<< "$rows"
else
  [ -n "$MAXF" ] || MAXF=40
  if [[ "$q" == 0x* ]]; then roots="${q#0x}"
  else roots="$(grep -F "'$q'" "$MAP" | awk '{print $1}' | sed 's/^0x//')"; fi
  [ -n "$roots" ] || { echo "tidak ada di peta: $q" >&2; exit 1; }
  echo "# k2recon: $q depth=$depth  LIB=$LIB"
  for r in $roots; do dump "$r" "$depth"; done
fi
echo; echo "# selesai: $count fungsi"
