"""左手/右手コレクションの全コントローラを、枠に自動フィットさせて同じ解像度でレンダリングする。

  blender -b ControllerRender.blend -P render.py              # 確認用 (Workbench) -> preview/
  blender -b ControllerRender.blend -P render.py -- --final   # 本番 (.blend の設定 = Cycles) -> renders/

.blend が source/ フォルダにある構成では、出力先はその 1 つ上 (--final は left/ right/ を直下に作る):
  blender -b source/ControllerRender.blend --python-exit-code 1 -P tools/render.py -- --final --size 2048

アプリ (apps/nyaw/crates/guide) は 2048 x 2048 を前提にする (表の座標の大きさを読むときに確かめる)。

各ボタン中心の画像上の座標と大きさ (radius) は、コントローラごとに buttons/<left|right>/<名前>.json に書く (button_map.py)。
--map-only は画像を描かずに JSON だけ作り直す。テクスチャは SteamVR のファイルを直接参照する。

コレクション内の MESH が対象。モデルを追加するならコレクションに入れるだけ。.blend は保存しない。
レンダリング中だけ、元の法線に沿って丸めた細分化メッシュに差し替える (--subdiv 0 で無効)。
元のメッシュは変更しない。
"""
import argparse
import math
import re
import sys
from contextlib import contextmanager
from pathlib import Path

import bpy
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
import button_map

SIDES = {"左手": "left", "右手": "right"}

p = argparse.ArgumentParser()
p.add_argument("--final", action="store_true")
p.add_argument("--size", type=int, help="正方形の一辺 (px)。省略時は .blend の解像度")
p.add_argument("--margin", type=float, default=0.05, help="長辺側の片側余白 (画像に対する割合)")
p.add_argument("--out", help="出力先フォルダ。省略時は preview/ (--final は renders/。source/ 構成では 1 つ上)")
p.add_argument("--map-only", action="store_true", help="画像を描かず buttons/ の JSON だけ作る")
p.add_argument("--only", default="", help="名前にこの文字列を含むモデルだけ")
p.add_argument("--subdiv", type=int, default=2, help="細分化レベル。辺を 2**N 分割する (0 で無効)")
p.add_argument("--bend", type=float, default=30, help="辺を丸める上限角度 (度)。超える辺は直線のまま")
args = p.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])

scene = bpy.context.scene
r = scene.render

# テクスチャは SteamVR を直接参照する。欠けるとピンク色のまま出力されるので先に止める
missing = sorted({bpy.path.abspath(i.filepath) for i in bpy.data.images
                  if i.users and i.source == "FILE" and not i.packed_file
                  and not Path(bpy.path.abspath(i.filepath)).is_file()})
assert not missing, f"テクスチャが見つからない (SteamVR の場所を確認): {missing}"
if args.size:
    r.resolution_x = r.resolution_y = args.size
r.resolution_percentage = 100
r.film_transparent = True
r.image_settings.file_format = "PNG"
r.image_settings.color_mode = "RGBA"
if not args.final:
    r.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "TEXTURE"
    # Workbench はアクティブな画像ノードを表示するので、Base Color のテクスチャに合わせる
    for mat in bpy.data.materials:
        if mat.node_tree:
            for link in mat.node_tree.links:
                if link.to_socket.name == "Base Color" and link.from_node.type == "TEX_IMAGE":
                    mat.node_tree.nodes.active = link.from_node


def use_gpu():
    """使える GPU があれば選ぶ。無ければ CPU。ユーザー設定は保存しない。"""
    prefs = bpy.context.preferences.addons["cycles"].preferences
    available = {t[0] for t in prefs.get_device_types(bpy.context)}
    for kind in ("OPTIX", "CUDA", "HIP", "ONEAPI", "METAL"):
        if kind in available and prefs.get_devices_for_type(kind):
            prefs.compute_device_type = kind
            prefs.refresh_devices()
            for d in prefs.devices:
                d.use = d.type == kind
            scene.cycles.device = "GPU"
            print(f"Cycles: GPU ({kind})")
            return
    scene.cycles.device = "CPU"
    print("Cycles: 対応 GPU が無いので CPU")


if args.final and r.engine == "CYCLES":
    use_gpu()
W, H = r.resolution_x, r.resolution_y
blend_dir = Path(bpy.data.filepath).parent
# buttons/ は本番画像の隣に置くので、--map-only も本番の出力先を使う
production = args.final or args.map_only
if blend_dir.name == "source":
    out_root = blend_dir.parent / (args.out or ("." if production else "preview"))
else:
    out_root = blend_dir / (args.out or ("renders" if production else "preview"))


def fit(cam, obj):
    """カメラの向きはそのまま、ortho_scale と shift で obj を中央に収める。"""
    dg = bpy.context.evaluated_depsgraph_get()
    ev = obj.evaluated_get(dg)
    to_cam = cam.evaluated_get(dg).matrix_world.normalized().inverted() @ ev.matrix_world
    # bound_box ではなく実頂点を使う (回転したモデルでも余白がぶれない)
    pts = np.array([(to_cam @ v.co)[:2] for v in ev.to_mesh().vertices])
    ev.to_mesh_clear()
    lo, hi = pts.min(0), pts.max(0)
    # sensor_fit AUTO: ortho_scale と shift はどちらも画像の長辺が基準
    scale = max((hi - lo) * max(W, H) / (W, H)) / (1 - 2 * args.margin)
    cam.data.ortho_scale = scale
    cam.data.shift_x, cam.data.shift_y = (lo + hi) / 2 / scale


def pn_mesh(src, segments, max_bend):
    """元の法線に沿って面を丸めた新しいメッシュを作る (PN Triangles)。src は読むだけ。

    元の頂点位置は動かさず、法線と UV は元の値をそのまま補間するので陰影は元と一致する。
    """
    src.calc_loop_triangles()
    tris = src.loop_triangles
    co = np.array([v.co for v in src.vertices])
    tri_v = np.array([t.vertices for t in tris])
    tri_l = np.array([t.loops for t in tris])
    pos = co[tri_v]                                                       # (T, 3, 3)
    nor = np.array([n.vector for n in src.corner_normals])[tri_l]          # (T, 3, 3)

    # 同じ位置の頂点は同じ点とみなす。辺の曲線を位置で共有するので隙間が開かない
    _, weld = np.unique(co.round(6), axis=0, return_inverse=True)
    wid = weld.reshape(-1)[tri_v]
    edge_normals = {}
    for t in range(len(tri_v)):
        for i, j in ((0, 1), (1, 2), (2, 0)):
            a, b = (i, j) if wid[t, i] < wid[t, j] else (j, i)
            ends = edge_normals.setdefault((wid[t, a], wid[t, b]), ([], []))
            ends[0].append(nor[t, a])
            ends[1].append(nor[t, b])

    min_cos = math.cos(math.radians(max_bend))

    def control(p0, p1, normals):
        """辺の p0 側の制御点。角の辺は両側の接平面の交線に沿わせる。"""
        d = p1 - p0
        normals = np.array(normals)
        dots = normals @ normals.T
        i, j = np.unravel_index(dots.argmin(), dots.shape)
        if dots[i, j] > 0.996:                      # 約 5 度以内: 滑らかな辺
            n = normals.sum(0)
            n /= np.linalg.norm(n)
            tangent = d - d.dot(n) * n
        else:
            line = np.cross(normals[i], normals[j])
            size = np.linalg.norm(line)
            # 法線が正反対 (裏表 2 枚の面) だと交線が決まらないので直線にする
            tangent = d.dot(line / size) * line / size if size > 1e-6 else d
        # 辺から大きく離れる向きは膨らみの原因になるので直線のままにする
        length = np.linalg.norm(tangent) * np.linalg.norm(d)
        if length < 1e-12 or tangent.dot(d) / length < min_cos:
            tangent = d
        return p0 + tangent / 3

    first = np.zeros(weld.max() + 1, dtype=int)
    first[weld.reshape(-1)] = np.arange(len(co))
    point = co[first]
    curves = {
        (a, b): (control(point[a], point[b], ends[0]), control(point[b], point[a], ends[1]))
        for (a, b), ends in edge_normals.items()
    }

    # 3 次ベジェ三角形の制御点: 角 3 + 辺 6 + 中央 1
    ctrl = np.empty((len(tri_v), 10, 3))
    ctrl[:, :3] = point[wid]
    for t in range(len(tri_v)):
        for k, (i, j) in enumerate(((0, 1), (1, 2), (2, 0))):
            a, b = wid[t, i], wid[t, j]
            near_a, near_b = curves[(a, b)] if a < b else curves[(b, a)][::-1]
            ctrl[t, 3 + 2 * k] = near_a
            ctrl[t, 4 + 2 * k] = near_b
    edge_avg = ctrl[:, 3:9].mean(1)
    ctrl[:, 9] = edge_avg + (edge_avg - ctrl[:, :3].mean(1)) / 2

    n = segments
    grid = [(i, j) for i in range(n + 1) for j in range(n + 1 - i)]
    index = {ij: k for k, ij in enumerate(grid)}
    bary = np.array([((n - i - j) / n, i / n, j / n) for i, j in grid])    # (G, 3)
    u, v, w = bary.T
    basis = np.stack([
        u**3, v**3, w**3,
        3 * u**2 * v, 3 * u * v**2,
        3 * v**2 * w, 3 * v * w**2,
        3 * w**2 * u, 3 * w * u**2,
        6 * u * v * w], axis=1)                                            # (G, 10)
    cells = []
    for i in range(n):
        for j in range(n - i):
            cells.append((index[i, j], index[i + 1, j], index[i, j + 1]))
            if i + j < n - 1:
                cells.append((index[i + 1, j], index[i + 1, j + 1], index[i, j + 1]))
    cells = np.array(cells)

    verts = np.einsum("gc,tcx->tgx", basis, ctrl).reshape(-1, 3)
    normals = np.einsum("gc,tcx->tgx", bary, nor).reshape(-1, 3)
    normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-12)
    faces = (cells[None] + (np.arange(len(tri_v)) * len(grid))[:, None, None]).reshape(-1, 3)

    assert np.isfinite(verts).all(), f"{src.name}: 細分化で不正な座標が出た"
    out = bpy.data.meshes.new(src.name + "_render")
    out.from_pydata(verts.tolist(), [], faces.tolist())
    for mat in src.materials:
        out.materials.append(mat)
    material = np.repeat([t.material_index for t in tris], len(cells))
    out.polygons.foreach_set("material_index", material.astype(np.int32))
    for layer in src.uv_layers:
        uv = np.array([d.vector for d in layer.uv])[tri_l]                     # (T, 3, 2)
        uv = np.einsum("gc,tcx->tgx", bary, uv).reshape(-1, 2)
        new_layer = out.uv_layers.new(name=layer.name)
        new_layer.uv.foreach_set("vector", uv[faces.reshape(-1)].astype(np.float32).ravel())
        new_layer.active_render = layer.active_render
    out.polygons.foreach_set("use_smooth", np.ones(len(faces), dtype=bool))
    out.normals_split_custom_set_from_vertices(normals.tolist())
    return out


@contextmanager
def subdivided(obj):
    """レンダリング中だけ細分化したメッシュに差し替える。抜けると元のメッシュに戻る。"""
    if not args.subdiv:
        yield
        return
    original = obj.data
    obj.data = pn_mesh(original, 2 ** args.subdiv, args.bend)
    try:
        yield
    finally:
        temp, obj.data = obj.data, original
        bpy.data.meshes.remove(temp)


def check(path):
    """出力の検証: 解像度が揃っていて、モデルが余白の位置ぴったりに収まっている。"""
    img = bpy.data.images.load(str(path))
    assert tuple(img.size) == (W, H), f"{path.name}: size {tuple(img.size)}"
    px = np.empty(W * H * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    bpy.data.images.remove(img)
    ys, xs = np.nonzero(px[3::4].reshape(H, W) > 0.5)
    assert len(xs), f"{path.name}: 何も写っていない"
    gaps = np.array([xs.min() / W, (W - 1 - xs.max()) / W, ys.min() / H, (H - 1 - ys.max()) / H])
    tol = 3 / min(W, H)
    assert gaps.min() >= args.margin - tol, f"{path.name}: はみ出し gaps={gaps.round(3)}"
    assert min(gaps[:2].max(), gaps[2:].max()) <= args.margin + tol, f"{path.name}: 小さすぎ gaps={gaps.round(3)}"


done = 0
for col_name, side in SIDES.items():
    objs = bpy.data.collections[col_name].objects
    cam = next(o for o in objs if o.type == "CAMERA")
    scene.camera = cam
    for target in [o for o in objs if o.type == "MESH" and args.only in o.name]:
        # 対象モデルと、同じ側のライトだけを残す
        for o in scene.objects:
            if o.type in {"MESH", "LIGHT"}:
                o.hide_render = not (o == target or (o.type == "LIGHT" and o.name in objs))
        path = out_root / side / (re.sub(r"\.\d+$", "", target.name) + ".png")
        r.filepath = str(path)
        original = target.data
        with subdivided(target):
            fit(cam, target)
            dg = bpy.context.evaluated_depsgraph_get()
            proj = button_map.Projection(
                cam.evaluated_get(dg).matrix_world.copy(), target.matrix_world.copy(),
                cam.data.ortho_scale, (cam.data.shift_x, cam.data.shift_y), W, H)
            if not args.map_only:
                bpy.ops.render.render(write_still=True)
        button_map.write(out_root, button_map.controller(path.stem, side, original, proj), W, H)
        if not args.map_only:
            check(path)
        print(f"OK {side}/{path.name}", flush=True)
        done += 1

assert done, "対象モデルが無い"
print(f"DONE {done} images {W}x{H} -> {out_root}")
