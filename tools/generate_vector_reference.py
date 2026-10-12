"""Original analytic radial-gradient references; no browser/engine capture."""
import argparse
from pathlib import Path
import struct
import zlib


def png(scale):
    size = 40 * scale
    rows = bytearray()
    for y in range(size):
        rows.append(0)
        for x in range(size):
            distance2 = ((x + 0.5) / scale - 20) ** 2 + ((y + 0.5) / scale - 20) ** 2
            rows.extend((255, 0, 0, 255) if distance2 < 100 else (0, 0, 255, 255))

    def chunk(name, data):
        return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data))

    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent / "tests/reftests-vectors/support"
    for scale in (1, 2):
        path = root / f"radial-{scale}.png"
        data = png(scale)
        if args.check:
            if not path.exists() or path.read_bytes() != data:
                raise SystemExit(f"stale reference: {path}")
        else:
            root.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)


if __name__ == "__main__":
    main()
