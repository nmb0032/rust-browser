"""Original SDR fixtures/references, independently computed from W3C CSS Color 4
sample matrices and ICC.1 matrix/TRC tags; no browser or engine captures.
https://www.w3.org/TR/css-color-4/#color-conversion-code
https://www.color.org/specification/ICC.1-2022-05.pdf
"""
import argparse
from pathlib import Path
import struct
import zlib

SRGB = (
    (0.4123907993, 0.3575843394, 0.1804807884),
    (0.2126390059, 0.7151686788, 0.0721923154),
    (0.0193308187, 0.1191947798, 0.9505321522),
)
P3 = (
    (0.4865709486, 0.2656676932, 0.1982172852),
    (0.2289745641, 0.6917385218, 0.0792869141),
    (0.0, 0.0451133819, 1.0439443689),
)
D65_TO_D50 = (
    (1.0479298208, 0.0229467933, -0.0501922295),
    (0.0296278157, 0.9904344846, -0.0170738250),
    (-0.0092430582, 0.0150551449, 0.7518742814),
)


def mul(matrix, vector):
    return tuple(sum(a * b for a, b in zip(row, vector)) for row in matrix)


def inverse(matrix):
    a, b, c = matrix
    cofactor = (
        (b[1]*c[2]-b[2]*c[1], a[2]*c[1]-a[1]*c[2], a[1]*b[2]-a[2]*b[1]),
        (b[2]*c[0]-b[0]*c[2], a[0]*c[2]-a[2]*c[0], a[2]*b[0]-a[0]*b[2]),
        (b[0]*c[1]-b[1]*c[0], a[1]*c[0]-a[0]*c[1], a[0]*b[1]-a[1]*b[0]),
    )
    determinant = sum(matrix[0][i] * cofactor[i][0] for i in range(3))
    return tuple(tuple(v / determinant for v in row) for row in cofactor)


def decode(value):
    return value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4


def encode(value):
    return 12.92 * value if value <= 0.0031308 else 1.055 * value ** (1 / 2.4) - 0.055


def byte(value):
    return int(max(0, min(1, value)) * 255 + 0.5)


def convert(rgb, source, target):
    if source == target:
        return rgb
    xyz = mul(source, tuple(decode(v / 255) for v in rgb))
    return tuple(byte(encode(v)) for v in mul(inverse(target), xyz))


def fixed(value):
    return struct.pack(">i", round(value * 65536))


def p3_icc():
    def xyz(values):
        return b"XYZ \0\0\0\0" + b"".join(fixed(v) for v in values)
    tags = [(b"wtpt", xyz((0.9642, 1, 0.8249)))]
    for i, name in enumerate((b"rXYZ", b"gXYZ", b"bXYZ")):
        tags.append((name, xyz(mul(D65_TO_D50, tuple(row[i] for row in P3)))))
    trc = b"para\0\0\0\0\0\x03\0\0" + b"".join(
        fixed(v) for v in (2.4, 1 / 1.055, 0.055 / 1.055, 1 / 12.92, 0.04045)
    )
    tags += [(name, trc) for name in (b"rTRC", b"gTRC", b"bTRC")]
    offset = 132 + len(tags) * 12
    table, payload = bytearray(), bytearray()
    for name, data in tags:
        table.extend(name + struct.pack(">II", offset + len(payload), len(data)))
        payload.extend(data)
        payload.extend(b"\0" * (-len(payload) % 4))
    header = bytearray(128)
    header[:4] = struct.pack(">I", offset + len(payload))
    header[8:24] = b"\x04\x30\0\0mntrRGB XYZ "
    header[24:36] = struct.pack(">6H", 2026, 1, 1, 0, 0, 0)
    header[36:40] = b"acsp"
    header[64:68] = struct.pack(">I", 1)
    header[68:80] = b"".join(fixed(v) for v in (0.9642, 1, 0.8249))
    return bytes(header) + struct.pack(">I", len(tags)) + table + payload


def chunk(name, data):
    return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data))


def png(width, height, pixels, metadata=()):
    rows = b"".join(b"\0" + bytes(pixels[y*width*4:(y+1)*width*4]) for y in range(height))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
            + b"".join(chunk(name, data) for name, data in metadata)
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def fixtures():
    icc = p3_icc()
    iccp = (b"iCCP", b"Original Display P3\0\0" + zlib.compress(icc, 9))
    yield "p3.icc", icc
    yield "p3-alpha.png", png(1, 1, (128, 180, 40, 128), (iccp,))
    yield "p3-cicp.png", png(1, 1, (255, 0, 0, 255), ((b"cICP", bytes((12, 13, 0, 1))),))
    yield "linear.png", png(1, 1, (128, 128, 128, 255), ((b"gAMA", struct.pack(">I", 100000)),))
    yield "unsupported-hdr.png", png(1, 1, (128, 128, 128, 255), ((b"cICP", bytes((9, 16, 0, 1))),))
    yield "invalid-icc.png", png(1, 1, (1, 2, 3, 255), ((b"iCCP", b"Invalid\0\0" + zlib.compress(b"invalid ICC")),))
    for target, name in ((SRGB, "srgb"), (P3, "p3")):
        colors = [
            convert((255, 0, 0), SRGB, target),
            convert((255, 0, 0), P3, target),
            tuple((v * 128 + 255 * 127) // 255 for v in convert((128, 180, 40), P3, target)),
            (188, 188, 188),
        ]
        for scale in (1, 2):
            width, height = 80 * scale, 40 * scale
            pixels = bytearray()
            for y in range(height):
                for x in range(width):
                    tile = x // (20 * scale)
                    rgb = colors[tile] if y < 20 * scale else colors[(0, 1, 2, 1)[tile]]
                    pixels.extend((*rgb, 255))
            metadata = ((b"sRGB", b"\x01"),) if name == "srgb" else (iccp,)
            yield f"profile-{name}-{scale}.png", png(width, height, pixels, metadata)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent / "tests/fixtures/display"
    for name, data in fixtures():
        path = root / name
        if args.check:
            if not path.exists() or path.read_bytes() != data:
                raise SystemExit(f"stale display reference: {path}")
        else:
            root.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)


if __name__ == "__main__":
    main()
