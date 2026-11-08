"""Create synthetic, redistributable texture-cache fixtures (requires Pillow)."""
from pathlib import Path
from PIL import Image, ImageDraw
import io
import struct
import uuid

root = Path(__file__).resolve().parents[1] / "web/tests/fixtures/texturecache"
root.mkdir(parents=True, exist_ok=True)
entries = bytearray(struct.pack("<fI32sI", 1.71, 64, b"OpenJPEG synthetic fixture", 4))
headers = bytearray()
for i, name in enumerate(["red stripes", "blue circle", "green leaves", "yellow dots"]):
    image = Image.new("RGB", (128, 128), "white")
    draw = ImageDraw.Draw(image)
    if i == 0:
        for x in range(0, 128, 16): draw.rectangle((x, 0, x+7, 127), fill="red")
    elif i == 1:
        draw.ellipse((12, 12, 116, 116), fill="blue")
    elif i == 2:
        for x in range(10, 100, 30): draw.ellipse((x, 10, x+20, 110), fill="green")
    else:
        for x in range(8, 128, 25):
            for y in range(8, 128, 25): draw.ellipse((x, y, x+12, y+12), fill="#ddbd00")
    out = io.BytesIO()
    image.save(out, "JPEG2000", no_jp2=True)
    data = out.getvalue()
    ident = uuid.UUID(int=i+1)
    entries.extend(ident.bytes + struct.pack("<iiI", len(data), max(0, len(data)-600), 1700000000-i))
    headers.extend(data[:600].ljust(600, b"\0"))
    (root / "0").mkdir(exist_ok=True)
    (root / "0" / f"{ident}.texture").write_bytes(data[600:])
(root / "texture.entries").write_bytes(entries)
(root / "texture.cache").write_bytes(headers)
print("Generated four synthetic JPEG 2000 textures; no customer data.")
