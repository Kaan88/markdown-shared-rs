# Writes the JPEG test pictures of mdshared_text::image. Run with Pillow from this folder:
#   python gen.py
# The pictures are made here, so they carry no one else's licence.
from PIL import Image

def halves(w, h):
    im = Image.new("RGB", (w, h))
    for y in range(h):
        for x in range(w):
            im.putpixel((x, y), (220, 30, 30) if x < w // 2 else (30, 30, 220))
    return im

base = halves(16, 8)
base.save("halves.jpg", quality=95, subsampling=0)
base.save("halves-progressive.jpg", quality=95, subsampling=0, progressive=True)
base.save("halves-420.jpg", quality=95, subsampling=2)
Image.new("L", (8, 8), 128).save("gray.jpg", quality=95)
# Stored turned a quarter anticlockwise, with EXIF orientation 6 ("turn clockwise to show"):
# shown right, it is the 16 x 8 picture above.
exif = Image.Exif()
exif[0x0112] = 6
base.rotate(90, expand=True).save("halves-orient6.jpg", quality=95, subsampling=0, exif=exif)
