#!/bin/sh
# Renders a line of synthetic speech to raw mono PCM16 at 24 kHz, the format
# a GPT-Live session expects. Output is generated, not committed.
#   fixtures/audio/make-fixture.sh customer-cmdb "Our CMDB gets outdated very quickly."
# An optional third argument adds that many seconds of silence before the speech.
set -eu
name="$1"
text="$2"
lead="${3:-0}"
dir="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
say -o "$tmp/speech.aiff" "$text"
afconvert -f WAVE -d LEI16@24000 -c 1 "$tmp/speech.aiff" "$tmp/speech.wav"
# Strip the WAV container: the API takes headerless samples.
python3 - "$tmp/speech.wav" "$dir/$name.pcm" "$lead" <<'PY'
import sys, wave
with wave.open(sys.argv[1], "rb") as w:
    assert (w.getnchannels(), w.getsampwidth(), w.getframerate()) == (1, 2, 24000)
    silence = bytes(int(float(sys.argv[3]) * 24000) * 2)
    open(sys.argv[2], "wb").write(silence + w.readframes(w.getnframes()))
PY
echo "$dir/$name.pcm"
