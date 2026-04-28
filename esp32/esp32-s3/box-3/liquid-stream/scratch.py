from PIL import Image, ImageDraw, ImageFont

def get_char(char):
    img = Image.new('L', (16, 16), 0)
    draw = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype('/System/Library/Fonts/Hiragino Sans GB.ttc', 16)
    except Exception as e:
        try:
            font = ImageFont.truetype('/System/Library/Fonts/STHeiti Light.ttc', 16)
        except Exception as e:
            print("Failed to load font:", e)
            return [0] * 16
        
    draw.text((8, 6), char, font=font, fill=255, anchor="mm")
    
    data = list(img.getdata())
    res = []
    for y in range(16):
        row = 0
        s = ""
        for x in range(16):
            if data[y*16 + x] > 64:  # Threshold for antialiasing
                row |= (1 << (15 - x))
                s += "XX"
            else:
                s += ".."
        # print(s)
        res.append(row)
    return res

chars = "无限光河"
print("const CHARS: [[u16; 16]; 4] = [")
for c in chars:
    res = get_char(c)
    hex_res = [f"0x{r:04X}" for r in res]
    print(f"    [{', '.join(hex_res)}], // {c}")
print("];")
