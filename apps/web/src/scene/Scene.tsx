import { useEffect, useRef, useState } from "react";

import type { Locale } from "@/i18n/config";
import { reducedMotion } from "@/motion/reducedMotion";
import type { Theme } from "@/theme";
import { quickSettingsOpacity, shotAt } from "@/scene/path";

/** アプリのキャプチャ（apps/nyaw の `site` のテスト）が書き出すパネル */
type Panel = {
  role: "dashboard" | "mirroring" | "quick-settings" | "wrist";
  width: number;
  height: number;
  position: [number, number, number];
  orientation: [number, number, number, number];
  sort: number;
};

/** ミラーリング中の場面にない、クイック設定だけを別の場面から持ってくる */
const MOMENTS = {
  mirroring: ["dashboard", "mirroring", "wrist"],
  "quick-settings": ["quick-settings"],
};

export const sceneDir = (theme: Theme, locale: Locale, moment: keyof typeof MOMENTS) =>
  `/scene/${theme}-${locale}/${moment}`;

async function loadPanels(theme: Theme, locale: Locale) {
  const lists = await Promise.all(
    Object.entries(MOMENTS).map(async ([moment, roles]) => {
      const dir = sceneDir(theme, locale, moment as keyof typeof MOMENTS);
      const panels: Panel[] = await fetch(`${dir}/scene.json`).then((r) => r.json());
      return panels
        .filter((p) => roles.includes(p.role))
        .map((p) => ({ ...p, url: `${dir}/${p.role}.png` }));
    }),
  );
  return lists.flat();
}

/** CSS の色（oklch など）を three.js が読める rgb() にする */
function rgb(color: string) {
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return "#888";
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, 1, 1);
  const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
  return `rgb(${r}, ${g}, ${b})`;
}

type Props = {
  theme: Theme;
  locale: Locale;
  /** スクロールの進み（0〜1）。毎フレーム読む */
  progress: { current: number };
  label: string;
};

/** nyaw のパネルを、アプリと同じ位置と向きで three.js の空間に並べる */
export function Scene({ theme, locale, progress, label }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let disposed = false;
    let cleanup = () => {};
    void (async () => {
      const THREE = await import("three");
      if (disposed) return;
      let renderer: InstanceType<typeof THREE.WebGLRenderer>;
      try {
        renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
      } catch {
        setFailed(true);
        return;
      }
      renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      const scene = new THREE.Scene();
      const camera = new THREE.PerspectiveCamera(50, 1, 0.01, 50);

      // 足元の目安の格子。目の高さを 1.6m とする。色はキャンバスの文字色（枠線の色）
      const line = rgb(getComputedStyle(canvas).color);
      const grid = new THREE.GridHelper(8, 32, line, line);
      grid.position.y = -1.6;
      (grid.material as InstanceType<typeof THREE.Material>).transparent = true;
      (grid.material as InstanceType<typeof THREE.Material>).opacity = 0.6;
      scene.add(grid);

      const loader = new THREE.TextureLoader();
      const anisotropy = renderer.capabilities.getMaxAnisotropy();
      const meshes = new Map<Panel["role"], InstanceType<typeof THREE.Mesh>>();
      const panels = await loadPanels(theme, locale);
      const textures = await Promise.all(panels.map((p) => loader.loadAsync(p.url)));
      if (disposed) return renderer.dispose();
      panels.forEach((panel, i) => {
        const texture = textures[i];
        texture.colorSpace = THREE.SRGBColorSpace;
        texture.anisotropy = anisotropy;
        const material = new THREE.MeshBasicMaterial({
          map: texture,
          transparent: true,
          depthWrite: false,
          side: THREE.DoubleSide,
        });
        const mesh = new THREE.Mesh(new THREE.PlaneGeometry(panel.width, panel.height), material);
        mesh.position.set(...panel.position);
        mesh.quaternion.set(...panel.orientation);
        // アプリと同じく、重なり順はランタイムに渡した順序で決める
        mesh.renderOrder = panel.sort;
        meshes.set(panel.role, mesh);
        scene.add(mesh);
      });

      const pointer = { x: 0, y: 0 };
      const onPointer = (e: PointerEvent) => {
        pointer.x = (e.clientX / innerWidth) * 2 - 1;
        pointer.y = (e.clientY / innerHeight) * 2 - 1;
      };
      const still = reducedMotion();
      if (!still) addEventListener("pointermove", onPointer, { passive: true });

      const resize = () => {
        const { clientWidth: w, clientHeight: h } = canvas;
        renderer.setSize(w, h, false);
        camera.aspect = w / h;
        // 縦長の画面では、横幅に収まるよう画角を広げる
        camera.fov = camera.aspect < 1 ? 50 / Math.max(camera.aspect, 0.55) : 50;
        camera.updateProjectionMatrix();
      };
      const observer = new ResizeObserver(resize);
      observer.observe(canvas);
      resize();

      // 画面に入っている間だけ描く
      let visible = true;
      const io = new IntersectionObserver(([entry]) => (visible = entry?.isIntersecting ?? true));
      io.observe(canvas);

      const look = new THREE.Vector3();
      const sway = { x: 0, y: 0 };
      let frame = 0;
      const render = () => {
        frame = requestAnimationFrame(render);
        if (!visible) return;
        const p = progress.current;
        const shot = shotAt(p);
        sway.x += (pointer.x - sway.x) * 0.05;
        sway.y += (pointer.y - sway.y) * 0.05;
        camera.position.set(
          shot.position[0] + sway.x * 0.04,
          shot.position[1] - sway.y * 0.03,
          shot.position[2],
        );
        camera.lookAt(look.set(...shot.target));
        const quick = quickSettingsOpacity(p);
        const fade = (role: Panel["role"], opacity: number) => {
          const mesh = meshes.get(role);
          if (!mesh) return;
          (mesh.material as InstanceType<typeof THREE.MeshBasicMaterial>).opacity = opacity;
          mesh.visible = opacity > 0.001;
        };
        fade("quick-settings", quick);
        fade("mirroring", 1 - quick);
        renderer.render(scene, camera);
      };
      render();
      canvas.dataset.ready = "";

      cleanup = () => {
        cancelAnimationFrame(frame);
        removeEventListener("pointermove", onPointer);
        observer.disconnect();
        io.disconnect();
        for (const mesh of meshes.values()) {
          mesh.geometry.dispose();
          const material = mesh.material as InstanceType<typeof THREE.MeshBasicMaterial>;
          material.map?.dispose();
          material.dispose();
        }
        grid.geometry.dispose();
        renderer.dispose();
        delete canvas.dataset.ready;
      };
    })().catch(() => setFailed(true));
    return () => {
      disposed = true;
      cleanup();
    };
  }, [theme, locale, progress]);

  if (failed) {
    // WebGL が使えない環境では、ミラーリング中の画面だけを見せる
    return (
      <img
        src={`${sceneDir(theme, locale, "mirroring")}/mirroring.png`}
        alt={label}
        className="size-full object-contain p-6"
      />
    );
  }
  return (
    <canvas
      ref={canvasRef}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- 描いた絵を 1 枚の画像として読み上げさせる
      role="img"
      aria-label={label}
      className="size-full text-neutral-border"
    />
  );
}
