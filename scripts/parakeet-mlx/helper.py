"""Run local Parakeet TDT v3 MLX on length-prefixed, 16 kHz float32 windows."""

import json
import os
from pathlib import Path
import struct
import sys

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
os.environ["NUMBA_DISABLE_JIT"] = "1"

import mlx.core as mx
import numpy as np
from parakeet_mlx import from_pretrained
from parakeet_mlx.audio import get_logmel


def main():
    model_directory = Path(sys.argv[1]).resolve(strict=True)
    mx.set_default_device(mx.gpu)
    model = from_pretrained(str(model_directory), dtype=mx.bfloat16)
    source, dest = sys.stdin.buffer, sys.stdout.buffer
    while header := source.read(4):
        (count,) = struct.unpack("<I", header)
        if not 0 < count <= 30 * 16_000:
            raise ValueError("Invalid Parakeet window length")
        size = count * 4
        data = source.read(size)
        if len(data) != size:
            raise EOFError("Incomplete Parakeet window")
        # The preprocessor needs one FFT window even if the recording ends just past a seam.
        samples = np.frombuffer(data, dtype="<f4")
        minimum = model.preprocessor_config.n_fft
        if len(samples) < minimum:
            samples = np.pad(samples, (0, minimum - len(samples)))
        audio = mx.array(samples, dtype=mx.float32)
        tokens = model.generate(get_logmel(audio, model.preprocessor_config))[0].tokens
        response = json.dumps({
            "tokens": [token.text.replace("▁", " ") for token in tokens],
            "timestamps": [token.start for token in tokens],
        }, allow_nan=False).encode("utf-8")
        dest.write(struct.pack("<I", len(response)))
        dest.write(response)
        dest.flush()


if __name__ == "__main__":
    main()
