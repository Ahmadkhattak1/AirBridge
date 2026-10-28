"""Generate AirBridge's original vector-style icon and Windows resources."""
from pathlib import Path
import io
import struct
import math
from PIL import Image, ImageDraw

root = Path(__file__).resolve().parents[1]
assets = root / 'crates/airbridge-app/assets'
assets.mkdir(exist_ok=True)
im = Image.new('RGBA', (1024, 1024))
d = ImageDraw.Draw(im)
d.rounded_rectangle((24, 24, 1000, 1000), radius=252, fill='#276ae8')
def stroke(points, width=64):
    points = [(int((x-1.778175)*32+512), int((y+1.778175)*32+512)) for x,y in points]
    d.line(points, fill='white', width=width, joint='curve')
    for x,y in (points[0], points[-1]):
        d.ellipse((x-width/2,y-width/2,x+width/2,y+width/2), fill='white')
for radius in (8,14):
    stroke([(-6+radius*math.cos(-math.pi/2*(1-i/96)),6+radius*math.sin(-math.pi/2*(1-i/96))) for i in range(97)],58)
for x,y in [(-6,6),(-6+22/math.sqrt(2),6-22/math.sqrt(2))]:
    cx,cy=(x-1.778175)*32+512,(y+1.778175)*32+512
    d.ellipse((cx-61,cy-61,cx+61,cy+61),fill='white')
im.resize((256,256),Image.Resampling.LANCZOS).save(assets/'airbridge.png')
small=im.resize((64,64),Image.Resampling.LANCZOS)
(assets/'airbridge-64.rgba').write_bytes(small.tobytes())
images=[]
for size in [16,24,32,48,64,128,256]:
    buf=io.BytesIO()
    im.resize((size,size),Image.Resampling.LANCZOS).save(buf,format='PNG')
    images.append((size,buf.getvalue()))
ico=struct.pack('<HHH',0,1,len(images));offset=6+16*len(images)
for size,png in images:
    ico+=struct.pack('<BBBBHHII',size%256,size%256,0,0,1,32,len(png),offset)
    offset+=len(png)
ico+=b''.join(png for _,png in images)
(assets/'airbridge.ico').write_bytes(ico)
def res(type_id, name_id, data):
    header=struct.pack('<IIHHHHIHHII',len(data),32,65535,type_id,65535,name_id,0,0x1030,0x409,0,0)
    return header+data+b'\0'*((-len(data))%4)
resources=struct.pack('<IIHHHHIHHII',0,32,65535,0,65535,0,0,0,0,0,0)
group=struct.pack('<HHH',0,1,len(images))
for index,(size,png) in enumerate(images,1):
    resources+=res(3,index,png)
    group+=struct.pack('<BBBBHHIH',size%256,size%256,0,0,1,32,len(png),index)
resources+=res(14,1,group)
(assets/'airbridge.res').write_bytes(resources)
print('Generated icon at 16–256 px, RGBA window icon and Windows executable resources.')
