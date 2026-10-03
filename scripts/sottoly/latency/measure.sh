#!/bin/sh
# Mide la latencia de Segmentos (Q31) con la App corriendo y una grabación iniciada.
#   scripts/sottoly/latency/measure.sh <app.log>
# Reproduce phrases.txt con voz sintética por la salida por defecto (entra como Contraparte),
# luego imprime cada Segmento transcrito, su latencia y p50/p90.
set -e
LOG="${1:?uso: measure.sh <ruta al log de la App con RUST_LOG=info>}"
DIR="$(cd "$(dirname "$0")" && pwd)"
TMP="${TMPDIR:-/tmp}/sottoly-latency"
mkdir -p "$TMP"

# 1. Nada más debe sonar: el tap de Meetily es global y captura cualquier proceso.
[ -x "$TMP/audio-procs" ] || swiftc -O -o "$TMP/audio-procs" "$DIR/audio-procs.swift" 2>/dev/null
echo "Procesos con audio activo (solo debería aparecer Meetily):"
"$TMP/audio-procs"

# 2. Frases sintéticas, 4 s de pausa entre una y otra (Turnos separados).
i=0
while IFS= read -r line; do
  i=$((i+1))
  [ -f "$TMP/frase-$i.aiff" ] || say -v Paulina -o "$TMP/frase-$i.aiff" "$line"
done < "$DIR/phrases.txt"
start=$(wc -l < "$LOG")
for n in $(seq 1 "$i"); do afplay "$TMP/frase-$n.aiff"; sleep 4; done
sleep 8

# 3. Resultado: texto transcrito, latencias y percentiles.
tail -n +"$((start+1))" "$LOG" | tr -d '\033' \
  | grep -E "Float sample|transcribed:|SOTTOLY_LATENCY" \
  | sed -E 's/^\[[0-9-]+T([0-9:]+)Z [A-Z]+ +[a-z_:]+\] /\1 /; s/✅ Worker [0-9]+ transcribed: /TXT: /; s/ \(confidence.*//'
tail -n +"$((start+1))" "$LOG" | grep -o "latency_ms=[0-9]*" | cut -d= -f2 | sort -n \
  | awk '{v[NR]=$1} END {if (NR==0) {print "sin Segmentos"; exit} \
         i50=int(0.5*(NR-1)+0.5)+1; i90=int(0.9*(NR-1)+0.5)+1; \
         printf "n=%d p50=%d p90=%d max=%d\n", NR, v[i50], v[i90], v[NR]}'
