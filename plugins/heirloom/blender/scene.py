"""Heirloom — a photoreal Blender (Cycles) render of the pedal.

    blender -b -P scene.py -- ASSETS OUT [hero|top] [samples]

ASSETS holds face_print.py's maps, the HDRI and the desk textures.
Geometry is src/pedal/mod.rs's, in points (1 pt = 0.3 mm), y down on
the face → world −Y.
"""

import math
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Vector

argv = sys.argv[sys.argv.index("--") + 1:]
ASSETS = Path(argv[0]).resolve()
OUT = Path(argv[1]).resolve()
VIEW = argv[2] if len(argv) > 2 else "hero"
SAMPLES = int(argv[3]) if len(argv) > 3 else 256

S = 0.0003  # metres per point
TOP = 0.034  # enclosure height
KNOBS = [(135, 178), (260, 178), (385, 178), (135, 400), (260, 400), (385, 400)]
KNOB_V = [0.55, 0.40, 0.50, 0.30, 0.62, 0.22]
TOGGLES = [(135, 292), (260, 292), (385, 292)]
TOGGLE_POS = [1, 0, 2]
FOOT = [((150, 588), (150, 528)), ((370, 588), (370, 528))]
LED_LIT = [True, False]


def P(x, y, z=TOP):
    return Vector(((x - 260) * S, -(y - 350) * S, z))


# ----------------------------------------------------------- scene

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.engine = "CYCLES"
prefs = bpy.context.preferences.addons["cycles"].preferences
try:
    prefs.compute_device_type = "METAL"
    prefs.get_devices()
    for d in prefs.devices:
        d.use = True
    sc.cycles.device = "GPU"
except Exception as e:  # noqa: BLE001
    print("GPU unavailable:", e)
print("devices:", [(d.name, d.type, d.use) for d in prefs.devices], sc.cycles.device)
sc.cycles.samples = SAMPLES
sc.cycles.use_denoising = True
sc.cycles.max_bounces = 12
sc.cycles.glossy_bounces = 6
sc.view_settings.view_transform = "AgX"
try:
    sc.view_settings.look = "AgX - Medium High Contrast"
except TypeError:
    pass


def mat(name, **kw):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    for k, v in kw.items():
        b.inputs[k].default_value = v
    return m, b


def image(path, colour=True):
    img = bpy.data.images.load(str(path))
    if not colour:
        img.colorspace_settings.name = "Non-Color"
    return img


def link(m, a, b):
    m.node_tree.links.new(a, b)


def node(m, kind, **kw):
    n = m.node_tree.nodes.new(kind)
    for k, v in kw.items():
        setattr(n, k, v)
    return n


def obj_from(bm, name, material, smooth=True):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    sc.collection.objects.link(o)
    o.data.materials.append(material)
    if smooth:
        for f in o.data.polygons:
            f.use_smooth = True
    return o


def add_mod_bevel(o, width, segs=3):
    b = o.modifiers.new("bevel", "BEVEL")
    b.width = width
    b.segments = segs
    b.limit_method = "ANGLE"
    b.angle_limit = math.radians(35)
    b.harden_normals = False
    return b


# ----------------------------------------------------------- materials

def powder_coat():
    m, b = mat("powder coat", **{"Roughness": 0.45, "Coat Weight": 0.15, "Coat Roughness": 0.25})
    uv = node(m, "ShaderNodeTexCoord")
    alb = node(m, "ShaderNodeTexImage", image=image(ASSETS / "face_albedo.png"))
    rgh = node(m, "ShaderNodeTexImage", image=image(ASSETS / "face_rough.png", False))
    hgt = node(m, "ShaderNodeTexImage", image=image(ASSETS / "face_height.png", False))
    for t in (alb, rgh, hgt):
        t.extension = "EXTEND"
        t.interpolation = "Cubic"
        link(m, uv.outputs["UV"], t.inputs["Vector"])
    link(m, alb.outputs["Color"], b.inputs["Base Color"])
    link(m, rgh.outputs["Color"], b.inputs["Roughness"])
    bump = node(m, "ShaderNodeBump")
    bump.inputs["Strength"].default_value = 0.35
    bump.inputs["Distance"].default_value = 0.00006
    link(m, hgt.outputs["Color"], bump.inputs["Height"])
    link(m, bump.outputs["Normal"], b.inputs["Normal"])
    # Vinyl reads as a deeper gloss: coat where the print is dark and smooth.
    inv = node(m, "ShaderNodeMath", operation="SUBTRACT")
    inv.inputs[0].default_value = 0.6
    link(m, rgh.outputs["Color"], inv.inputs[1])
    cw = node(m, "ShaderNodeMath", operation="MULTIPLY")
    cw.inputs[1].default_value = 2.5
    link(m, inv.outputs[0], cw.inputs[0])
    cl = node(m, "ShaderNodeClamp")
    link(m, cw.outputs[0], cl.inputs["Value"])
    link(m, cl.outputs[0], b.inputs["Coat Weight"])
    vin = node(m, "ShaderNodeTexImage", image=image(ASSETS / "face_vinyl.png", False))
    vin.extension = "EXTEND"
    link(m, uv.outputs["UV"], vin.inputs["Vector"])
    an = node(m, "ShaderNodeMath", operation="MULTIPLY")
    an.inputs[1].default_value = 0.9
    link(m, vin.outputs["Color"], an.inputs[0])
    link(m, an.outputs[0], b.inputs["Anisotropic"])
    geo = node(m, "ShaderNodeNewGeometry")
    sub = node(m, "ShaderNodeVectorMath", operation="SUBTRACT")
    sub.inputs[1].default_value = ((260 - 260) * S, -(680 - 350) * S, 0)
    link(m, geo.outputs["Position"], sub.inputs[0])
    cr = node(m, "ShaderNodeVectorMath", operation="CROSS_PRODUCT")
    cr.inputs[1].default_value = (0, 0, 1)
    link(m, sub.outputs[0], cr.inputs[0])
    nz = node(m, "ShaderNodeVectorMath", operation="NORMALIZE")
    link(m, cr.outputs[0], nz.inputs[0])
    link(m, nz.outputs[0], b.inputs["Tangent"])
    return m


def metal(name, rough, aniso=0.0, colour=(0.9, 0.9, 0.92, 1)):
    m, b = mat(name, **{"Base Color": colour, "Metallic": 1.0, "Roughness": rough})
    if aniso:
        b.inputs["Anisotropic"].default_value = aniso
        t = node(m, "ShaderNodeTangent", direction_type="RADIAL", axis="Z")
        link(m, t.outputs["Tangent"], b.inputs["Tangent"])
        # Lathe rings: fine concentric bump.
        tc = node(m, "ShaderNodeTexCoord")
        w = node(m, "ShaderNodeTexWave", wave_type="RINGS", rings_direction="SPHERICAL")
        w.inputs["Scale"].default_value = 900.0
        w.inputs["Distortion"].default_value = 2.0
        link(m, tc.outputs["Object"], w.inputs["Vector"])
        bp = node(m, "ShaderNodeBump")
        bp.inputs["Strength"].default_value = 0.08
        link(m, w.outputs["Fac"], bp.inputs["Height"])
        link(m, bp.outputs["Normal"], b.inputs["Normal"])
    return m


POWDER = powder_coat()
CHROME = metal("chrome", 0.06, colour=(0.93, 0.93, 0.95, 1))
NICKEL = metal("nickel", 0.18, colour=(0.85, 0.85, 0.86, 1))
SPUN = metal("spun aluminium", 0.22, aniso=0.85, colour=(0.91, 0.91, 0.93, 1))
SKIRT, _ = mat("knob plastic", **{"Base Color": (0.012, 0.012, 0.013, 1), "Roughness": 0.38, "Coat Weight": 0.15, "Coat Roughness": 0.2})
INKM, _ = mat("engraving", **{"Base Color": (0.01, 0.01, 0.01, 1), "Roughness": 0.7})
DOT, _ = mat("skirt dot", **{"Base Color": (0.92, 0.9, 0.85, 1), "Roughness": 0.5})


def lens(name, colour, lit):
    m, b = mat(name, **{"Base Color": colour, "Roughness": 0.05, "Transmission Weight": 0.6, "Coat Weight": 1.0})
    if lit:
        b.inputs["Emission Color"].default_value = colour
        b.inputs["Emission Strength"].default_value = 12.0
    return m


# ----------------------------------------------------------- enclosure

def enclosure():
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=(420 * S, 600 * S, TOP), verts=bm.verts)
    bmesh.ops.translate(bm, vec=(0, 0, TOP / 2), verts=bm.verts)
    vertical = [e for e in bm.edges if abs(e.verts[0].co.z - e.verts[1].co.z) > 1e-6]
    bmesh.ops.bevel(bm, geom=vertical, offset=20 * S, segments=16, profile=0.5, affect="EDGES")
    o = obj_from(bm, "enclosure", POWDER)
    add_mod_bevel(o, 0.0022, 6)
    bpy.context.view_layer.objects.active = o
    bpy.ops.object.modifier_apply(modifier="bevel")
    for f in o.data.polygons:
        f.use_smooth = True
    # Top-down UV: the print covers the pedal's top (x 50…470, y 50…650).
    me = o.data
    uv = me.uv_layers.new(name="UV")
    for f in me.polygons:
        top = f.normal.z > 0.25
        for li in f.loop_indices:
            co = me.vertices[me.loops[li].vertex_index].co
            if top:
                x, y = co.x / S + 260, -co.y / S + 350
                u, v = (x - 50) / 420, 1 - (y - 50) / 600
            else:
                u, v = 0.1, 0.5
            uv.data[li].uv = (min(max(u, 0.0), 1.0), min(max(v, 0.0), 1.0))
    return o


# ----------------------------------------------------------- parts

def cyl(name, r, h, z0, at, material, verts=64, top_bevel=0.0, smooth=True):
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=verts, radius1=r, radius2=r, depth=h)
    bmesh.ops.translate(bm, vec=(0, 0, h / 2), verts=bm.verts)
    o = obj_from(bm, name, material, smooth)
    o.location = (at.x, at.y, z0)
    if top_bevel:
        add_mod_bevel(o, top_bevel, 4)
    return o


def knob(i, cx, cy, v):
    c = P(cx, cy)
    # Knurled skirt: a tapered cylinder with 44 ridges, a rounded shoulder.
    r0, h = 27 * S, 0.0105
    bm = bmesh.new()
    rings = [(0.0, 1.0), (0.82, 1.0), (0.93, 0.97), (1.0, 0.86)]
    n = 176
    grid = []
    for z, k in rings:
        row = []
        for j in range(n):
            a = 2 * math.pi * j / n
            ridge = 1.0 - 0.025 * (0.5 + 0.5 * math.cos(44 * a)) if z < 0.95 else 1.0
            rr = r0 * k * ridge
            row.append(bm.verts.new((rr * math.cos(a), rr * math.sin(a), z * h)))
        grid.append(row)
    for a_row, b_row in zip(grid, grid[1:]):
        for j in range(n):
            bm.faces.new((a_row[j], a_row[(j + 1) % n], b_row[(j + 1) % n], b_row[j]))
    bm.faces.new(list(reversed(grid[0])))
    bm.faces.new(grid[-1])
    sk = obj_from(bm, f"knob {i} skirt", SKIRT)
    sk.location = c
    # The spun cap, slightly domed, chamfered rim.
    cap = cyl(f"knob {i} cap", 19.5 * S, 0.0022, c.z + h - 0.0004, c, SPUN, 96, 0.0006)
    cap.modifiers.new("sub", "SUBSURF").levels = 1
    # Indicator: an engraved line on the cap; a painted dot on the skirt shoulder.
    a = math.radians(135 + 270 * v)
    d = Vector((math.cos(a), -math.sin(a), 0))
    bpy.ops.mesh.primitive_cube_add(size=1)
    ln = bpy.context.object
    ln.name = f"knob {i} line"
    ln.scale = (12 * S, 0.0005, 0.0004)
    ln.location = c + d * (12 * S) + Vector((0, 0, h + 0.0022 - 0.0004 + 0.0001))
    ln.rotation_euler = (0, 0, math.atan2(d.y, d.x))
    ln.data.materials.append(INKM)
    dot = cyl(f"knob {i} dot", 0.00055, 0.0002, c.z + h * 0.995, c + d * (23.0 * S), DOT, 24)


def toggle(t, cx, cy, pos):
    c = P(cx, cy)
    cyl(f"toggle {t} washer", 0.0042, 0.0005, c.z, c, NICKEL, 48, 0.0001)
    nut = cyl(f"toggle {t} nut", 0.0039, 0.0022, c.z + 0.0005, c, NICKEL, 6, 0.0003)
    nut.rotation_euler.z = math.radians(30)
    cyl(f"toggle {t} bushing", 0.0026, 0.0045, c.z + 0.0027, c, NICKEL, 48, 0.0002)
    # Bat lever, tilted toward its position (0 = up the face).
    tilt = {0: 22, 1: 0, 2: -22}[pos]
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=40, radius1=0.0011, radius2=0.0017, depth=0.011)
    bmesh.ops.translate(bm, vec=(0, 0, 0.0055), verts=bm.verts)
    lever = obj_from(bm, f"toggle {t} lever", CHROME)
    tip = bpy.data.objects.new(f"toggle {t} tip", None)
    lever.location = c + Vector((0, 0, 0.0062))
    lever.rotation_euler.x = math.radians(-tilt)
    bpy.ops.mesh.primitive_uv_sphere_add(radius=0.0018, segments=32, ring_count=16)
    ball = bpy.context.object
    ball.name = f"toggle {t} ball"
    ball.data.materials.append(CHROME)
    for f in ball.data.polygons:
        f.use_smooth = True
    ball.parent = lever
    ball.location = (0, 0, 0.0111)
    del tip


def footswitch(k, fx, fy, lx, ly):
    c = P(fx, fy)
    cyl(f"foot {k} washer", 0.0075, 0.0006, c.z, c, NICKEL, 64, 0.0001)
    nut = cyl(f"foot {k} nut", 0.0074, 0.0032, c.z + 0.0006, c, CHROME, 6, 0.0005)
    nut.rotation_euler.z = math.radians(30)
    cyl(f"foot {k} bushing", 0.0048, 0.0042, c.z + 0.0038, c, CHROME, 64, 0.0003)
    # Domed button.
    bpy.ops.mesh.primitive_uv_sphere_add(radius=14.5 * S, segments=64, ring_count=32)
    b = bpy.context.object
    b.name = f"foot {k} button"
    b.scale.z = 0.55
    b.location = c + Vector((0, 0, 0.0074))
    b.data.materials.append(CHROME)
    for f in b.data.polygons:
        f.use_smooth = True
    cyl(f"foot {k} stem", 14.5 * S, 0.0034, c.z + 0.004, c, CHROME, 64)
    # LED: chrome bezel and a glass lens (lit or not).
    l = P(lx, ly)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=7.5 * S, radius2=6.2 * S, depth=0.0016)
    bmesh.ops.translate(bm, vec=(0, 0, 0.0008), verts=bm.verts)
    bz = obj_from(bm, f"led {k} bezel", CHROME)
    bz.location = l
    add_mod_bevel(bz, 0.0002, 2)
    bpy.ops.mesh.primitive_uv_sphere_add(radius=5.0 * S, segments=32, ring_count=16)
    ln = bpy.context.object
    ln.name = f"led {k} lens"
    ln.scale.z = 0.7
    ln.location = l + Vector((0, 0, 0.0016))
    for f in ln.data.polygons:
        f.use_smooth = True
    col = (1.0, 0.12, 0.04, 1) if k == 0 else (1.0, 0.55, 0.08, 1)
    ln.data.materials.append(lens(f"lens {k}", col, LED_LIT[k]))
    if LED_LIT[k]:
        pl = bpy.data.lights.new(f"led {k} glow", "POINT")
        pl.energy = 0.06
        pl.color = col[:3]
        pl.shadow_soft_size = 0.001
        lo = bpy.data.objects.new(f"led {k} glow", pl)
        lo.location = l + Vector((0, 0, 0.003))
        sc.collection.objects.link(lo)


def along(o, side):
    """Turn a part built along +Z to point out of a side wall (±X)."""
    o.rotation_euler = (0, math.radians(90 * side), 0)


def jacks():
    """¼" jacks on the side walls (input right, output left), each with
    a plugged cable running off across the desk; the DC jack on top."""
    rubber, rb = mat("cable rubber", **{"Base Color": (0.008, 0.008, 0.009, 1), "Roughness": 0.5, "Sheen Weight": 0.05})
    tc = node(rubber, "ShaderNodeTexCoord")
    nz = node(rubber, "ShaderNodeTexNoise")
    nz.inputs["Scale"].default_value = 4000.0
    link(rubber, tc.outputs["Object"], nz.inputs["Vector"])
    bp = node(rubber, "ShaderNodeBump")
    bp.inputs["Strength"].default_value = 0.05
    link(rubber, nz.outputs["Fac"], bp.inputs["Height"])
    link(rubber, bp.outputs["Normal"], rb.inputs["Normal"])
    y0, z0 = (350 - 170) * S, TOP * 0.5
    for side in (-1, 1):
        x0 = side * 210 * S
        def part(name, r, length, start, material, verts=48, bevel=0.0003, r2=None):
            bm = bmesh.new()
            bmesh.ops.create_cone(bm, cap_ends=True, segments=verts, radius1=r, radius2=r if r2 is None else r2, depth=length)
            bmesh.ops.translate(bm, vec=(0, 0, length / 2), verts=bm.verts)
            o = obj_from(bm, name, material, verts > 8)
            if bevel:
                add_mod_bevel(o, bevel, 3)
            along(o, side)
            o.location = (x0 + side * start, y0, z0)
            return o
        tag = "in" if side > 0 else "out"
        part(f"jack {tag} washer", 0.0068, 0.0006, 0.0, NICKEL, 64, 0.0001)
        nut = part(f"jack {tag} nut", 0.0066, 0.0026, 0.0006, NICKEL, 6, 0.0004)
        nut.rotation_euler.x = math.radians(30)
        # The plug: nickel collar, knurled chrome barrel, rubber boot.
        part(f"plug {tag} collar", 0.0045, 0.003, 0.0032, NICKEL, 48)
        bm = bmesh.new()
        n, rings = 120, 12
        L, R = 0.019, 0.0052
        grid = []
        for k in range(rings + 1):
            z = L * k / rings
            row = []
            for j in range(n):
                a = 2 * math.pi * j / n
                knurl = 1.0 - (0.03 * (0.5 + 0.5 * math.cos(30 * a)) if 2 <= k <= rings - 2 else 0.0)
                taper = 0.92 if k in (0, rings) else 1.0
                rr = R * knurl * taper
                row.append(bm.verts.new((rr * math.cos(a), rr * math.sin(a), z)))
            grid.append(row)
        for ra, rb2 in zip(grid, grid[1:]):
            for j in range(n):
                bm.faces.new((ra[j], ra[(j + 1) % n], rb2[(j + 1) % n], rb2[j]))
        bm.faces.new(list(reversed(grid[0])))
        bm.faces.new(grid[-1])
        barrel = obj_from(bm, f"plug {tag} barrel", CHROME)
        along(barrel, side)
        barrel.location = (x0 + side * 0.0062, y0, z0)
        part(f"plug {tag} boot", 0.0046, 0.022, 0.0252, rubber, 48, 0.0008, r2=0.0033)
        # The cable: leaves the boot straight, sags to the desk, runs off.
        cu = bpy.data.curves.new(f"cable {tag}", "CURVE")
        cu.dimensions = "3D"
        cu.bevel_depth = 0.0031
        cu.bevel_resolution = 6
        cu.resolution_u = 24
        sp = cu.splines.new("BEZIER")
        x1 = x0 + side * 0.0472
        desk_z = -0.002 + 0.0031
        pts = [
            (x1, y0, z0),
            (x1 + side * 0.040, y0 + 0.004, desk_z + 0.004),
            (x1 + side * 0.085, y0 + 0.045, desk_z),
            (x1 + side * 0.110, y0 + 0.160, desk_z),
            (x1 + side * 0.090, y0 + 0.420, desk_z),
        ]
        sp.bezier_points.add(len(pts) - 1)
        for bpt, co in zip(sp.bezier_points, pts):
            bpt.co = co
            bpt.handle_left_type = bpt.handle_right_type = "AUTO"
        first = sp.bezier_points[0]
        first.handle_left_type = first.handle_right_type = "ALIGNED"
        first.handle_right = (x1 + side * 0.022, y0, z0)
        first.handle_left = (x1 - side * 0.005, y0, z0)
        co = bpy.data.objects.new(f"cable {tag}", cu)
        sc.collection.objects.link(co)
        cu.materials.append(rubber)
    # DC jack, top wall.
    at = Vector((0, 300 * S, TOP * 0.55))
    o = cyl("dc jack", 0.0045, 0.0028, 0, Vector((0, 0, 0)), NICKEL, 48, 0.0003)
    o.rotation_euler.x = math.radians(-90)
    o.location = at
    h = cyl("dc jack hole", 0.0025, 0.003, 0, Vector((0, 0, 0)), INKM, 32)
    h.rotation_euler.x = math.radians(-90)
    h.location = at + Vector((0, 0.0002, 0))


def feet():
    for sx in (-1, 1):
        for sy in (-1, 1):
            at = Vector((sx * 0.052, sy * 0.078, 0))
            m, _ = mat("rubber", **{"Base Color": (0.02, 0.02, 0.02, 1), "Roughness": 0.8})
            cyl("foot", 0.006, 0.002, -0.002, at, m, 32, 0.0006)


# ----------------------------------------------------------- desk + light

def desk():
    bpy.ops.mesh.primitive_plane_add(size=1.2, location=(0, 0, -0.002))
    d = bpy.context.object
    d.name = "desk"
    m, b = mat("walnut", **{"Roughness": 0.55})
    tc = node(m, "ShaderNodeTexCoord")
    mp = node(m, "ShaderNodeMapping")
    mp.inputs["Scale"].default_value = (2.2, 2.2, 2.2)
    mp.inputs["Rotation"].default_value = (0, 0, math.radians(90))
    link(m, tc.outputs["UV"], mp.inputs["Vector"])
    di = node(m, "ShaderNodeTexImage", image=image(ASSETS / "dark_wood_diff_2k.jpg"))
    ro = node(m, "ShaderNodeTexImage", image=image(ASSETS / "dark_wood_rough_2k.jpg", False))
    no = node(m, "ShaderNodeTexImage", image=image(ASSETS / "dark_wood_nor_gl_2k.jpg", False))
    for t in (di, ro, no):
        link(m, mp.outputs["Vector"], t.inputs["Vector"])
    dark = node(m, "ShaderNodeHueSaturation")
    dark.inputs["Value"].default_value = 0.55
    link(m, di.outputs["Color"], dark.inputs["Color"])
    link(m, dark.outputs["Color"], b.inputs["Base Color"])
    link(m, ro.outputs["Color"], b.inputs["Roughness"])
    nm = node(m, "ShaderNodeNormalMap")
    link(m, no.outputs["Color"], nm.inputs["Color"])
    link(m, nm.outputs["Normal"], b.inputs["Normal"])
    d.data.materials.append(m)


def lights():
    w = bpy.data.worlds.new("world")
    sc.world = w
    w.use_nodes = True
    bg = next(n for n in w.node_tree.nodes if n.type == "BACKGROUND")
    env = w.node_tree.nodes.new("ShaderNodeTexEnvironment")
    env.image = bpy.data.images.load(str(ASSETS / "studio_small_08_2k.hdr"))
    mp = w.node_tree.nodes.new("ShaderNodeMapping")
    tc = w.node_tree.nodes.new("ShaderNodeTexCoord")
    mp.inputs["Rotation"].default_value = (0, 0, math.radians(140))
    w.node_tree.links.new(tc.outputs["Generated"], mp.inputs["Vector"])
    w.node_tree.links.new(mp.outputs["Vector"], env.inputs["Vector"])
    w.node_tree.links.new(env.outputs["Color"], bg.inputs["Color"])
    bg.inputs["Strength"].default_value = 0.25
    # Key: a large warm softbox, upper left (the plugin's key light).
    def area(name, loc, size, energy, colour, target=(0, 0, 0.02)):
        l = bpy.data.lights.new(name, "AREA")
        l.shape = "RECTANGLE"
        l.size, l.size_y = size
        l.energy = energy
        l.color = colour
        o = bpy.data.objects.new(name, l)
        o.location = loc
        sc.collection.objects.link(o)
        tr = o.constraints.new("TRACK_TO")
        e = bpy.data.objects.new(name + " target", None)
        e.location = target
        sc.collection.objects.link(e)
        tr.target = e
        return o
    area("key", (-0.32, 0.30, 0.45), (0.35, 0.25), 9, (1.0, 0.94, 0.86))
    area("fill", (0.40, -0.10, 0.25), (0.4, 0.4), 1.5, (0.85, 0.9, 1.0))
    area("rim", (0.10, 0.45, 0.12), (0.3, 0.06), 4, (1.0, 0.9, 0.8))


def camera():
    cam = bpy.data.cameras.new("cam")
    o = bpy.data.objects.new("cam", cam)
    sc.collection.objects.link(o)
    sc.camera = o
    target = bpy.data.objects.new("look", None)
    sc.collection.objects.link(target)
    tr = o.constraints.new("TRACK_TO")
    tr.target = target
    cam.dof.use_dof = True
    cam.dof.focus_object = target
    if VIEW == "top":
        cam.lens = 85
        o.location = (0.0, -0.05, 0.70)
        target.location = (0, 0.0, TOP)
        cam.dof.aperture_fstop = 22
        sc.render.resolution_x, sc.render.resolution_y = 1700, 1400
    else:
        cam.lens = 70
        o.location = (0.15, -0.36, 0.34)
        target.location = (0.004, -0.006, TOP)
        cam.dof.aperture_fstop = 11
        sc.render.resolution_x, sc.render.resolution_y = 1600, 1200
    sc.render.resolution_percentage = 100


enclosure()
for i, ((x, y), v) in enumerate(zip(KNOBS, KNOB_V)):
    knob(i, x, y, v)
for t, ((x, y), p) in enumerate(zip(TOGGLES, TOGGLE_POS)):
    toggle(t, x, y, p)
for k, ((f, l)) in enumerate(FOOT):
    footswitch(k, *f, *l)
jacks()
feet()
desk()
lights()
camera()
sc.render.filepath = str(OUT)
sc.render.image_settings.file_format = "PNG"
bpy.ops.wm.save_as_mainfile(filepath=str(OUT.with_suffix(".blend")))
bpy.ops.render.render(write_still=True)
print("rendered", OUT)
