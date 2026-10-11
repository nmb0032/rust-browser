"""Generate our original 4x2 red/blue object-fit fixture; no external artwork."""
import argparse
import pathlib
import struct
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
TARGET = ROOT / "tests/reftests/support/stripes.png"


def fixture():
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data)))
    row = b"\0" + bytes([255, 0, 0, 255]) * 2 + bytes([0, 0, 255, 255]) * 2
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">2I5B", 4, 2, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(row * 2))
            + chunk(b"IEND", b""))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        if not TARGET.is_file() or TARGET.read_bytes() != fixture():
            parser.exit(1, "stripes.png is missing or stale\n")
    else:
        TARGET.write_bytes(fixture())
