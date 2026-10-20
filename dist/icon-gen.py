#!/usr/bin/env python3
"""Genera l'icona di Whats-Open: logo WhatsApp monocromatico, fumetto aperto."""
import math
import sys
from PIL import Image, ImageDraw

S = 2048          # canvas di lavoro
OUT = 1024        # dimensione finale
BG = (8, 8, 8, 255)
FG = (245, 245, 245, 255)


def phone_mask(size, radius, stroke, pad, angle):
    """Cornetta del telefono bianca su trasparente, ruotata."""
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    cx = cy = size / 2
    box = [cx - radius, cy - radius, cx + radius, cy + radius]
    start, end = 145, 285
    d.arc(box, start=start, end=end, fill=FG, width=int(stroke))
    for deg in (start, end):
        a = math.radians(deg)
        px, py = cx + radius * math.cos(a), cy + radius * math.sin(a)
        d.ellipse([px - pad, py - pad, px + pad, py + pad], fill=FG)
    return layer.rotate(angle, resample=Image.BICUBIC, center=(cx, cy))


def build(size, filled, radius, stroke, pad, angle, phone_scale):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m = size * 0.025
    d.rounded_rectangle([m, m, size - m, size - m], radius=size * 0.235, fill=BG)

    cx, cy = size * 0.522, size * 0.472
    rad = size * 0.243
    tail_stroke = size * 0.040

    # fumetto: disco pieno (o anello) + coda
    if filled:
        d.ellipse([cx - rad, cy - rad, cx + rad, cy + rad], fill=FG)
    else:
        d.ellipse([cx - rad, cy - rad, cx + rad, cy + rad], outline=FG, width=int(tail_stroke))

    a_out, a_in = math.radians(110), math.radians(164)
    k = 0.985
    out = (cx + rad * k * math.cos(a_out), cy + rad * k * math.sin(a_out))
    inn = (cx + rad * k * math.cos(a_in), cy + rad * k * math.sin(a_in))
    tip = (cx - rad * 1.02, cy + rad * 1.00)
    if filled:
        # include il centro: il triangolo copre la corda, niente intaccatura
        d.polygon([out, tip, inn, (cx, cy)], fill=FG)

    layer = phone_mask(size, size * radius, size * stroke, size * pad, angle)
    layer = layer.resize((int(size * phone_scale), int(size * phone_scale)), Image.LANCZOS)
    off = (int(cx - layer.size[0] / 2), int(cy - layer.size[1] / 2))
    if filled:
        # foro: sottraggo il telefono dal fumetto bianco
        mask = layer.split()[3]
        plate = Image.new("RGBA", img.size, (0, 0, 0, 0))
        plate.paste((0, 0, 0, 255), off, mask)
        img.alpha_composite(plate)
    else:
        img.alpha_composite(layer, off)
    return img


VARIANTS = [
    (True, 0.150, 0.050, 0.048, -45, 0.95),
    (True, 0.155, 0.046, 0.046, -40, 0.92),
    (True, 0.145, 0.054, 0.050, -50, 0.98),
    (False, 0.150, 0.050, 0.046, -45, 0.95),
]

if __name__ == "__main__":
    if sys.argv[1] == "grid":
        sheet = Image.new("RGBA", (680, 680), (58, 58, 58, 255))
        for i, v in enumerate(VARIANTS):
            img = build(S, *v).resize((320, 320), Image.LANCZOS)
            sheet.alpha_composite(img, ((i % 2) * 340 + 10, (i // 2) * 340 + 10))
        sheet.save("/tmp/grid.png")
        print("ok")
    else:
        img = build(S, True, 0.152, 0.048, 0.047, -45, 0.95).resize((OUT, OUT), Image.LANCZOS)
        img.save(sys.argv[1])
        print("scritto", sys.argv[1])
        sizes = [16, 24, 32, 48, 64, 128, 256, 512]
        for size in sizes:
            small = img.resize((size, size), Image.LANCZOS)
            small.save(f"dist/icons/whats-open-{size}.png")
        img.resize((256, 256), Image.LANCZOS).save("res/icon-256.png")
        img.resize((64, 64), Image.LANCZOS).save("res/icon-64.png")
        print("icone derivate ok")
