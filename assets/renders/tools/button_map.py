"""SteamVR のレンダーモデル部品から、各ボタンの中心がレンダー画像のどの画素かを求める。

render.py から呼ぶ。ボタンの形状は SteamVR の部品 OBJ、入力名は入力プロファイルが正本で、
手で座標を打つ場所はない。座標は画像の左上が原点。
"""
import json
import os
import re
from pathlib import Path

import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

STEAMVR = Path(os.environ.get("STEAMVR_DIR", r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR"))

# .blend のオブジェクト名 (= SteamVR が報告するレンダーモデル名) の接頭辞 -> 入力プロファイル
PROFILES = {
    "valve_controller_knu": "drivers/indexcontroller/resources/input/index_controller_profile.json",
    "vr_controller_vive": "drivers/htc/resources/input/vive_controller_profile.json",
    "oculus_": "drivers/oculus/resources/input/touch_profile.json",
    "frame_controller": "drivers/frame_controller/resources/input/frame_controller_profile.json",
    "pico_": "drivers/vrlink/resources/input/pico_controller_profile.json",
    "vive_focus3": "drivers/vrlink/resources/input/vive_focus3_controller_profile.json",
}
# 名前と形状の生成元が違うもの。Index の製品版は knu_1_0 を名乗るが、.blend の形状は EV2
SOURCE_MODEL = {f"valve_controller_knu_1_0_{h}": f"valve_controller_knu_ev2_0_{h}" for h in ("left", "right")}
# 同じ画像を使えるレンダーモデル名 (Rift S と Quest 1 は形状が同じ)
ALIASES = {
    **{name: [source] for name, source in SOURCE_MODEL.items()},
    **{f"oculus_rifts_controller_{h}": [f"oculus_quest_controller_{h}"] for h in ("left", "right")},
}

# 部品 OBJ のファイル名 (モデル名の接頭辞を除く) -> ボタン id。ここに無い部品はボタンではない
PARTS = {
    "button_a": "a", "a_button": "a", "button_b": "b", "b_button": "b",
    "button_x": "x", "x_button": "x", "button_y": "y", "y_button": "y",
    "button_enter": "menu", "button_menu": "menu", "menu_button": "menu",
    "button_system": "system", "system_button": "system", "sys_button": "system", "button_home": "system",
    "view_button": "view", "bumper": "bumper", "trigger": "trigger",
    "grip": "grip", "button_grip": "grip", "grip_button": "grip", "l_grip": "grip_left", "r_grip": "grip_right",
    "thumbstick": "thumbstick", "trackpad": "trackpad", "dpad": "dpad",
}
# 上面の中心を取る部品。それ以外 (trigger, grip, bumper) は見えている面の中心を取る
TOP_FACE = {"a", "b", "x", "y", "menu", "view", "system", "thumbstick", "trackpad", "dpad"}
NOT_PHYSICAL = ("pinch", "finger/")


def load_json(path):
    """SteamVR の JSON はコメントと末尾カンマを含む。"""
    text = Path(path).read_text(encoding="utf-8-sig", errors="replace")
    text = re.sub(r'("(?:\\.|[^"\\])*")|//[^\n]*|/\*.*?\*/', lambda m: m.group(1) or "", text, flags=re.S)
    return json.loads(re.sub(r",(\s*[}\]])", r"\1", text))


def model_dir(name):
    for pattern in ("resources/rendermodels", "drivers/*/resources/rendermodels"):
        for base in STEAMVR.glob(pattern):
            if (base / name).is_dir():
                return base / name
    raise FileNotFoundError(f"SteamVR にレンダーモデルが無い: {name}")


def read_obj(path):
    verts, faces = [], []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("v "):
            verts.append([float(v) for v in line.split()[1:4]])
        elif line.startswith("f "):
            ids = [int(v.split("/")[0]) for v in line.split()[1:]]
            ids = [i - 1 if i > 0 else len(verts) + i for i in ids]
            faces += [(ids[0], ids[k], ids[k + 1]) for k in range(1, len(ids) - 1)]
    return np.array(verts), np.array(faces)


def parts(name):
    """モデルのボタン部品を {id: (OBJ パス, motion)} で返す。"""
    source = SOURCE_MODEL.get(name, name)
    folder = model_dir(source)
    found = {}
    for component in load_json(folder / f"{source}.json")["components"].values():
        filename = component.get("filename")
        if not filename:
            continue
        stem = Path(filename).stem.removeprefix(source + "_")
        # Vive の button.obj はアプリケーションメニュー
        part = "menu" if stem == "button" and name.startswith("vr_controller_vive") else PARTS.get(stem)
        if part:
            found[part] = (folder / filename, component.get("motion") or {})
    if name.startswith("valve_controller_knu"):
        # EV2 には握り込みの部品が無いので、製品版の位置を借りる (形状が少し違うので近似)
        found["grip"] = (model_dir(name) / "gripsqueeze.obj", {"approximate": True})
    return found


def unit(v):
    return v / np.linalg.norm(v)


class Projection:
    """render.py の fit() と同じカメラ計算。OBJ 座標 = メッシュのローカル座標。"""

    def __init__(self, cam_matrix, obj_matrix, ortho_scale, shift, width, height):
        self.to_cam = np.array(cam_matrix.normalized().inverted() @ obj_matrix)
        self.scale, self.shift = ortho_scale, np.array(shift)
        self.width, self.height = width, height
        # カメラの方向 (メッシュのローカル座標)
        toward = obj_matrix.inverted().to_3x3() @ cam_matrix.to_3x3().col[2]
        self.view = unit(np.array(toward))

    def pixels(self, points):
        p = np.atleast_2d(points) @ self.to_cam[:3, :3].T + self.to_cam[:3, 3]
        n = (p[:, :2] / self.scale - self.shift) * max(self.width, self.height)
        return np.stack([self.width / 2 + n[:, 0], self.height / 2 - n[:, 1]], axis=1)


def visible(points, bvh, view):
    """各点からカメラへ向かって何にも当たらなければ見えている。1 mm 以内の自己交差は無視する。"""
    direction = Vector(view)
    return np.array([bvh.ray_cast(Vector(p + view * 0.001), direction)[0] is None for p in points], dtype=bool)


def surface(verts, faces):
    tri = verts[faces]
    cross = np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0])
    area = np.linalg.norm(cross, axis=1)
    keep = area > 1e-14
    tri, cross, area = tri[keep], cross[keep], area[keep]
    # 面の中だけでなく角の近くにも点を置き、大きい面でも偏らないようにする
    weights = np.array([[1 / 3, 1 / 3, 1 / 3], [0.7, 0.15, 0.15], [0.15, 0.7, 0.15], [0.15, 0.15, 0.7]])
    samples = np.einsum("wk,tkx->twx", weights, tri)
    return tri, cross / area[:, None], area / 2, samples


def top_face(part, verts, faces, motion, view):
    """押し込み軸を求め、上面の中心 (3D)、上面のサンプル点と重み、軸を返す。"""
    tri, normal, area, samples = surface(verts, faces)
    outward = (normal * area[:, None]).sum(0)
    axis = None
    if part == "thumbstick" and motion.get("center"):
        axis = unit(verts.mean(0) - np.array(motion["center"], dtype=float))
    elif np.linalg.norm(outward) > 0.05 * area.sum():
        axis = unit(outward)
    if axis is None:
        axis = view
    if axis.dot(view) < 0:
        axis = -axis
    facing = normal @ axis > 0.6
    height = samples[:, 0] @ axis
    depth = 0.0035 if part in ("thumbstick", "dpad") else 0.0025
    cap = facing & (height > height[facing].max() - depth)
    u = unit(np.cross(axis, [1.0, 0, 0]))
    v = np.cross(axis, u)
    flat = np.stack([tri[cap].reshape(-1, 3) @ u, tri[cap].reshape(-1, 3) @ v], axis=1)
    mid = (flat.min(0) + flat.max(0)) / 2
    center = u * mid[0] + v * mid[1] + axis * np.average(height[cap], weights=area[cap])
    return center, samples[cap].reshape(-1, 3), np.repeat(area[cap] / 4, 4), axis


def point(center, samples, weights, bvh, proj, always_snap):
    """中心が隠れていれば、見えているサンプルのうち中心に最も近い点へ寄せる。"""
    seen = visible(samples, bvh, proj.view)
    ratio = float(weights[seen].sum() / weights.sum()) if weights.sum() else 0.0
    center_seen = bool(visible([center], bvh, proj.view)[0])
    target = proj.pixels(center)[0]
    adjusted = False
    if seen.any() and (always_snap or not center_seen):
        screen = proj.pixels(samples[seen])
        if always_snap:
            target = np.average(screen, axis=0, weights=weights[seen])
        target_seen = screen[np.linalg.norm(screen - target, axis=1).argmin()]
        adjusted = not always_snap
        target = target_seen
    return {
        "x": round(float(target[0]), 1), "y": round(float(target[1]), 1),
        "u": round(float(target[0]) / proj.width, 5), "v": round(float(target[1]) / proj.height, 5),
        "visible": bool(seen.any() or center_seen), "visibility": round(ratio, 3),
        **({"adjusted": True} if adjusted else {}),
    }


def visible_face(verts, faces, bvh, proj):
    tri, normal, area, samples = surface(verts, faces)
    toward = normal @ proj.view
    front = toward > 0.15
    points = samples[front].reshape(-1, 3)
    weights = np.repeat((area * toward)[front] / 4, 4)
    center = (tri.mean(1) * area[:, None]).sum(0) / area.sum()
    return point(center, points, weights, bvh, proj, always_snap=True)


def dpad_arms(center, samples, axis):
    """十字キーの 4 方向。上はコントローラの前方 (-Z)、実際の腕の向きは形状に合わせる。"""
    up = np.array([0, 0, -1.0])
    up = unit(up - up.dot(axis) * axis)
    right = np.cross(up, axis)
    if right[0] < 0:
        right = -right
    offset = samples - center
    px, py = offset @ right, offset @ up
    radius, angle = np.hypot(px, py), np.arctan2(py, px)
    # 十字は 90 度ごとに対称なので、半径の高次モーメントの位相が腕の向きになる
    phase = np.angle((radius**8 * np.exp(4j * angle)).sum()) / 4
    arms = {}
    for name, nominal in (("right", 0), ("up", 90), ("left", 180), ("down", 270)):
        a = np.radians(nominal)
        a = phase + np.round((a - phase) / (np.pi / 2)) * (np.pi / 2)
        direction = right * np.cos(a) + up * np.sin(a)
        reach = (offset @ direction).max()
        arms[name] = center + direction * reach * 0.65
    return arms


def controller(name, hand, mesh, proj):
    """1 台ぶんのボタン座標。mesh は細分化前の元のメッシュ (遮蔽の判定に使う)。"""
    bvh = BVHTree.FromPolygons([v.co for v in mesh.vertices], [list(p.vertices) for p in mesh.polygons])
    profile = load_json(STEAMVR / next(path for prefix, path in PROFILES.items() if name.startswith(prefix)))
    inputs = {key.removeprefix("/input/"): spec for key, spec in profile["input_source"].items()
              if key.startswith("/input/") and spec.get("type") != "skeleton"
              and spec.get("side", hand) == hand}

    def input_name(button):
        if button == "grip_left" or button == "grip_right":
            return "grip"
        if button == "thumbstick" and "joystick" in inputs:
            return "joystick"
        if button == "menu":
            # Vive は application_menu。Touch 系の左メニューは SteamVR では system
            for candidate in ("menu", "application_menu", "system"):
                if candidate in inputs:
                    return candidate
            return None
        return button if button in inputs else None

    buttons = []

    def add(button, position, **extra):
        found = input_name(button)
        buttons.append({
            "id": button,
            "input": f"/user/hand/{hand}/input/{found}" if found else None,
            **position, **extra,
        })

    for part, (path, motion) in parts(name).items():
        verts, faces = read_obj(path)
        extra = {"approximate": True} if motion.get("approximate") else {}
        if part not in TOP_FACE:
            add(part, visible_face(verts, faces, bvh, proj), **extra)
            continue
        center, samples, weights, axis = top_face(part, verts, faces, motion, proj.view)
        if part == "dpad":
            for direction, arm in dpad_arms(center, samples, axis).items():
                near = np.linalg.norm(samples - arm, axis=1) < 0.004
                add(f"dpad_{direction}", point(arm, samples[near], weights[near], bvh, proj, False))
        else:
            add(part, point(center, samples, weights, bvh, proj, False), **extra)

    # 同じ物理ボタンを指す部品が 2 つある機種は 1 つにまとめる
    system = next((b for b in buttons if b["id"] == "system"), None)
    menu = next((b for b in buttons if b["id"] == "menu"), None)
    if system and menu and abs(system["x"] - menu["x"]) < 4 and abs(system["y"] - menu["y"]) < 4:
        menu["input"] = menu["input"] or system["input"]
        buttons.remove(system)
    # 入力名は 1 つのボタンにだけ割り当てる (Vive の左右グリップは同じ入力)。≡ を優先する
    used = set()
    for b in sorted(buttons, key=lambda b: b["id"] != "menu"):
        if b["input"] in used and not b["id"].startswith("grip_"):
            b["input"] = None
        if b["input"]:
            used.add(b["input"])
    names = {b["id"] for b in buttons}
    for b in buttons:
        # 右手の Meta / Oculus ボタンは部品名が menu でも、実体はシステムボタン
        if b["id"] == "menu" and not b["input"] and "system" not in names:
            b["id"] = "system"
        # PICO は ≡ と ○ のどちらが system かを SteamVR のデータから確定できない
        if profile["controller_type"] == "pico_controller" and b["id"] == "menu" and b["input"]:
            b["input_assumed"] = True

    mapped = {b["input"].rsplit("/input/")[1] for b in buttons if b["input"]}
    unmapped = [
        {"input": f"/user/hand/{hand}/input/{key}",
         "reason": "gesture" if key.startswith(NOT_PHYSICAL) else "no_render_model_component"}
        for key in inputs if key not in mapped
    ]
    return {
        "render_model": name,
        "render_model_aliases": ALIASES.get(name, []),
        "source_render_model": SOURCE_MODEL.get(name, name),
        "hand": hand,
        "controller_type": profile["controller_type"],
        "image": f"{hand}/{name}.png",
        "buttons": sorted(buttons, key=lambda b: b["id"]),
        "inputs_without_position": unmapped,
    }


def write(root, entry, width, height):
    """1 台ぶんを root/buttons/<hand>/<render_model>.json に書く。image は root からの相対パス。"""
    data = {
        "schema_version": 1,
        "coordinates": {"width": width, "height": height, "origin": "top-left",
                        "x_y": "pixels", "u_v": "normalized 0..1"},
        **entry,
    }
    path = Path(root) / "buttons" / entry["hand"] / f'{entry["render_model"]}.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
